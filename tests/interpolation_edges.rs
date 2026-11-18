//! State interpolation boundary gate.
//!
//! `interp_parts` treats `{{` as a literal brace, `{name}` as a state read, and
//! rejects an unterminated brace with a source-relative help message. These
//! rules are shared by node props and locale catalogs, so a regression would
//! silently change rendered text on every emitter.

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
fn doubled_brace_stays_literal_text() {
    let dir = out_dir("interp-literal");
    let src = dir.join("literal.opu");
    let ir = dir.join("literal.opuir.json");
    fs::write(&src, "app Literal { Text(\"{{literal}}\") }\n").expect("write source");
    let result = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(
        result.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["props"]["value"], "{{literal}}");
}

#[test]
fn unterminated_interpolation_has_positioned_diagnostic_and_no_artifact() {
    let dir = out_dir("interp-unterminated");
    let src = dir.join("bad.opu");
    let ir = dir.join("bad.opuir.json");
    fs::write(&src, "app Bad { Text(\"bad {\") }\n").expect("write source");
    let result = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("unterminated '{name}' interpolation"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(
        stderr.contains(":1:21: error:"),
        "diagnostic must carry source position: {stderr}"
    );
    assert!(
        stderr.contains("write '{{' for a literal '{'"),
        "help must name the literal escape: {stderr}"
    );
    assert!(!ir.exists(), "no IR may be written on a rejected source");
}
