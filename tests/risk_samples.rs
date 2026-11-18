//! Replayable boundary samples for deferred cross-module risks.
//!
//! These cases are intentionally kept outside the green gate: they document
//! where the current compiler stops and what a coordinated protocol round
//! would have to change. Every sample is a CLI invocation against a fixture
//! file, so a reviewer can reproduce the boundary without editing the tree.
//! No public IR field, emitter protocol or source grammar is changed here.

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

fn run(dir: &PathBuf, name: &str, source: &str) -> (bool, String, Vec<u8>) {
    let src = dir.join(format!("{name}.opu"));
    let out = dir.join(format!("{name}.opuir.json"));
    fs::write(&src, source).expect("write risk sample");
    let result = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc compile");
    let ir = fs::read(&out).unwrap_or_default();
    (
        result.status.success(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
        ir,
    )
}

fn compile_chain(dir: &PathBuf, name: &str, depth: usize) -> PathBuf {
    let mut source = String::from("app DepthRisk {\n");
    for _ in 0..depth {
        source.push_str("  Container {\n");
    }
    source.push_str("    Text(\"leaf\")\n");
    for _ in 0..depth {
        source.push_str("  }\n");
    }
    source.push_str("}\n");
    let (ok, stderr, _) = run(dir, name, &source);
    assert!(ok, "depth {depth} must compile: {stderr}");
    dir.join(format!("{name}.opuir.json"))
}

#[test]
fn serde_json_depth_boundary_is_replayable() {
    let dir = out_dir("risk-depth");
    let supported = compile_chain(&dir, "depth-60", 60);
    let emitted = bin()
        .arg("target")
        .arg(&supported)
        .arg("--target")
        .arg("web")
        .arg("-o")
        .arg(dir.join("web-60"))
        .output()
        .expect("run opuromc target");
    assert!(
        emitted.status.success(),
        "supported depth must emit: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );

    let rejected = compile_chain(&dir, "depth-200", 200);
    let out = bin()
        .arg("target")
        .arg(&rejected)
        .arg("--target")
        .arg("web")
        .arg("-o")
        .arg(dir.join("web-200"))
        .output()
        .expect("run opuromc target");
    assert!(!out.status.success(), "depth 200 must expose the limit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("recursion limit exceeded"),
        "unexpected depth diagnostic: {stderr}"
    );
}

#[test]
fn bigint_boundary_is_replayable() {
    let dir = out_dir("risk-bigint");
    let source = "app BigIntRisk {\n  state n = 9007199254740993\n  Text(\"{n}\")\n}\n";
    let (ok, _, ir) = run(&dir, "bigint", source);
    assert!(ok, "large literal must compile under the current contract");
    let text = String::from_utf8(ir).expect("IR is UTF-8");
    assert!(
        text.contains("9007199254740993"),
        "IR must preserve the literal exactly; JS-like emitters may still lose precision"
    );
    let emitted = bin()
        .arg("target")
        .arg(&dir.join("bigint.opuir.json"))
        .arg("--target")
        .arg("web")
        .arg("-o")
        .arg(dir.join("web"))
        .output()
        .expect("run opuromc target");
    assert!(emitted.status.success());
    let js = fs::read_to_string(dir.join("web/app.js")).expect("read web output");
    assert!(
        js.contains("9007199254740993"),
        "sample must reproduce the out-of-safe-range literal; host Number may round it at runtime"
    );
}

#[test]
fn parenthesized_expression_boundary_is_replayable() {
    let dir = out_dir("risk-paren");
    let source = "app ParenRisk {\n  state a = 1\n  computed b = (a + 1) * 2\n  Text(\"{b}\")\n}\n";
    let (ok, stderr, ir) = run(&dir, "paren", source);
    if ok {
        let text = String::from_utf8(ir).expect("IR is UTF-8");
        assert!(
            text.contains("(a + 1) * 2"),
            "unexpected grouped expr shape"
        );
        return;
    }
    assert!(
        stderr.contains(": error: "),
        "rejection must carry a positioned diagnostic: {stderr}"
    );
}
