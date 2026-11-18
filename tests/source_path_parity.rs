//! Source path stability gate.
//!
//! The IR embeds app identity and source-level spans, but must not embed the
//! absolute path used to invoke the compiler. A relative path and an absolute
//! path to the same file must therefore produce byte-identical IR. This keeps
//! CI checkouts reproducible across machines and workspace layouts.

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

#[test]
fn relative_and_absolute_source_paths_produce_identical_ir() {
    let dir = out_dir("source-path-parity");
    let src = dir.join("app.opu");
    fs::write(&src, "app PathParity { state n = 0 Text(\"{n}\") }\n").expect("write source");

    let relative = dir.join("relative.opuir.json");
    let absolute = dir.join("absolute.opuir.json");
    let canonical = src.canonicalize().expect("canonical source path");
    for (arg, out) in [(src.as_path(), &relative), (canonical.as_path(), &absolute)] {
        let result = bin()
            .arg("compile")
            .arg(arg)
            .arg("-o")
            .arg(out)
            .current_dir(&dir)
            .output()
            .expect("run opuromc compile");
        assert!(
            result.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&relative).expect("read relative IR"),
        fs::read(&absolute).expect("read absolute IR"),
        "input path style must not affect IR bytes"
    );
}
