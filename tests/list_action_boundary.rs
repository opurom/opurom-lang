//! Unsupported runtime list mutation gate.
//!
//! List states are expanded at compile time; they are not mutable runtime
//! values. The compiler currently accepts `append` and `removeAt` statements,
//! but every emitter must reject them with a clear IR diagnostic instead of
//! generating a handler that silently updates a compile-time-only value.
//! Dart models unmodifiable lists explicitly: length and element changes are
//! rejected (`core/list.dart:239-245`, SDK 3.14.0, Flutter 3.50.0). Opurom list
//! states are compile-time expansion inputs, so every emitter must reject
//! runtime mutation with a clear IR diagnostic instead of generating a handler
//! that silently updates a compile-time-only value.

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

fn compile_list_source(dir: &PathBuf) -> PathBuf {
    let src = dir.join("list.opu");
    let ir = dir.join("list.opuir.json");
    fs::write(
        &src,
        "app ListAction {\n  state items = [\"a\"]\n  Button(\"add\") { items.append(\"b\") }\n}\n",
    )
    .expect("write source");
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
    ir
}

#[test]
fn runtime_list_mutation_is_rejected_on_every_target() {
    let dir = out_dir("list-action");
    let ir = compile_list_source(&dir);
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["events"]["tap"]["op"], "append");
    assert_eq!(doc["tree"]["events"]["tap"]["target"], "items");

    for target in [
        "web", "native", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
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
            "{target} accepted list mutation"
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("list state 'items' is compile-time only; do not mutate it at runtime"),
            "{target} diagnostic: {stderr}"
        );
    }
}
