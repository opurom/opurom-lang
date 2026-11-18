//! Optimizer idempotence gate.
//!
//! Release and incremental pipelines may run the optimizer more than once on
//! the same IR. A second pass must be a byte no-op once all constant folds and
//! dead-code eliminations have been applied. This pins that invariant using
//! the shipped CLI; it also guards the additive map exports (stateRefs and
//! nodeBoundaries) from changing shape on a repeat pass.

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

const SOURCE: &str = r#"app Idempotent {
  state visible = true
  state hidden = false
  state count = 0
  computed total = 2 + 3 * 4
  Column {
    Text("constant: {total} / {visible}")
    if (hidden) { Text("hidden") }
    Button("+") { count += 1 }
  }
}
"#;

#[test]
fn full_fold_is_idempotent() {
    let dir = out_dir("optimizer-idempotence");
    let src = dir.join("source.opu");
    let raw = dir.join("raw.opuir.json");
    let first = dir.join("first.json");
    let second = dir.join("second.json");
    fs::write(&src, SOURCE).expect("write source");

    for (input, output) in [(&src, &raw), (&raw, &first), (&first, &second)] {
        let mut command = bin();
        if output == &raw {
            command.arg("compile").arg(input).arg("-o").arg(output);
        } else {
            command
                .arg("optimize")
                .arg(input)
                .arg("-o")
                .arg(output)
                .arg("--full-fold");
        }
        let result = command.output().expect("run opuromc");
        assert!(
            result.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    assert_eq!(
        fs::read(&first).expect("read first pass"),
        fs::read(&second).expect("read second pass"),
        "full fold must be idempotent"
    );
}
