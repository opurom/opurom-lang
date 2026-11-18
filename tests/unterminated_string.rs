//! Unterminated string diagnostic gate.
//!
//! A single-line string must end on the same source line. Dart's grammar for
//! `singleLineStringLiteral` uses matching quotes, while multiline strings use
//! triple quotes (`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:46498-46500`,
//! SDK 3.14.0, Flutter 3.50.0). Opurom accepts only the single-line form, so
//! an unclosed quote must report the opening position and must never write IR.

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

fn assert_bad(src: &PathBuf, out: &PathBuf, line_col: &str, tail: &str) {
    let result = compile(src, out);
    assert_eq!(
        result.status.code(),
        Some(1),
        "unterminated string must fail"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains(line_col) && stderr.contains(tail),
        "unexpected diagnostic: {stderr}"
    );
    assert!(!out.exists(), "failed compile must not write IR");
}

#[test]
fn unterminated_string_at_end_of_line_reports_opening_position() {
    let dir = out_dir("unterminated-eol");
    let src = dir.join("eol.opu");
    let out = dir.join("eol.opuir.json");
    fs::write(&src, "app S {\n  Text(\"abc)\n}\n").expect("write source");
    assert_bad(
        &src,
        &out,
        ":2:8: error: unterminated string literal",
        "before the end of the line",
    );
}

#[test]
fn unterminated_string_at_end_of_file_reports_opening_position() {
    let dir = out_dir("unterminated-eof");
    let src = dir.join("eof.opu");
    let out = dir.join("eof.opuir.json");
    fs::write(&src, "app S {\n  Text(\"abc").expect("write source");
    assert_bad(
        &src,
        &out,
        ":2:8: error: unterminated string literal",
        "before the end of the file",
    );
}

#[test]
fn trailing_escape_does_not_hide_unterminated_string() {
    let dir = out_dir("unterminated-escape");
    let src = dir.join("escape.opu");
    let out = dir.join("escape.opuir.json");
    fs::write(&src, "app S {\n  Text(\"abc\\").expect("write source");
    assert_bad(
        &src,
        &out,
        ":2:8: error: unterminated string literal",
        "before the end of the file",
    );
}

#[test]
fn escaped_quote_and_escaped_backslash_close_normally() {
    let dir = out_dir("unterminated-escape-ok");
    let src = dir.join("escape-ok.opu");
    let out = dir.join("escape-ok.opuir.json");
    fs::write(&src, "app S {\n  Text(\"a\\\" b and c\\\\ d\")\n}\n").expect("write source");
    let result = compile(&src, &out);
    assert!(
        result.status.success(),
        "valid escapes failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir = fs::read_to_string(&out).expect("read IR");
    assert!(ir.contains("a\\\" b and c\\\\ d"), "{ir}");
}

#[test]
fn unterminated_string_failure_keeps_previous_ir() {
    let dir = out_dir("unterminated-rollback");
    let src = dir.join("rollback.opu");
    let out = dir.join("rollback.opuir.json");
    fs::write(&src, "app S { Text(\"good\") }\n").expect("write good source");
    let good = compile(&src, &out);
    assert!(
        good.status.success(),
        "good compile failed: {}",
        String::from_utf8_lossy(&good.stderr)
    );
    let before = fs::read(&out).expect("read good IR");

    fs::write(&src, "app S { Text(\"bad) }\n").expect("write bad source");
    let bad = compile(&src, &out);
    assert_eq!(bad.status.code(), Some(1), "bad compile must fail");
    assert_eq!(
        before,
        fs::read(&out).expect("read IR after failure"),
        "failed compile must not overwrite last good IR"
    );
}
