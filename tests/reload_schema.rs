//! Stable state schema for state-preserving reload.
//!
//! The dev runner classifies a reload as state-preserving only when the IR
//! state schema is unchanged. The compiler emits `state` entries in source
//! declaration order (`lower.rs`, `compile_source`), so identical declarations
//! must produce identical bytes across builds and any name/type/initial-value
//! change must be visible in that sequence. These gates pin that contract.

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

fn compile(dir: &PathBuf, name: &str, source: &str) -> serde_json::Value {
    let src = dir.join(format!("{name}.opu"));
    let ir = dir.join(format!("{name}.opuir.json"));
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
    serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR")
}

fn schema(ir: &serde_json::Value) -> serde_json::Value {
    serde_json::Value::Array(
        ir["state"]
            .as_array()
            .expect("state array")
            .iter()
            .map(|entry| {
                serde_json::json!([
                    entry["name"].clone(),
                    entry["type"].clone(),
                    entry["init"].clone()
                ])
            })
            .collect(),
    )
}

#[test]
fn ui_text_change_preserves_state_schema() {
    let dir = out_dir("reload-schema");
    let first = compile(
        &dir,
        "first",
        "app Reload {\n  state count = 0\n  state label = \"ready\"\n  Text(\"{label}: {count}\")\n}\n",
    );
    let second = compile(
        &dir,
        "second",
        "app Reload {\n  state count = 0\n  state label = \"ready\"\n  Text(\"changed: {count}\")\n}\n",
    );
    assert_eq!(schema(&first), schema(&second));
}

#[test]
fn state_changes_alter_state_schema() {
    let dir = out_dir("reload-schema-changes");
    let base = compile(
        &dir,
        "base",
        "app Reload {\n  state count = 0\n  Text(\"{count}\")\n}\n",
    );
    let init = compile(
        &dir,
        "init",
        "app Reload {\n  state count = 1\n  Text(\"{count}\")\n}\n",
    );
    let ty = compile(
        &dir,
        "type",
        "app Reload {\n  state count: string = \"0\"\n  Text(\"{count}\")\n}\n",
    );
    let name = compile(
        &dir,
        "name",
        "app Reload {\n  state value = 0\n  Text(\"{value}\")\n}\n",
    );
    assert_ne!(schema(&base), schema(&init));
    assert_ne!(schema(&base), schema(&ty));
    assert_ne!(schema(&base), schema(&name));
}
