//! Failed compilation keeps the last usable artifact.
//!
//! The dev loop rolls back to the previous good build when a source edit fails
//! to compile. That guarantee only holds if the compiler never writes a partial
//! IR on failure. `opuromc` lowers to memory and writes the output only after a
//! successful pass (`main.rs`, `compile` command), so an existing artifact must
//! remain byte-identical after a rejected compile.

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
fn compile_failure_keeps_previous_ir_byte_identical() {
    let dir = out_dir("compile-rollback");
    let src = dir.join("main.opu");
    let ir = dir.join("main.opuir.json");

    fs::write(
        &src,
        "app Rollback {\n  state count = 0\n  Text(\"good: {count}\")\n}\n",
    )
    .expect("write good source");
    let good = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run good compile");
    assert!(
        good.status.success(),
        "good compile failed: {}",
        String::from_utf8_lossy(&good.stderr)
    );
    let before = fs::read(&ir).expect("read good IR");

    fs::write(
        &src,
        "app Rollback {\n  state count = 0\n  computed bad = count + missing\n  Text(\"bad: {bad}\")\n}\n",
    )
    .expect("write bad source");
    let bad = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run bad compile");
    assert_eq!(bad.status.code(), Some(1), "bad compile must fail");
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(
        stderr.contains(": error: ") && stderr.contains("  help: "),
        "bad compile needs a positioned diagnostic: {stderr}"
    );

    let after = fs::read(&ir).expect("read IR after failure");
    assert_eq!(
        before, after,
        "failed compile must not overwrite last good IR"
    );
}
