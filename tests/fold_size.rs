//! Compile-time folding and dead-code elimination size gates.
//!
//! The framework acceptance bar requires a constant-folding plus DCE pass to
//! shrink the artifact by at least 15 percent for a workload whose values and
//! branches are all known at compile time. This test drives the shipped CLI
//! and compares real emitted files, never an aspirational model.

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

const SOURCE: &str = r#"app Dead {
  state visible = true
  state hidden = false
  state unused = "dead string payload"
  computed wasted = 2 + 3 * 4
  Column {
    Text("always: {visible}")
    if (hidden) { Text("hidden: {hidden}") }
    Text("constant: {wasted}")
  }
}
"#;

#[test]
fn fold_and_dce_shrink_ir_by_at_least_15_percent() {
    let dir = out_dir("fold-size");
    let src = dir.join("dead.opu");
    let ir = dir.join("dead.opuir.json");
    let optimized = dir.join("dead.opt.json");
    fs::write(&src, SOURCE).expect("write source");

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

    let result = bin()
        .arg("optimize")
        .arg(&ir)
        .arg("-o")
        .arg(&optimized)
        .output()
        .expect("run opuromc optimize");
    assert!(
        result.status.success(),
        "optimize failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let before = fs::metadata(&ir).expect("stat IR").len() as f64;
    let after = fs::metadata(&optimized).expect("stat optimized IR").len() as f64;
    let saved = (before - after) / before;
    assert!(
        saved >= 0.15,
        "fold and DCE saved {:.1}% of IR bytes (before={before}, after={after})",
        saved * 100.0
    );

    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&optimized).expect("read optimized IR"))
            .expect("parse optimized IR");
    assert!(json
        .get("state")
        .and_then(|s| s.as_array())
        .is_none_or(|a| a.is_empty()));
    assert!(json.get("computed").is_none());
    let children = json["tree"]["children"]
        .as_array()
        .expect("tree children array");
    assert_eq!(children.len(), 2, "constant-hidden branch must be deleted");
    let values: Vec<&str> = children
        .iter()
        .map(|node| node["props"]["value"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(values, vec!["always: true", "constant: 14"]);
}
