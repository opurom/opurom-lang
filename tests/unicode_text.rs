//! Unicode text lowering gate.
//!
//! Source text may contain Latin accents, CJK and RTL scripts. The compiler
//! must carry those code points through IR and every text-bearing emitter
//! without transliteration, lossy escaping or byte-order drift.

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

const TEXT: &str = "你好, café, مرحبا";

#[test]
fn unicode_text_survives_ir_and_text_bearing_emitters() {
    let dir = out_dir("unicode-text");
    let src = dir.join("unicode.opu");
    let ir = dir.join("unicode.opuir.json");
    fs::write(&src, format!("app Unicode {{ Text(\"{TEXT}\") }}\n")).expect("write source");
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

    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["props"]["value"], TEXT);

    for (target, file) in [
        ("web", "app.js"),
        ("native", "src/main.rs"),
        ("wechat", "pages/index/index.wxml"),
    ] {
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
        let emitted = fs::read_to_string(out.join(file)).expect("read emitted source");
        assert!(emitted.contains(TEXT), "{target} lost Unicode text");
    }
}
