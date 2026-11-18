//! Target artifact manifest gate.
//!
//! `opuromc targets` advertises the exact file set each emitter writes. Build
//! scripts and release packaging rely on that list, so an emitter that silently
//! skips or renames a file must fail CI. This test compiles one source once and
//! checks every declared artifact exists and is non-empty for all ten targets.

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
fn every_target_writes_its_declared_artifacts() {
    let dir = out_dir("artifact-manifest");
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");
    fs::write(&src, "app Artifacts { Text(\"ok\") }\n").expect("write source");
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

    let manifest = bin().arg("targets").output().expect("run opuromc targets");
    assert!(manifest.status.success());
    let listing = String::from_utf8_lossy(&manifest.stdout);
    let targets: Vec<(&str, Vec<&str>)> = listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, files)| (name, files.split(", ").collect()))
        .collect();
    assert_eq!(targets.len(), 11, "unexpected target count: {listing}");

    for (target, files) in targets {
        if target == "web-dom" {
            // The DOM emitter lives in the sibling repo and is invoked by the
            // CLI, not by this compiler binary.
            continue;
        }
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
        for file in files {
            let path = out.join(file);
            assert!(path.is_file(), "{target}: missing artifact {file}");
            assert!(
                fs::metadata(&path).expect("stat artifact").len() > 0,
                "{target}: empty artifact {file}"
            );
        }
    }
}
