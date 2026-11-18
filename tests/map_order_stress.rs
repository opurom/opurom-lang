//! Large declaration-order determinism gate.
//!
//! The compiler uses hash maps while validating components, state types and
//! computed dependencies. A large source with hundreds of independently named
//! declarations must still lower to the same IR bytes across repeated runs.
//! Dart's ordered maps guarantee insertion or sorted iteration and explicitly
//! reject unordered iteration as a stable contract (`collection/linked_hash_map.dart:109-115`,
//! `collection/splay_tree.dart:407-411`, SDK 3.14.0, Flutter 3.50.0).

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

const DECLARATIONS: usize = 250;

fn source() -> String {
    let mut out = String::from("app MapOrder {\n");
    for i in 0..DECLARATIONS {
        out.push_str(&format!("  state s{i} = {i}\n"));
    }
    out.push_str("  Column {\n");
    for i in 0..DECLARATIONS {
        out.push_str(&format!("    Text(\"{{s{i}}}\")\n"));
    }
    out.push_str("  }\n}\n");
    out
}

#[test]
fn hundreds_of_declarations_lower_deterministically() {
    let dir = out_dir("map-order-stress");
    let src = dir.join("source.opu");
    fs::write(&src, source()).expect("write source");
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
        artifacts.push(fs::read(out).expect("read IR"));
    }
    assert_eq!(artifacts[0], artifacts[1], "second run changed IR bytes");
    assert_eq!(artifacts[0], artifacts[2], "third run changed IR bytes");
    let ir = String::from_utf8(artifacts[0].clone()).expect("IR is UTF-8");
    for i in [0, DECLARATIONS / 2, DECLARATIONS - 1] {
        assert!(
            ir.contains(&format!("\"name\": \"s{i}\"")),
            "lost state s{i}"
        );
    }
}
