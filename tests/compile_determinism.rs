//! Deterministic compilation gate.
//!
//! Identical source text must lower to byte-identical IR on every run. The
//! compiler is a pure function of source plus path; no clock, random value or
//! hash-map iteration order may leak into the artifact. This is the foundation
//! for golden digests, incremental reuse and cross-machine CI comparison.

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

const SOURCE: &str = r##"app Deterministic {
  state count = 0
  state label = "ready"
  state names = ["a", "b", "c"]
  theme { mode: dark, accent: "#097FEB", radius: 12 }
  computed doubled = count * 2
  Column(gap: 8) {
    Text("{label}: {doubled}")
    for (item in names) { Text("{item}") }
    Button("+") { count += 1 }
  }
}
"##;

#[test]
fn repeated_compile_produces_byte_identical_ir() {
    let dir = out_dir("compile-determinism");
    let src = dir.join("source.opu");
    fs::write(&src, SOURCE).expect("write source");

    let mut artifacts = Vec::new();
    for run in 0..3 {
        let out = dir.join(format!("run-{run}.opuir.json"));
        let result = bin()
            .arg("compile")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run opuromc compile");
        assert!(
            result.status.success(),
            "run {run} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        artifacts.push(fs::read(&out).expect("read IR"));
    }
    assert_eq!(artifacts[0], artifacts[1], "first and second runs differ");
    assert_eq!(artifacts[0], artifacts[2], "first and third runs differ");
}
