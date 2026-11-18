//! Large-tree robustness gate.
//!
//! A thousand-sibling tree stresses lowering, layout, rendering metadata and
//! emitter traversal. It must keep every node, preserve declaration order and
//! compile within the deep-round latency ceiling without recursion overflow.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

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

#[test]
fn thousand_sibling_tree_compiles_with_all_nodes_intact() {
    let dir = out_dir("large-tree");
    let src = dir.join("big.opu");
    let ir = dir.join("big.opuir.json");
    let mut source = String::from("app Big {\n  state n = 0\n  Column {\n");
    for index in 0..1_000 {
        source.push_str(&format!("    Text(\"row {index}: {{n}}\")\n"));
    }
    source.push_str("  }\n}\n");
    fs::write(&src, source).expect("write source");

    let started = Instant::now();
    let result = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    let elapsed = started.elapsed();
    assert!(
        result.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        elapsed.as_millis() <= 2000,
        "large tree compile took {elapsed:?}"
    );

    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    let children = doc["tree"]["children"].as_array().expect("children");
    assert_eq!(children.len(), 1_000);
    assert_eq!(children[0]["props"]["value"], "row 0: 0");
    assert_eq!(children[999]["props"]["value"], "row 999: 0");
}
