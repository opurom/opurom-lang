//! Single-file compile latency gate.
//!
//! Acceptance bar: one source file compiles in at most 200 ms. The test drives
//! the shipped compiler binary on a mid-size source and measures process
//! wall-clock time from the test harness. It is deliberately a ceiling test,
//! not a benchmark report: a regression that crosses the bar fails CI.

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

fn source(node_count: usize) -> String {
    let mut out = String::from("app Latency {\n  state count = 0\n  Column {\n");
    for index in 0..node_count {
        out.push_str(&format!("    Text(\"row {index}: {{count}}\")\n"));
    }
    out.push_str("    Button(\"+\") { count += 1 }\n  }\n}\n");
    out
}

#[test]
fn single_file_compile_stays_under_200ms() {
    let dir = out_dir("compile-latency");
    let src = dir.join("latency.opu");
    let ir = dir.join("latency.opuir.json");
    fs::write(&src, source(48)).expect("write source");

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
        elapsed.as_millis() <= 200,
        "single-file compile took {} ms; limit is 200 ms",
        elapsed.as_millis()
    );
    assert!(fs::metadata(&ir).expect("stat IR").len() > 0);
}
