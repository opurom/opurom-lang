//! Diagnostic parity between `opuromc check` and `opuromc compile`.
//!
//! Both commands share the same front end. For the same source, a rejected
//! program must produce byte-identical stderr from both entry points so IDE
//! diagnostics and build failures cannot disagree on message or position.

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
fn check_and_compile_emit_identical_diagnostics() {
    let dir = out_dir("diagnostic-parity");
    let src = dir.join("bad.opu");
    fs::write(
        &src,
        "app Bad {\n  state count = 0\n  computed broken = count + missing\n  Text(\"{broken}\")\n}\n",
    )
    .expect("write source");

    let check = bin()
        .arg("check")
        .arg(&src)
        .output()
        .expect("run opuromc check");
    let compile = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&dir.join("out.opuir.json"))
        .output()
        .expect("run opuromc compile");

    assert_eq!(check.status.code(), Some(1));
    assert_eq!(compile.status.code(), Some(1));
    assert_eq!(
        check.stderr, compile.stderr,
        "check and compile must report the same diagnostic"
    );
    assert!(
        !dir.join("out.opuir.json").exists(),
        "a rejected program must not leave an IR artifact"
    );
}
