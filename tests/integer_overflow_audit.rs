//! Integer overflow audit for action chains and compile-time arithmetic.
//!
//! Dart's default `int` is a 64-bit two's-complement value with wrap-around
//! overflow, while JavaScript targets use an exact-integer subset
//! (`core/int.dart:9-16`, SDK 3.14.0, Flutter 3.50.0). Opurom's closed compiler
//! protocol rejects overflow in compile-time computed expressions, rejects
//! integer literals outside `i64`, bounds list indexes and delays, and keeps
//! runtime action mutation on the native `i64` path. This gate audits those
//! boundaries without changing the JS-like representation.

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

fn check(src: &PathBuf) -> String {
    let result = bin()
        .arg("check")
        .arg(src)
        .output()
        .expect("run opuromc check");
    assert_eq!(result.status.code(), Some(1), "source must be rejected");
    String::from_utf8_lossy(&result.stderr).into_owned()
}

fn compile(src: &PathBuf, out: &PathBuf) -> bool {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
        .status
        .success()
}

#[test]
fn compile_time_arithmetic_stays_inside_i64() {
    let dir = out_dir("overflow-computed");
    let src = dir.join("overflow.opu");
    let out = dir.join("overflow.opuir.json");
    fs::write(
        &src,
        "app Overflow {\n  computed bad = 9223372036854775807 + 1\n  Text(\"{bad}\")\n}\n",
    )
    .expect("write source");
    let stderr = check(&src);
    assert!(
        stderr.contains("computed expression '+' overflows int: 9223372036854775807 + 1")
            && stderr.contains(
                "int results must fit between -9223372036854775808 and 9223372036854775807"
            ),
        "overflow diagnostic: {stderr}"
    );
    assert!(!compile(&src, &out), "failed compile must not write IR");
    assert!(!out.exists(), "no IR may exist after overflow");
}

#[test]
fn integer_literal_range_is_rejected() {
    let dir = out_dir("overflow-literal");
    let src = dir.join("literal.opu");
    fs::write(
        &src,
        "app Overflow { state n = 9223372036854775808 Text(\"{n}\") }\n",
    )
    .expect("write source");
    let stderr = check(&src);
    assert!(
        stderr.contains("integer literal '9223372036854775808' is out of range")
            && stderr.contains("use a value between -9223372036854775808 and 9223372036854775807"),
        "literal diagnostic: {stderr}"
    );
}

#[test]
fn extreme_list_index_is_rejected_without_usize_conversion_panic() {
    let dir = out_dir("overflow-index");
    let src = dir.join("index.opu");
    fs::write(
        &src,
        "app Overflow {\n  state items = [\"a\"]\n  Text(\"{items[9223372036854775807]}\")\n}\n",
    )
    .expect("write source");
    let stderr = check(&src);
    assert!(
        stderr.contains("list index 9223372036854775807 is out of range for 'items' with 1 values")
            && stderr.contains("use an index from 0 to 0"),
        "index diagnostic: {stderr}"
    );
}

#[test]
fn extreme_delay_is_rejected() {
    let dir = out_dir("overflow-delay");
    let src = dir.join("delay.opu");
    fs::write(
        &src,
        "app Overflow {\n  Button(\"x\") { await delay(9223372036854775807) }\n}\n",
    )
    .expect("write source");
    let stderr = check(&src);
    assert!(
        stderr.contains("delay must be between 0 and 60000 ms")
            && stderr.contains("write a delay from 0 to 60000 milliseconds"),
        "delay diagnostic: {stderr}"
    );
}

#[test]
fn runtime_action_keeps_i64_extreme_on_every_target() {
    let dir = out_dir("overflow-action");
    let src = dir.join("action.opu");
    let ir = dir.join("action.opuir.json");
    fs::write(
        &src,
        "app Overflow {\n  state n = 9223372036854775807\n  Button(\"x\") { n += 1 }\n}\n",
    )
    .expect("write source");
    assert!(compile(&src, &ir), "max i64 state must compile");

    for target in [
        "web", "native", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let result = bin()
            .arg("target")
            .arg(&ir)
            .arg("--target")
            .arg(target)
            .arg("-o")
            .arg(dir.join(format!("{target}-out")))
            .output()
            .expect("run opuromc target");
        assert!(
            result.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let web = fs::read_to_string(dir.join("web-out/app.js")).expect("read web output");
    assert!(
        web.contains("\"n\":9223372036854775807"),
        "web lost i64 init"
    );
    let native =
        fs::read_to_string(dir.join("native-out/src/main.rs")).expect("read native output");
    assert!(
        native.contains("n: 9223372036854775807i64")
            && native.contains("_state.n = _state.n.saturating_add(1i64);"),
        "native lost i64 action path"
    );
}
