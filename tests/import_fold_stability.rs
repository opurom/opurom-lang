//! Fold pipeline stability after import merge.
//!
//! Flutter's build system treats build hashes as reproducible noise-free data
//! (`packages/flutter_tools/lib/src/build_system/hash.dart:7-9`, Flutter 3.50.0).
//! Opurom's release path must give the same guarantee after declarations arrive
//! from several modules. This gate builds one program with two imported states
//! and a constant computed value, then compares raw lowering, the default
//! compile output, `--full-fold` optimize output and a second optimize pass.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
    fs::create_dir_all(dir.join("modules")).expect("create module root");
    fs::write(
        dir.join("opurom.json"),
        "{\"name\":\"fold-merge\",\"version\":\"0.1.0\",\"moduleRoots\":[\"modules\"],\"dependencies\":{}}",
    )
    .expect("write manifest");
    fs::write(dir.join("a.opu"), "state a = 1\n").expect("write module a");
    fs::write(dir.join("b.opu"), "state b: int = 3\n").expect("write module b");
    fs::write(
        dir.join("main.opu"),
        "import \"a.opu\"\nimport \"b.opu\"\n\napp Folded {\n  computed ca = a + 2\n  computed cb = b * 4\n  computed total = 2 + 3 * 4\n  Column {\n    Text(\"{ca} {cb} {total}\")\n    Button(\"inc\") { a += 1 }\n  }\n}\n",
    )
    .expect("write source");
    dir
}

fn run(dir: &PathBuf, args: &[&str]) -> Vec<u8> {
    let mut command = bin();
    for arg in args {
        command.arg(arg);
    }
    let result = command.current_dir(dir).output().expect("run opuromc");
    assert!(
        result.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::read(dir.join("out.json")).expect("read IR")
}

#[test]
fn imported_state_fold_pipeline_is_byte_stable() {
    let dir = out_dir("fold-import-stability");
    let src = dir.join("main.opu").to_string_lossy().into_owned();
    let out = dir.join("out.json").to_string_lossy().into_owned();

    let raw = run(&dir, &["compile", &src, "-o", &out, "--no-fold"]);
    let default = run(&dir, &["compile", &src, "-o", &out]);
    let full = run(&dir, &["optimize", &out, "-o", &out, "--full-fold"]);
    let full_again = run(&dir, &["optimize", &out, "-o", &out, "--full-fold"]);

    assert_ne!(raw, default, "constant folding must change raw IR");
    assert_eq!(
        default, full,
        "default compile must match full-fold optimize after import merge"
    );
    assert_eq!(
        full, full_again,
        "second full-fold optimize must be a byte no-op"
    );
}

#[test]
fn imported_states_keep_their_order_and_folded_values() {
    let dir = out_dir("fold-import-values");
    let src = dir.join("main.opu").to_string_lossy().into_owned();
    let out = dir.join("out.json").to_string_lossy().into_owned();
    run(&dir, &["compile", &src, "-o", &out]);

    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("out.json")).expect("read IR"))
            .expect("parse IR");
    let states: Vec<(&str, serde_json::Value)> = doc["state"]
        .as_array()
        .expect("state array")
        .iter()
        .filter_map(|s| Some((s["name"].as_str()?, s["init"].clone())))
        .collect();
    assert_eq!(states[0].0, "a");
    assert_eq!(states[1].0, "b");
    assert_eq!(states[2], ("ca", serde_json::json!(3)));
    assert_eq!(states[3], ("cb", serde_json::json!(12)));
    assert_eq!(states[4], ("total", serde_json::json!(14)));
    let total = doc["computed"]
        .as_array()
        .expect("computed array")
        .iter()
        .find(|c| c["name"] == "total")
        .expect("total computed");
    assert_eq!(total["expr"], "14");
}
