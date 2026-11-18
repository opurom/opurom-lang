//! Interpolation stress boundary gate.
//!
//! Dart represents interpolation as alternating literal and expression
//! elements (`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:29439-29469`,
//! SDK 3.14.0, Flutter 3.50.0). Opurom's `interp_parts` uses the same linear
//! model: `{{` and `}}` are literal braces, `{name}` and `{items[i]}` are reads.
//! This gate covers large literals, many placeholders, repeated brace pairs,
//! and malformed brace runs with stable character-column diagnostics.

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

fn compile(src: &PathBuf, out: &PathBuf) -> std::process::Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
}

#[test]
fn large_literal_and_many_placeholders_lower_deterministically() {
    let dir = out_dir("interp-stress-ok");
    let src = dir.join("stress.opu");
    let first = dir.join("first.opuir.json");
    let second = dir.join("second.opuir.json");
    let literal = "x".repeat(20_000);
    let placeholders = "{n}".repeat(500);
    let text = format!("app Stress {{\n  state n = 7\n  Column {{\n    Text(\"{literal}\")\n    Text(\"{placeholders}\")\n    Text(\"{}\", size: 12)\n  }}\n}}\n", "{{lit}}");
    fs::write(&src, text).expect("write source");
    for out in [&first, &second] {
        let result = compile(&src, out);
        assert!(
            result.status.success(),
            "stress compile failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&first).expect("read first IR"),
        fs::read(&second).expect("read second IR"),
        "large template must lower deterministically"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&first).expect("read IR")).expect("parse IR");
    let values: Vec<&str> = doc["tree"]["children"]
        .as_array()
        .expect("children")
        .iter()
        .map(|n| n["props"]["value"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(values[0], literal);
    assert_eq!(values[1], "7".repeat(500));
    assert_eq!(values[2], "{{lit}}");
}

#[test]
fn repeated_doubled_braces_stay_literal() {
    let dir = out_dir("interp-stress-braces");
    let src = dir.join("braces.opu");
    let out = dir.join("braces.opuir.json");
    let literal = "{{x}}".repeat(5_000);
    fs::write(&src, format!("app Stress {{ Text(\"{literal}\") }}\n")).expect("write source");
    let result = compile(&src, &out);
    assert!(
        result.status.success(),
        "repeated braces failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["props"]["value"], literal);
}

#[test]
fn deep_unterminated_brace_run_has_stable_position_and_no_artifact() {
    let dir = out_dir("interp-stress-deep");
    let src = dir.join("deep.opu");
    let out = dir.join("deep.opuir.json");
    let prefix = "x".repeat(10_000);
    fs::write(&src, format!("app Stress {{ Text(\"{prefix} {{\") }}\n")).expect("write source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "deep brace run must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains(":1:10021: error: unterminated '{name}' interpolation")
            && stderr.contains("write '{{' for a literal '{'")
            && !out.exists(),
        "deep brace diagnostic: {stderr}"
    );
}

#[test]
fn unknown_state_after_many_placeholders_reports_exact_column() {
    let dir = out_dir("interp-stress-unknown");
    let src = dir.join("unknown.opu");
    let out = dir.join("unknown.opuir.json");
    let text = format!(
        "app Stress {{\n  state n = 1\n  Text(\"{}{{bad}}\")\n}}\n",
        "{n}".repeat(5_000)
    );
    fs::write(&src, text).expect("write source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "unknown state must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains(":3:15010: error: unknown state 'bad' in Text interpolation")
            && !out.exists(),
        "unknown state diagnostic: {stderr}"
    );
}
