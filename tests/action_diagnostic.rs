//! Contextual IR action diagnostic gate.
//!
//! A hand-authored or stale IR can name an undeclared state in an event
//! action. The emitter must name the failing operation and node id so a build
//! failure is actionable without searching the whole tree. Flutter's frontend
//! reports compile errors with source context before accepting a bundle
//! (`packages/flutter_tools/lib/src/compile.dart:96-114`,
//! `packages/flutter_tools/lib/src/devfs.dart:636-642`, Flutter 3.50.0).

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

fn compile_single_action_ir(dir: &PathBuf, action: serde_json::Value) -> PathBuf {
    let src = dir.join("source.opu");
    let ir = dir.join("source.opuir.json");
    fs::write(
        &src,
        "app ActionDiag {\n  state a = 0\n  Button(\"x\") { a += 1 }\n}\n",
    )
    .expect("write source");
    let compiled = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let mut doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    doc["tree"]["events"]["tap"] = action;
    fs::write(&ir, serde_json::to_vec(&doc).expect("serialize IR")).expect("write IR");
    ir
}

#[test]
fn unknown_action_op_names_the_target() {
    let dir = out_dir("unknown-action-op");
    let ir = compile_single_action_ir(
        &dir,
        serde_json::json!({"op": "bogus", "target": "a", "value": 1}),
    );
    for target in ["web", "native", "wechat", "harmony"] {
        let result = bin()
            .arg("target")
            .arg(&ir)
            .arg("--target")
            .arg(target)
            .arg("-o")
            .arg(dir.join(format!("{target}-unknown-out")))
            .output()
            .expect("run opuromc target");
        assert_eq!(
            result.status.code(),
            Some(1),
            "{target} accepted unknown op"
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("unknown event op 'bogus' on target 'a'")
                && stderr.contains("  help: "),
            "{target} diagnostic: {stderr}"
        );
    }
}

#[test]
fn undeclared_action_target_names_the_operation_and_node() {
    let dir = out_dir("action-diagnostic");
    let src = dir.join("source.opu");
    let ir = dir.join("source.opuir.json");
    fs::write(
        &src,
        "app ActionDiag {\n  state a = 0\n  state b = 0\n  Button(\"x\") { a += 1; b += 2; a += 3 }\n}\n",
    )
    .expect("write source");
    let compiled = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let mut doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    doc["tree"]["events"]["tap"] = serde_json::json!({
        "actions": [
            {"op": "add", "target": "a", "value": 1},
            {"op": "set", "target": "bogus", "value": 1},
            {"op": "add", "target": "a", "value": 3}
        ]
    });
    fs::write(&ir, serde_json::to_vec(&doc).expect("serialize IR")).expect("write IR");

    for target in ["web", "native", "wechat", "harmony"] {
        let result = bin()
            .arg("target")
            .arg(&ir)
            .arg("--target")
            .arg(target)
            .arg("-o")
            .arg(dir.join(format!("{target}-out")))
            .output()
            .expect("run opuromc target");
        assert_eq!(
            result.status.code(),
            Some(1),
            "{target} accepted bad action"
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("state 'bogus' is not declared in set action on node ")
                && stderr.contains("  help: "),
            "{target} diagnostic: {stderr}"
        );
    }
}
