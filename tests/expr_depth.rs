//! Computed expression recursion boundary gate.
//!
//! Dart's analyzer represents a binary operation as an AST node with two
//! operand slots (`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:3978-4000`,
//! SDK 3.14.0, Flutter 3.50.0). Opurom's left-deep AST and recursive type
//! inference need the same shape, so an unbounded chain can exhaust the native
//! stack. The compiler caps one computed expression at 512 operands/operators
//! and turns longer chains into a positioned diagnostic without writing IR.

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

fn source(op: &str, count: usize) -> String {
    format!(
        "app DeepChain {{\n  state n = 0\n  computed sum = {}\n  Text(\"{{sum}}\")\n}}\n",
        std::iter::repeat("1")
            .take(count)
            .collect::<Vec<_>>()
            .join(op)
    )
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
fn add_chain_accepts_512_operands_and_rejects_513() {
    let dir = out_dir("expr-depth-add");
    let ok = dir.join("ok.opu");
    let bad = dir.join("bad.opu");
    let out = dir.join("bad.opuir.json");
    fs::write(&ok, source(" + ", 512)).expect("write accepted source");
    fs::write(&bad, source(" + ", 513)).expect("write rejected source");

    let accepted = compile(&ok, &dir.join("ok.opuir.json"));
    assert!(
        accepted.status.success(),
        "512 operands must compile: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let rejected = compile(&bad, &out);
    assert_eq!(rejected.status.code(), Some(1), "513 operands must fail");
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        stderr.contains(":3:2066: error: computed expression exceeds 512 operands")
            && stderr.contains("split the expression into declared computed values"),
        "depth diagnostic: {stderr}"
    );
    assert!(!out.exists(), "failed deep compile must not write IR");
}

#[test]
fn mul_chain_has_the_same_stable_boundary() {
    let dir = out_dir("expr-depth-mul");
    let src = dir.join("mul.opu");
    let out = dir.join("mul.opuir.json");
    fs::write(&src, source(" * ", 1600)).expect("write deep multiplication");
    let result = compile(&src, &out);
    assert_eq!(
        result.status.code(),
        Some(1),
        "deep multiplication must fail"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("computed expression exceeds 512 operands") && !out.exists(),
        "deep multiplication diagnostic: {stderr}"
    );
}

#[test]
fn nested_negation_reports_nesting_limit_without_panic() {
    let dir = out_dir("expr-depth-not");
    let src = dir.join("not.opu");
    let out = dir.join("not.opuir.json");
    let text = format!(
        "app DeepChain {{\n  state f = true\n  computed v = {}f\n  Text(\"{{v}}\")\n}}\n",
        "!".repeat(513)
    );
    fs::write(&src, text).expect("write deep negation");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "deep negation must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("computed expression nesting exceeds 512 operators")
            && stderr.contains("  help: ")
            && !out.exists(),
        "deep negation diagnostic: {stderr}"
    );
}

#[test]
fn extreme_add_chain_reports_diagnostic_instead_of_stack_overflow() {
    let dir = out_dir("expr-depth-extreme");
    let src = dir.join("extreme.opu");
    let out = dir.join("extreme.opuir.json");
    fs::write(&src, source(" + ", 1600)).expect("write extreme source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "extreme chain must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("computed expression exceeds 512 operands")
            && !stderr.contains("stack overflow")
            && !out.exists(),
        "extreme chain diagnostic: {stderr}"
    );
}
