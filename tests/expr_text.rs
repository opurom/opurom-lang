//! Emitter conformance for computed-expression IR text.
//!
//! The IR `computed.expr` slot is produced by `Expr::text` in `parser.rs`:
//! a string literal keeps its spaces (`:51`) and a negation binds to its
//! operand without a space (`:58`). Every emitter re-parses that same text
//! through `emit::computed_expr`, so a token shape that works for one target
//! must work for all of them. These tests pin the two source shapes that the
//! previous whitespace tokenizer could not read.

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

fn compile(source: &str) -> (PathBuf, PathBuf) {
    let dir = out_dir("expr-text");
    let src = dir.join("probe.opu");
    let ir = dir.join("probe.opuir.json");
    fs::write(&src, source).expect("write source");
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
    (dir, ir)
}

fn emit(ir: &PathBuf, target: &str, dir: &PathBuf) {
    let result = bin()
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(dir)
        .output()
        .expect("run opuromc target");
    assert!(
        result.status.success(),
        "{target} emit failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn string_literal_with_spaces_emits_on_every_target() {
    let (dir, ir) = compile(
        "app Probe {\n  state label = \"a b\"\n  computed same = label == \"a b\"\n  Column {\n    Text(\"{same}\")\n  }\n}\n",
    );
    for target in ["web", "native", "wechat", "harmony"] {
        emit(&ir, target, &dir.join(target));
    }
    let web = fs::read_to_string(dir.join("web/app.js")).expect("read web app");
    assert!(
        web.contains("state.same = (state.label == \"a b\");"),
        "{web}"
    );
}

#[test]
fn negation_without_space_emits_on_every_target() {
    let (dir, ir) = compile(
        "app Probe {\n  state flag = true\n  computed inverse = !flag\n  Column {\n    Text(\"{inverse}\")\n  }\n}\n",
    );
    for target in ["web", "native", "wechat", "harmony"] {
        emit(&ir, target, &dir.join(target));
    }
    let web = fs::read_to_string(dir.join("web/app.js")).expect("read web app");
    assert!(web.contains("state.inverse = !state.flag;"), "{web}");
}
