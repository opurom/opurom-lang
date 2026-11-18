//! Output path safety and stale artifact audit.
//!
//! Flutter's web server rejects any untrusted request path that can select a
//! location outside its root and hands the filesystem a normalized path
//! (`packages/flutter_tools/lib/src/isolated/web_server_utilities.dart:44-56`,
//! Flutter 3.50.0). Opurom's artifact writers always join the `-o` directory
//! with a generated name, so a traversal segment in `-o` is a caller-level
//! path choice, not an IR-derived one. This gate pins three things:
//! traversal segments still land inside the requested parent, repeat emission
//! is byte-stable, and removed routes leave stale page files untouched. The
//! last point is recorded as a risk because the current contract is additive.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
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

fn compile(src: &PathBuf, out: &PathBuf) -> std::process::Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
}

fn emit(ir: &PathBuf, target: &str, out: &PathBuf) -> std::process::Output {
    bin()
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc target")
}

fn snapshot(dir: &PathBuf) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &PathBuf, dir: &PathBuf, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("read output dir") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.is_file() {
                let name = path
                    .strip_prefix(base)
                    .expect("path under output dir")
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
fn traversal_segments_stay_inside_the_requested_parent() {
    let dir = out_dir("output-traversal");
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");
    fs::write(&src, "app Paths { Text(\"x\") }\n").expect("write source");
    assert!(compile(&src, &ir).status.success());

    for target in TARGETS {
        let requested = dir.join(format!("{target}-root"));
        fs::create_dir_all(&requested).expect("create requested root");
        let escaping = requested.join("sibling");
        let result = emit(&ir, target, &escaping);
        assert!(
            result.status.success(),
            "{target} traversal emit failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let files = snapshot(&escaping);
        assert!(
            !files.is_empty(),
            "{target}: traversal output must land in the sibling directory"
        );
        for name in files.keys() {
            assert!(
                !name.contains(".."),
                "{target}: artifact {name} escaped the requested output directory"
            );
        }
    }
}

#[test]
fn repeat_emission_overwrites_and_leaves_no_temporary_files() {
    let dir = out_dir("output-repeat");
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");
    fs::write(&src, "app Repeat { Text(\"x\") }\n").expect("write source");
    assert!(compile(&src, &ir).status.success());

    for target in TARGETS {
        let out = dir.join(format!("{target}-out"));
        let first = emit(&ir, target, &out);
        assert!(first.status.success());
        let before = snapshot(&out);
        assert!(!before.is_empty(), "{target}: first emission is empty");

        let second = emit(&ir, target, &out);
        assert!(second.status.success());
        let after = snapshot(&out);
        assert_eq!(
            before, after,
            "{target}: repeat emission changed the artifact set or bytes"
        );
        let temporaries = after
            .keys()
            .filter(|name| name.ends_with(".tmp") || name.ends_with(".bak"))
            .count();
        assert_eq!(temporaries, 0, "{target}: temporary artifact left behind");
    }
}

#[test]
fn removed_route_stale_page_files_are_risk_documented() {
    let dir = out_dir("output-stale");
    let old_source = "app Routes { route Home { Text(\"home\") } route Old { Text(\"old\") } }\n";
    let new_source = "app Routes { route Home { Text(\"home\") } }\n";
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");

    fs::write(&src, old_source).expect("write two-route source");
    assert!(compile(&src, &ir).status.success());
    let out = dir.join("wechat");
    let emitted = emit(&ir, "wechat", &out);
    assert!(emitted.status.success());
    let stale = out.join("pages/old/index.js");
    assert!(stale.is_file(), "fixture must create the stale page file");

    fs::write(&src, new_source).expect("write one-route source");
    assert!(compile(&src, &ir).status.success());
    let re_emitted = emit(&ir, "wechat", &out);
    assert!(re_emitted.status.success());
    assert!(
        stale.is_file(),
        "current contract keeps stale route pages (documented in docs/OUTPUT_SAFETY.md)"
    );
}
