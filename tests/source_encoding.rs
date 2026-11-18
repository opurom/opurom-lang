//! Source encoding robustness gate.
//!
//! A leading UTF-8 BOM is metadata, not source syntax: Dart's UTF-8 decoder
//! discards it (`convert/utf.dart:48-49`, SDK 3.14.0, Flutter 3.50.0).
//! Malformed bytes are rejected, never replaced by U+FFFD
//! (`convert/utf.dart:33-39`). Mixed line endings must not perturb the IR
//! because the lexer treats every Unicode whitespace byte as a separator.

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

fn compile(src: &PathBuf, out: &PathBuf) -> std::process::Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
}

fn source(lines: &[&str], eol: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        bytes.extend_from_slice(line.as_bytes());
        if index + 1 < lines.len() {
            bytes.extend_from_slice(eol[index % eol.len()].as_bytes());
        }
    }
    bytes.push(b'\n');
    bytes
}

#[test]
fn bom_source_matches_plain_utf8_ir() {
    let dir = out_dir("source-encoding-bom");
    let plain = dir.join("plain.opu");
    let with_bom = dir.join("with-bom.opu");
    let plain_ir = dir.join("plain.opuir.json");
    let bom_ir = dir.join("bom.opuir.json");
    let text = "app Encoding { state n = 0 Text(\"你好: {n}\") }\n".as_bytes();
    fs::write(&plain, text).expect("write plain source");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(text);
    fs::write(&with_bom, bytes).expect("write BOM source");
    for (src, out) in [(&plain, &plain_ir), (&with_bom, &bom_ir)] {
        let result = compile(src, out);
        assert!(
            result.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&plain_ir).expect("read plain IR"),
        fs::read(&bom_ir).expect("read BOM IR"),
        "leading BOM must not change IR bytes"
    );
}

#[test]
fn invalid_utf8_fails_with_positioned_diagnostic_and_keeps_ir() {
    let dir = out_dir("source-encoding-invalid");
    let src = dir.join("invalid.opu");
    let ir = dir.join("kept.opuir.json");
    fs::write(&src, b"app Encoding { state n = 0 Text(\"n: {n}\") }\n")
        .expect("write valid source");
    let good = compile(&src, &ir);
    assert!(
        good.status.success(),
        "good compile failed: {}",
        String::from_utf8_lossy(&good.stderr)
    );
    let before = fs::read(&ir).expect("read good IR");

    let mut bytes = b"app Encoding { state n = 0 Text(\"n: {n}\") }\n".to_vec();
    bytes.insert(4, 0xFF);
    fs::write(&src, bytes).expect("write malformed source");
    let bad = compile(&src, &ir);
    assert_eq!(bad.status.code(), Some(1), "malformed UTF-8 must fail");
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(
        stderr.contains(":1:1: error: source is not valid UTF-8") && stderr.contains("  help: "),
        "malformed source needs a positioned diagnostic: {stderr}"
    );
    assert_eq!(
        before,
        fs::read(&ir).expect("read IR after failure"),
        "failed compile must not overwrite last good IR"
    );
}

#[test]
fn mixed_line_endings_match_normalized_lf_ir() {
    let dir = out_dir("source-encoding-eol");
    let mixed = dir.join("mixed.opu");
    let lf = dir.join("lf.opu");
    let mixed_ir = dir.join("mixed.opuir.json");
    let lf_ir = dir.join("lf.opuir.json");
    let lines = [
        "app Encoding {",
        "  state n = 0",
        "  Column { Text(\"n: {n}\") }",
        "}",
    ];
    fs::write(&mixed, source(&lines, &["\n", "\r\n", "\r"])).expect("write mixed source");
    fs::write(&lf, source(&lines, &["\n"])).expect("write LF source");
    for (src, out) in [(&mixed, &mixed_ir), (&lf, &lf_ir)] {
        let result = compile(src, out);
        assert!(
            result.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&mixed_ir).expect("read mixed IR"),
        fs::read(&lf_ir).expect("read LF IR"),
        "line ending style must not change IR bytes"
    );
}
