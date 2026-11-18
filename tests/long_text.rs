//! Long-text robustness gate.
//!
//! A 20k-code-point literal is far beyond ordinary UI copy. It must survive
//! lexing, IR serialization and target emission without truncation or a
//! superlinear compile stall. The test pins exact byte preservation and a
//! generous latency ceiling on the shipped binary.

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
fn twenty_thousand_character_literal_survives_compile_and_emit() {
    let dir = out_dir("long-text");
    let src = dir.join("long.opu");
    let ir = dir.join("long.opuir.json");
    let text = "x".repeat(20_000);
    fs::write(&src, format!("app LongText {{ Text(\"{text}\") }}\n")).expect("write source");

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
        elapsed.as_millis() <= 1000,
        "long text compile took {elapsed:?}"
    );

    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["props"]["value"], text);

    let out = dir.join("web");
    let emitted = bin()
        .arg("target")
        .arg(&ir)
        .arg("--target")
        .arg("web")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc target");
    assert!(
        emitted.status.success(),
        "emit failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let app = fs::read_to_string(out.join("app.js")).expect("read app.js");
    assert!(app.contains(&text), "emitter truncated long text");
}
