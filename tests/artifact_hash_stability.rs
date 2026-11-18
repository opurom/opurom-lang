//! Cross-target artifact manifest hash gate.
//!
//! Flutter's depfile reader normalizes output paths and removes duplicates so
//! the build graph stays stable (`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`,
//! Flutter 3.50.0). Opurom's `opuromc targets` is the equivalent manifest for
//! the ten built-in emitters. This gate hashes every declared artifact and
//! checks the complete file-name set, non-empty files, repeat-emission bytes
//! and relative/absolute IR path calls.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const TARGETS: &[&str] = &[
    "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
];

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

fn manifest() -> BTreeMap<String, Vec<String>> {
    let output = bin().arg("targets").output().expect("run opuromc targets");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(target, files)| {
            (
                target.to_string(),
                files.split(", ").map(str::to_string).collect(),
            )
        })
        .collect()
}

fn emit(ir: &Path, target: &str, out: &Path, cwd: Option<&Path>) {
    let mut command = bin();
    command
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(out);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let result = command.output().expect("run opuromc target");
    assert!(
        result.status.success(),
        "{target} emit failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("read output dir") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.is_file() {
                let name = path
                    .strip_prefix(base)
                    .expect("path under output dir")
                    .to_string_lossy()
                    .into_owned();
                out.insert(name, fs::read(&path).expect("read artifact"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn hash(text: &[u8]) -> String {
    // FNV-1a over bytes gives a compact, dependency-free change detector.
    let mut value = 0xcbf29ce484222325u64;
    for byte in text {
        value ^= *byte as u64;
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}

#[test]
fn declared_artifact_sets_are_exact_and_hashed() {
    let dir = out_dir("artifact-hash");
    let manifest = manifest();
    assert_eq!(manifest.len(), 11, "unexpected target count: {manifest:?}");
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");
    fs::write(&src, "app Artifacts { state n = 7 Text(\"{n}\") }\n").expect("write source");
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

    let mut report = BTreeMap::new();
    for target in TARGETS {
        let expected = manifest
            .get(*target)
            .unwrap_or_else(|| panic!("missing manifest row for {target}"));
        let first = dir.join(format!("{target}-first"));
        let second = dir.join(format!("{target}-second"));
        emit(&ir, target, &first, None);
        emit(&ir, target, &second, None);

        let first_files = snapshot(&first);
        let second_files = snapshot(&second);
        let mut expected_set: Vec<String> = expected.clone();
        expected_set.sort();
        let actual_set: Vec<String> = first_files.keys().cloned().collect();
        assert_eq!(actual_set, expected_set, "{target} artifact set drift");
        assert_eq!(first_files, second_files, "{target} emitted unstable bytes");
        for name in expected {
            let bytes = &first_files[name];
            assert!(!bytes.is_empty(), "{target}: empty artifact {name}");
        }
        let digest: Vec<(&String, String)> = expected
            .iter()
            .map(|name| (name, hash(&first_files[name])))
            .collect();
        report.insert(*target, digest);
    }

    // A second run must reproduce every hash. This is the release-manifest
    // contract: path, file set and bytes are all reproducible from source.
    for (target, first_digest) in &report {
        let out = dir.join(format!("{target}-hash-repeat"));
        emit(&ir, target, &out, None);
        let files = snapshot(&out);
        for (name, value) in first_digest {
            assert_eq!(
                hash(&files[name.as_str()]),
                *value,
                "{target}: unstable hash for {name}"
            );
        }
    }
}

#[test]
fn relative_ir_path_does_not_change_artifact_bytes() {
    let dir = out_dir("artifact-relative");
    let src = dir.join("app.opu");
    let ir = dir.join("app.opuir.json");
    fs::write(&src, "app Artifacts { state n = 7 Text(\"{n}\") }\n").expect("write source");
    let compiled = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(compiled.status.success());

    for target in TARGETS {
        let absolute = dir.join(format!("{target}-abs"));
        let relative = dir.join(format!("{target}-rel"));
        emit(&ir, target, &absolute, None);
        emit(Path::new("app.opuir.json"), target, &relative, Some(&dir));
        assert_eq!(
            snapshot(&absolute),
            snapshot(&relative),
            "{target} artifact bytes depend on IR path style"
        );
    }
}
