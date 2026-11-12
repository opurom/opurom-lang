//! The `wasm` emitter conformance: the generated crate builds for
//! `wasm32-unknown-unknown` and the module satisfies the ABI of WASM_ABI.md.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn out_dir(label: &str) -> PathBuf {
    let p = workspace().join("target/test-out").join(format!(
        "{}-{}",
        label,
        CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).expect("create test output dir");
    p
}

fn cargo_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME")).join(".cargo/bin/cargo")
}

fn cargo_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!(
        "{home}/.cargo/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    )
}

const SOURCE: &str = r#"app Counter {
  state count = 0
  Column(gap: 8, padding: 16) {
    Text("Count: {count}", size: 24)
    Button("+") { count += 1 }
  }
}
"#;

#[test]
fn wasm_emitter_builds_and_matches_the_abi() {
    let dir = out_dir("wasm");
    let src = dir.join("counter.opu");
    fs::write(&src, SOURCE).expect("write source");
    let ir = dir.join("counter.opuir.json");
    let r = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let out = dir.join("wasm");
    let r = bin()
        .arg("target")
        .arg(&ir)
        .arg("--target")
        .arg("wasm")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc target");
    assert!(
        r.status.success(),
        "wasm emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );

    for file in [
        "Cargo.toml",
        "src/lib.rs",
        "index.html",
        "host.js",
        "README.md",
    ] {
        assert!(out.join(file).is_file(), "missing {file}");
    }
    let manifest = fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("crate-type = [\"cdylib\"]"), "{manifest}");
    assert!(manifest.contains("name = \"opurom_wasm\""), "{manifest}");
    assert!(!manifest.contains("[dependencies."), "{manifest}");

    let lib = fs::read_to_string(out.join("src/lib.rs")).unwrap();
    for export in [
        "pub extern \"C\" fn opu_version() -> u32",
        "pub extern \"C\" fn opu_state_len() -> u32",
        "pub extern \"C\" fn opu_state(index: u32) -> i32",
        "pub extern \"C\" fn opu_tap(node_id: u32) -> u32",
        "pub extern \"C\" fn opu_input(node_id: u32, v: i32) -> u32",
    ] {
        assert!(lib.contains(export), "missing export: {export}");
    }

    // host.js must be valid JavaScript.
    let check = Command::new("node")
        .arg("--check")
        .arg(out.join("host.js"))
        .output()
        .expect("run node --check");
    assert!(
        check.status.success(),
        "node --check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    // The generated crate builds for wasm32.
    let build = Command::new(cargo_path())
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&out)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env("PATH", cargo_env())
        .output()
        .expect("run cargo build --release --target wasm32-unknown-unknown");
    assert!(
        build.status.success(),
        "wasm build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let wasm = out.join("target/wasm32-unknown-unknown/release/opurom_wasm.wasm");
    assert!(wasm.is_file(), "no .wasm artifact");

    // The module instantiates in Node with an empty import object.
    let script = dir.join("conformance.mjs");
    fs::write(
        &script,
        r#"import { readFile } from "node:fs/promises";
const bytes = await readFile(process.argv[2]);
const module = await WebAssembly.instantiate(bytes, {});
const e = module.instance.exports;
const results = {
  version: e.opu_version(),
  state_len: e.opu_state_len(),
  state0: e.opu_state(0),
  tap: e.opu_tap(2),
  state0_after: e.opu_state(0),
  unknown_tap: e.opu_tap(9999),
};
console.log(JSON.stringify(results));
"#,
    )
    .expect("write conformance script");
    let run = Command::new("node")
        .arg(&script)
        .arg(&wasm)
        .output()
        .expect("run node conformance");
    assert!(
        run.status.success(),
        "node conformance failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8_lossy(&run.stdout);
    let got: serde_json::Value = serde_json::from_str(text.trim()).expect("parse conformance JSON");
    assert_eq!(got["version"], 1, "{text}");
    assert_eq!(got["state_len"], 1, "{text}");
    assert_eq!(got["state0"], 0, "{text}");
    assert_eq!(got["tap"], 1, "{text}");
    assert_eq!(got["state0_after"], 1, "{text}");
    assert_eq!(got["unknown_tap"], 0, "{text}");
}

/// The wasm emitter also handles a multi-statement handler and `valueFrom`.
#[test]
fn wasm_emitter_supports_action_chains() {
    let dir = out_dir("wasm-chain");
    let src = dir.join("chain.opu");
    fs::write(
        &src,
        r#"app Chain {
  state count = 0
  state step = 2
  Button("go") { count += 1; count += step }
}
"#,
    )
    .expect("write source");
    let ir = dir.join("chain.opuir.json");
    let r = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let out = dir.join("wasm");
    let r = bin()
        .arg("target")
        .arg(&ir)
        .arg("--target")
        .arg("wasm")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc target");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let build = Command::new(cargo_path())
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&out)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env("PATH", cargo_env())
        .output()
        .expect("run cargo build");
    assert!(
        build.status.success(),
        "wasm build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let wasm = out.join("target/wasm32-unknown-unknown/release/opurom_wasm.wasm");

    let script = dir.join("conformance.mjs");
    fs::write(
        &script,
        r#"import { readFile } from "node:fs/promises";
const bytes = await readFile(process.argv[2]);
const module = await WebAssembly.instantiate(bytes, {});
const e = module.instance.exports;
console.log(JSON.stringify({
  state_len: e.opu_state_len(),
  before: e.opu_state(0),
  patches: e.opu_tap(0),
  after: e.opu_state(0),
}));
"#,
    )
    .expect("write conformance script");
    let run = Command::new("node")
        .arg(&script)
        .arg(&wasm)
        .output()
        .expect("run node conformance");
    assert!(
        run.status.success(),
        "node conformance failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8_lossy(&run.stdout);
    let got: serde_json::Value = serde_json::from_str(text.trim()).expect("parse conformance JSON");
    assert_eq!(got["state_len"], 2, "{text}");
    assert_eq!(got["before"], 0, "{text}");
    assert_eq!(got["patches"], 2, "{text}");
    assert_eq!(got["after"], 3, "{text}");
}

/// The web emitter keeps the state declaration shape opurom-cli depends on.
#[test]
fn web_emitter_keeps_the_cli_state_shape() {
    let dir = out_dir("web-shape");
    let src = dir.join("counter.opu");
    fs::write(
        &src,
        "app Counter {\n  state count = 0\n  Text(\"{count}\")\n}\n",
    )
    .expect("write source");
    let ir = dir.join("counter.opuir.json");
    let r = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("web");
    let r = bin()
        .arg("target")
        .arg(&ir)
        .arg("--target")
        .arg("web")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc target");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(out.join("app.js")).unwrap();
    assert!(
        app_js.contains("const state = {\"count\":0};"),
        "web state declaration shape changed:\n{app_js}"
    );
}

/// R4 effects stay in host.js: the wasm module still instantiates with an empty import object.
#[test]
fn wasm_effects_use_host_callbacks_without_wasm_imports() {
    let dir = out_dir("wasm-effects");
    let src = workspace().join("tests/fixtures/async.opu");
    let ir = dir.join("async.opuir.json");
    let r = bin()
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&ir)
        .output()
        .expect("run opuromc compile");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let out = dir.join("wasm");
    let r = bin()
        .arg("target")
        .arg(&ir)
        .arg("--target")
        .arg("wasm")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run opuromc target wasm");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let host = fs::read_to_string(out.join("host.js")).unwrap();
    for marker in [
        "const HOST_CALLBACKS =",
        "const EFFECTS =",
        "async function runEffects(id)",
        "await runEffects(id)",
        "await fetch(base + path)",
    ] {
        assert!(host.contains(marker), "host.js missing {marker}");
    }
    let readme = fs::read_to_string(out.join("README.md")).unwrap();
    assert!(
        readme.contains("host callbacks"),
        "README missing honest status"
    );
    let check = Command::new("node")
        .arg("--check")
        .arg(out.join("host.js"))
        .output()
        .expect("run node --check");
    assert!(
        check.status.success(),
        "node --check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let build = Command::new(cargo_path())
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&out)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env("PATH", cargo_env())
        .output()
        .expect("run cargo build --release --target wasm32-unknown-unknown");
    assert!(
        build.status.success(),
        "wasm build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let wasm = out.join("target/wasm32-unknown-unknown/release/opurom_wasm.wasm");

    let script = dir.join("conformance.mjs");
    fs::write(
        &script,
        r#"import { readFile } from "node:fs/promises";
const bytes = await readFile(process.argv[2]);
const module = await WebAssembly.instantiate(bytes, {});
const e = module.instance.exports;
console.log(JSON.stringify({
  version: e.opu_version(),
  state_len: e.opu_state_len(),
  before: e.opu_state(0),
  patches: e.opu_tap(0),
  after: e.opu_state(0),
}));
"#,
    )
    .expect("write conformance script");
    let run = Command::new("node")
        .arg(&script)
        .arg(&wasm)
        .output()
        .expect("run node conformance");
    assert!(
        run.status.success(),
        "node conformance failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8_lossy(&run.stdout);
    let got: serde_json::Value = serde_json::from_str(text.trim()).expect("parse conformance JSON");
    assert_eq!(got["version"], 1, "{text}");
    assert_eq!(got["state_len"], 1, "{text}");
    assert_eq!(got["before"], 0, "{text}");
    assert_eq!(got["patches"], 1, "{text}");
    assert_eq!(got["after"], 1, "{text}");
}

fn _unused(_: &Path) {}
