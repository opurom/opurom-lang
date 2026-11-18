//! Computed long-chain splitting gate.
//!
//! Dart evaluates a binary expression from two operand slots
//! (`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:3978-4000`,
//! SDK 3.14.0, Flutter 3.50.0). Opurom's parser bounds one computed expression
//! at 512 operands (`tests/expr_depth.rs`). Splitting a long arithmetic chain
//! into several declared computed values would be the natural relief, but the
//! current protocol keeps computed values read-only to each other: a later
//! computed expression cannot read an earlier one. This gate pins that
//! boundary and verifies that the 512-operand single-expression limit is the
//! only supported shape today.

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

#[test]
fn later_computed_value_cannot_read_earlier_one() {
    let dir = out_dir("expr-split");
    let src = dir.join("split.opu");
    let out = dir.join("split.opuir.json");
    fs::write(
        &src,
        "app Split {\n  state a = 0\n  computed first = a + 1\n  computed second = first + 2\n  Text(\"{second}\")\n}\n",
    )
    .expect("write source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "chained computed must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("computed expression cannot read computed value 'first'")
            && stderr.contains("computed-to-computed references are not supported yet"),
        "chain diagnostic: {stderr}"
    );
    assert!(!out.exists(), "rejected split must not write IR");
}

#[test]
fn grouped_long_chain_is_not_a_supported_alternative() {
    let dir = out_dir("expr-split-paren");
    let src = dir.join("paren.opu");
    let out = dir.join("paren.opuir.json");
    fs::write(
        &src,
        "app Split {\n  state a = 0\n  computed total = (a + 1) * 2\n  Text(\"{total}\")\n}\n",
    )
    .expect("write source");
    let result = compile(&src, &out);
    assert_eq!(
        result.status.code(),
        Some(1),
        "parenthesized form must fail"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("found '('"), "paren diagnostic: {stderr}");
    assert!(
        !out.exists(),
        "rejected parenthesized form must not write IR"
    );
}

#[test]
fn single_computed_chain_still_supports_512_operands() {
    let dir = out_dir("expr-split-ok");
    let src = dir.join("single.opu");
    let out = dir.join("single.opuir.json");
    let expr = std::iter::repeat("1")
        .take(512)
        .collect::<Vec<_>>()
        .join(" + ");
    fs::write(
        &src,
        format!("app Split {{\n  computed sum = {expr}\n  Text(\"{{sum}}\")\n}}\n"),
    )
    .expect("write source");
    let result = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--no-fold")
        .output()
        .expect("run opuromc compile --no-fold");
    assert!(
        result.status.success(),
        "512 operands must compile: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
    assert_eq!(doc["computed"][0]["expr"], expr);
    assert_eq!(doc["state"][0]["init"], 512);
}
