//! Source newline normalization gate.
//!
//! A program authored on Windows (CRLF) and the same program authored on Unix
//! (LF) must lower to byte-identical IR. This is a cross-platform baseline: if
//! newline bytes leak into string literals or source positions, downstream
//! golden digests and diagnostics drift by editor choice.

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
fn crlf_and_lf_sources_lower_to_identical_ir() {
    let dir = out_dir("source-eol");
    let lf = dir.join("lf.opu");
    let crlf = dir.join("crlf.opu");
    let lf_ir = dir.join("lf.opuir.json");
    let crlf_ir = dir.join("crlf.opuir.json");
    let lines = [
        "app Eol {",
        "  state count = 0",
        "  Text(\"value: {count}\")",
        "  Button(\"+\") { count += 1 }",
        "}",
    ];
    fs::write(&lf, format!("{}\n", lines.join("\n"))).expect("write LF source");
    fs::write(&crlf, format!("{}\r\n", lines.join("\r\n"))).expect("write CRLF source");

    for (src, out) in [(&lf, &lf_ir), (&crlf, &crlf_ir)] {
        let result = bin()
            .arg("compile")
            .arg(src)
            .arg("-o")
            .arg(out)
            .output()
            .expect("run opuromc compile");
        assert!(
            result.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&lf_ir).expect("read LF IR"),
        fs::read(&crlf_ir).expect("read CRLF IR"),
        "newline style must not change IR bytes"
    );
}
