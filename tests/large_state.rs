//! Large-state robustness gate.
//!
//! A project with hundreds of states and a long computed chain stresses the
//! symbol table, liveness analysis and emitter replanning. It must compile
//! without stack overflow or quadratic stalls, and the emitted tree must keep
//! a single reference to the computed result.

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
fn two_hundred_states_and_long_computed_chain_compile() {
    let dir = out_dir("large-state");
    let src = dir.join("many.opu");
    let ir = dir.join("many.opuir.json");
    let mut source = String::from("app Many {\n");
    for index in 0..200 {
        source.push_str(&format!("  state s{index} = {index}\n"));
    }
    let chain = (0..200)
        .map(|index| format!("s{index}"))
        .collect::<Vec<_>>()
        .join(" + ");
    source.push_str(&format!(
        "  computed total = {chain}\n  Text(\"{{total}}\")\n}}\n"
    ));
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
        "large state compile took {elapsed:?}"
    );

    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["state"].as_array().expect("states").len(), 201);
    assert_eq!(doc["computed"][0]["name"], "total");
    assert_eq!(doc["computed"][0]["type"], "int");
    assert_eq!(doc["tree"]["props"]["value"], "19900");
}
