//! Cross-target action chain ordering gate.
//!
//! A button handler with three writes is lowered to one ordered `actions`
//! array. The second write reads the state changed by the first, so reversing
//! or deduplicating actions would change program behavior. Flutter's compile
//! result keeps an explicit output and error count before a bundle is loaded
//! (`packages/flutter_tools/lib/src/compile.dart:96-114`,
//! `packages/flutter_tools/lib/src/devfs.dart:636-642`, Flutter 3.50.0).

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

const SOURCE: &str =
    "app Chain {\n  state a = 0\n  state b = 1\n  Button(\"x\") { a += b; b += a; a += 7 }\n}\n";

fn emit(ir: &PathBuf, target: &str, out: &PathBuf) {
    let result = bin()
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc target");
    assert!(
        result.status.success(),
        "{target} emit failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn action_chain_keeps_source_order_on_every_target() {
    let dir = out_dir("action-chain");
    let src = dir.join("chain.opu");
    let ir = dir.join("chain.opuir.json");
    fs::write(&src, SOURCE).expect("write source");
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
    let actions = doc["tree"]["events"]["tap"]["actions"]
        .as_array()
        .expect("action array");
    assert_eq!(actions.len(), 3, "chain must keep all three writes");
    assert_eq!(actions[0]["target"], "a");
    assert_eq!(actions[0]["valueFrom"], "b");
    assert_eq!(actions[1]["target"], "b");
    assert_eq!(actions[1]["valueFrom"], "a");
    assert_eq!(actions[2]["target"], "a");
    assert_eq!(actions[2]["value"], 7);

    for target in [
        "web", "native", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        emit(&ir, target, &dir.join(target));
    }

    let web = fs::read_to_string(dir.join("web/app.js")).expect("read web output");
    let web_a = web.find("state.a += state.b;").expect("web first write");
    let web_b = web.find("state.b += state.a;").expect("web second write");
    let web_last = web.find("state.a += 7;").expect("web third write");
    assert!(web_a < web_b && web_b < web_last, "web reordered actions");

    let native = fs::read_to_string(dir.join("native/src/main.rs")).expect("read native output");
    let native_a = native
        .find("_state.a = _state.a.saturating_add(_state.b);")
        .expect("native first");
    let native_b = native
        .find("_state.b = _state.b.saturating_add(_state.a);")
        .expect("native second");
    let native_last = native
        .find("_state.a = _state.a.saturating_add(7i64);")
        .expect("native third");
    assert!(
        native_a < native_b && native_b < native_last,
        "native reordered actions"
    );

    for (target, file) in [
        ("wechat", "pages/index/index.js"),
        ("alipay", "pages/index/index.js"),
    ] {
        let text =
            fs::read_to_string(dir.join(target).join(file)).expect("read mini-program output");
        let first = text
            .find("\"a\": this.data.a + (this.data.b)")
            .expect("mini first");
        let second = text
            .find("\"b\": this.data.b + (this.data.a)")
            .expect("mini second");
        let third = text.find("\"a\": this.data.a + (7)").expect("mini third");
        assert!(
            first < second && second < third,
            "{target} reordered actions"
        );
    }
}
