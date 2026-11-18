//! Deep-tree robustness gate.
//!
//! A 60-level container chain exercises recursive lowering, parent-id
//! numbering and emitter traversal. It must compile without stack overflow,
//! preserve the full depth in IR and emit on each target without losing nodes.

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
fn deep_container_chain_compiles_and_emits() {
    let dir = out_dir("deep-tree");
    let src = dir.join("deep.opu");
    let ir = dir.join("deep.opuir.json");
    let depth = 60;
    let mut source = String::from("app Deep {\n");
    for _ in 0..depth {
        source.push_str("  Container {\n");
    }
    source.push_str("    Text(\"leaf\")\n");
    for _ in 0..depth {
        source.push_str("  }\n");
    }
    source.push_str("}\n");
    fs::write(&src, source).expect("write source");

    let started = Instant::now();
    let compiled = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    let elapsed = started.elapsed();
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert!(
        elapsed.as_millis() <= 2000,
        "deep tree compile took {elapsed:?}"
    );

    let ir_text = fs::read_to_string(&ir).expect("read IR");
    let node_count = ir_text.matches("\"kind\": \"Container\"").count();
    assert_eq!(node_count, depth, "IR must keep every container node");
    assert!(ir_text.contains("\"leaf\""), "IR lost the leaf text");

    for target in ["web", "native"] {
        let out = dir.join(target);
        let result = bin()
            .arg("target")
            .arg(&ir)
            .arg("--target")
            .arg(target)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run opuromc target");
        assert!(
            result.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
