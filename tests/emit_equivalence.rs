//! Raw and optimized IR emitter equivalence gate.
//!
//! Flutter's depfile reader keeps build data stable by normalizing paths and
//! rejecting duplicates (`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`,
//! Flutter 3.50.0). Opurom applies the same expectation to the release
//! optimizer: after constant folding and constant-visibility pruning, each of
//! the ten emitters must preserve the rendered program. This gate compares a
//! raw lowered IR with the optimized IR by emitting both and checking that all
//! literal and computed state values reached the generated sources.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const TARGETS: &[&str] = &[
    "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
];

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn out_dir(label: &str) -> PathBuf {
    let dir = workspace().join("target/test-out").join(format!(
        "{}-{}",
        label,
        CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create test output dir");
    dir
}

fn emit(ir: &PathBuf, target: &str, out: &PathBuf) {
    let result = bin()
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc target");
    assert!(
        result.status.success(),
        "{target} emit failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn emit_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("read emit dir") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.is_file() {
                let name = path
                    .strip_prefix(base)
                    .expect("path under emit dir")
                    .to_string_lossy()
                    .into_owned();
                out.insert(name, fs::read(&path).expect("read artifact"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn folded_ir_preserves_visible_values_on_every_target() {
    let dir = out_dir("emit-equivalence");
    let src = dir.join("source.opu");
    let raw = dir.join("raw.opuir.json");
    let optimized = dir.join("optimized.opuir.json");
    fs::write(
        &src,
        "app Equivalent {\n  state visible = true\n  state hidden = false\n  state count = 0\n  computed total = 2 + 3 * 4\n  Column(repaintBoundary: true) {\n    Text(\"constant: {total} / {visible}\")\n    if (hidden) { Text(\"hidden\") }\n    Button(\"+\") { count += 1 }\n  }\n}\n",
    )
    .expect("write source");
    for (flag, out) in [("--no-fold", &raw), ("", &optimized)] {
        let mut command = bin();
        command.arg("compile").arg(&src).arg("-o").arg(out);
        if !flag.is_empty() {
            command.arg(flag);
        }
        let result = command.output().expect("run opuromc compile");
        assert!(
            result.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let raw_doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&raw).expect("read raw IR")).expect("parse raw IR");
    let opt_doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&optimized).expect("read optimized IR"))
            .expect("parse optimized IR");
    let raw_expr = raw_doc["computed"][0]["expr"].as_str().expect("raw expr");
    let opt_expr = opt_doc["computed"][0]["expr"]
        .as_str()
        .expect("optimized expr");
    assert_ne!(raw_expr, opt_expr, "test requires a real fold");
    assert_eq!(opt_expr, "14", "constant expression must fold to 14");
    let opt_text = opt_doc["tree"]["children"][0]["props"]["value"]
        .as_str()
        .expect("optimized text");
    assert_eq!(opt_text, "constant: 14 / true", "visible text must fold in");

    for target in TARGETS {
        let raw_out = dir.join(format!("raw-{target}"));
        let opt_out = dir.join(format!("opt-{target}"));
        emit(&raw, target, &raw_out);
        emit(&optimized, target, &opt_out);
        let raw_files = emit_tree(&raw_out);
        let opt_files = emit_tree(&opt_out);
        assert_eq!(
            raw_files.keys().collect::<Vec<_>>(),
            opt_files.keys().collect::<Vec<_>>(),
            "{target}: fold changed artifact set"
        );
        let all = raw_files
            .values()
            .chain(opt_files.values())
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["14", "constant", "true", "hidden", "count"] {
            assert!(
                all.contains(needle),
                "{target}: generated sources lost semantic marker {needle}"
            );
        }
    }
}
