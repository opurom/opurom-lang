//! Optimizer in-place failure safety gate.
//!
//! `opuromc optimize` may be called with the same path as input and output.
//! Validation happens before writing, but an interrupted or partial write must
//! never destroy the last usable IR. The command writes a sibling `.tmp` file
//! and renames it only after serialization succeeds, so validation and write
//! failures keep the original bytes.

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
    dir
}

#[test]
fn invalid_ir_optimize_in_place_keeps_original_bytes() {
    let dir = out_dir("optimize-in-place");
    let src = dir.join("source.opu");
    let ir = dir.join("source.opuir.json");
    fs::write(&src, "app S {\n  Column {\n    Text(\"x\")\n  }\n}\n").expect("write source");
    let compiled = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let mut doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    doc["tree"]["children"][0]["bogus"] = serde_json::json!(1);
    fs::write(&ir, serde_json::to_vec(&doc).expect("serialize invalid IR"))
        .expect("write invalid IR");
    let before = fs::read(&ir).expect("read invalid IR bytes");

    let result = bin()
        .arg("optimize")
        .arg(&ir)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc optimize");
    assert_eq!(result.status.code(), Some(1), "invalid IR must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("unknown IR node field 'bogus'"),
        "unexpected diagnostic: {stderr}"
    );
    assert_eq!(
        before,
        fs::read(&ir).expect("read IR after failed optimize"),
        "failed in-place optimize must not modify the artifact"
    );
    assert!(
        !ir.with_extension("json.tmp").exists(),
        "temporary file leaked"
    );
}
