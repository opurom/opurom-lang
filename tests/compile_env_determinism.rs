//! Deterministic compilation under environment and map-order perturbation.
//!
//! Stable artifacts are a build-system contract: the same source text must
//! lower to byte-identical IR on every run, regardless of unrelated process
//! environment values or the invocation path style. This gate widens the
//! existing compile-determinism coverage with map-order-sensitive declarations,
//! Unicode text, relative/absolute paths and hostile environment values.
//!
//! Basis: Dart's `JsonEncoder` preserves authored map member order in its
//! output (`convert/json.dart:260-275`, SDK 3.14.0, Flutter 3.50.0). Opurom
//! keeps the same source order in IR.

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

const SOURCE: &str = r##"app DeterministicEnv {
  state zeta = "z"
  state alpha = "a"
  state mid = "你好"
  state count = 0
  theme { mode: dark, accent: "#097FEB", radius: 12 }
  computed doubled = count * 2
  Column {
    Text("{zeta} {alpha} {mid}: {doubled}")
    Button("+") { count += 1 }
  }
}
"##;

fn compile(dir: &PathBuf, src: &std::path::Path, out: &std::path::Path, env_seed: &str) -> Vec<u8> {
    let result = bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .current_dir(dir)
        .env("OPUROM_HASH_SEED", env_seed)
        .env("OPR_BASE_URL", format!("http://{env_seed}.invalid"))
        .env("Opurom_FRAME_WIDTH", env_seed)
        .env("Opurom_FRAME_HEIGHT", env_seed)
        .output()
        .expect("run opuromc compile");
    assert!(
        result.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::read(out).expect("read IR")
}

#[test]
fn map_order_unicode_path_and_environment_do_not_change_ir() {
    let dir = out_dir("compile-env-determinism");
    let src = dir.join("source.opu");
    fs::write(&src, SOURCE).expect("write source");
    let canonical = src.canonicalize().expect("canonical source path");

    let mut artifacts = Vec::new();
    for run in 0..3 {
        let out = dir.join(format!("run-{run}.opuir.json"));
        let arg = if run == 2 {
            canonical.as_path()
        } else {
            src.as_path()
        };
        artifacts.push(compile(&dir, arg, &out, &format!("seed-{run}")));
    }
    assert_eq!(artifacts[0], artifacts[1], "environment changed IR bytes");
    assert_eq!(artifacts[0], artifacts[2], "path style changed IR bytes");
    let ir = String::from_utf8(artifacts[0].clone()).expect("IR is UTF-8");
    assert!(ir.contains("你好"), "Unicode text lost in IR");
}
