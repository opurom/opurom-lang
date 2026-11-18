//! Calculator op lowering gate: compound arithmetic and backspace reach IR.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn compile(source: &str) -> serde_json::Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-out")
        .join(format!(
            "calc-ops-{}",
            CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    fs::create_dir_all(&dir).expect("create test output dir");
    let src = dir.join("source.opu");
    let ir = dir.join("source.opuir.json");
    fs::write(&src, source).expect("write source");
    let result = Command::new(env!("CARGO_BIN_EXE_opuromc"))
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run compiler");
    assert!(
        result.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR")
}

#[test]
fn calculator_ops_lower_to_ir_actions() {
    let doc = compile(
        "app CalcOps {\n  state n = 5\n  state m = 2\n  state label = \"12\"\n  Button(\"op\") { n *= m; n /= m; n.backspace(); label.backspace() }\n}\n",
    );
    let actions = doc["tree"]["events"]["tap"]["actions"]
        .as_array()
        .expect("actions array");
    let ops: Vec<&str> = actions.iter().map(|a| a["op"].as_str().unwrap()).collect();
    assert_eq!(ops, ["mul", "div", "backspace", "backspace"]);
    assert_eq!(actions[0]["valueFrom"], "m");
    assert_eq!(actions[1]["valueFrom"], "m");
    assert_eq!(actions[2]["target"], "n");
    assert_eq!(actions[3]["target"], "label");
}

#[test]
fn backspace_rejects_bool_state() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-out")
        .join(format!(
            "calc-ops-error-{}",
            CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    fs::create_dir_all(&dir).expect("create test output dir");
    let src = dir.join("source.opu");
    fs::write(
        &src,
        "app Bad { state flag = true\n Button(\"x\") { flag.backspace() } }",
    )
    .expect("write source");
    let result = Command::new(env!("CARGO_BIN_EXE_opuromc"))
        .arg("check")
        .arg(&src)
        .output()
        .expect("run compiler");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("backspace()"));
}
