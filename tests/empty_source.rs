//! Empty-source robustness gate.
//!
//! An empty file and a comment-only file are common editor states. Both must
//! fail with a positioned diagnostic, never panic, and never leave a partial
//! IR artifact behind. The diagnostics may differ in line but must share the
//! same error and help text.

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
fn empty_and_comment_only_sources_fail_cleanly() {
    let dir = out_dir("empty-source");
    let cases = [("empty", ""), ("comment", "// only a comment\n")];
    for (name, source) in cases {
        let src = dir.join(format!("{name}.opu"));
        let ir = dir.join(format!("{name}.opuir.json"));
        fs::write(&src, source).expect("write source");
        let result = bin()
            .arg("compile")
            .arg(&src)
            .arg("-o")
            .arg(&ir)
            .output()
            .expect("run opuromc compile");
        assert_eq!(result.status.code(), Some(1), "{name} must fail");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("expected the 'app' keyword, found end of file"),
            "{name}: unexpected diagnostic: {stderr}"
        );
        assert!(
            stderr.contains("  help: "),
            "{name}: missing help line: {stderr}"
        );
        assert!(
            !stderr.contains("panicked") && !stderr.contains("thread '"),
            "{name}: compiler crashed: {stderr}"
        );
        assert!(!ir.exists(), "{name}: no IR may be written on error");
    }
}
