//! Deterministic code generation gate.
//!
//! The same IR emitted twice by the same target must produce byte-identical
//! output trees. Build caching and artifact verification rely on this: a
//! timestamp, absolute output path or hash-map iteration order in generated
//! source would break reproducible release builds.

use std::collections::BTreeMap;
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

fn snapshot(dir: &PathBuf) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read output dir") {
        let path = entry.expect("read dir entry").path();
        if path.is_file() {
            files.insert(
                path.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned(),
                fs::read(&path).expect("read emitted file"),
            );
        }
    }
    files
}

#[test]
fn every_target_emits_byte_identical_output_twice() {
    let dir = out_dir("emit-determinism");
    let src = dir.join("source.opu");
    let ir = dir.join("source.opuir.json");
    fs::write(
        &src,
        "app EmitDeterminism {\n  state count = 0\n  Text(\"count: {count}\")\n  Button(\"+\") { count += 1 }\n}\n",
    )
    .expect("write source");
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

    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let first = dir.join(format!("{target}-first"));
        let second = dir.join(format!("{target}-second"));
        for out in [&first, &second] {
            let result = bin()
                .arg("target")
                .arg(&ir)
                .arg("--target")
                .arg(target)
                .arg("-o")
                .arg(out)
                .output()
                .expect("run opuromc target");
            assert!(
                result.status.success(),
                "{target} emit failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert_eq!(
            snapshot(&first),
            snapshot(&second),
            "{target} emitted non-deterministic output"
        );
    }
}
