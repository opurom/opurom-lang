//! Diagnostic position stability gate.
//!
//! Positions are character columns, not UTF-8 byte offsets. Dart's `LineInfo`
//! computes a location as `offset - lineStart + 1` over UTF-16 code units and
//! treats `\\r`, `\\n` and `\\r\\n` as line endings (analyzer
//! `source/line_info.dart:45-69`, `:78-93`, SDK 3.14.0, Flutter 3.50.0). Opurom
//! uses Unicode scalar values, so BMP characters have the same one-column
//! weight; a leading BOM is metadata and must not shift the first column.

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

fn check(src: &PathBuf) -> String {
    let result = bin()
        .arg("check")
        .arg(src)
        .output()
        .expect("run opuromc check");
    assert_eq!(result.status.code(), Some(1), "check must fail");
    String::from_utf8_lossy(&result.stderr).into_owned()
}

fn assert_pos(stderr: &str, line_col: &str) {
    assert!(
        stderr.contains(line_col),
        "missing {line_col} in diagnostic: {stderr}"
    );
    assert!(
        !stderr.contains("byte") && !stderr.contains("\\u{"),
        "diagnostic must stay in source coordinates: {stderr}"
    );
}

#[test]
fn unicode_and_bmp_prefixes_keep_character_columns() {
    let dir = out_dir("diagnostic-position-unicode");
    let src = dir.join("unicode.opu");
    fs::write(
        &src,
        "app Pos {\n  Text(\"你好 café\")\n  Text(\"abc\", bogus: 1)\n}\n",
    )
    .expect("write source");
    assert_pos(&check(&src), ":3:22: error: unknown prop 'bogus' on Text");
}

#[test]
fn bom_crlf_and_mixed_eol_keep_line_and_column_positions() {
    let dir = out_dir("diagnostic-position-eol");
    let crlf = dir.join("crlf.opu");
    let mixed = dir.join("mixed.opu");
    let body = "app Pos {\r\n  Text(\"你好 café\")\r\n  NotAComponent(\"x\")\r\n}\r\n";
    fs::write(&crlf, body).expect("write CRLF source");
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(body.as_bytes());
    fs::write(&mixed, with_bom).expect("write BOM CRLF source");
    let expected = ":3:3: error: unknown node kind 'NotAComponent'";
    assert_pos(&check(&crlf), expected);
    assert_pos(&check(&mixed), expected);
}

#[test]
fn bare_cr_line_breaks_advance_the_diagnostic_line() {
    let dir = out_dir("diagnostic-position-bare-cr");
    let src = dir.join("cr.opu");
    fs::write(
        &src,
        "app Pos {\r  Text(\"你好 café\")\r  NotAComponent(\"x\")\r}\r",
    )
    .expect("write CR source");
    assert_pos(
        &check(&src),
        ":3:3: error: unknown node kind 'NotAComponent'",
    );
}
