use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value as Json};

static CASE: AtomicUsize = AtomicUsize::new(0);

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn spec(name: &str) -> PathBuf {
    workspace().join("..").join("opurom-spec").join(name)
}

fn fixture(name: &str) -> PathBuf {
    workspace().join("tests/fixtures").join(name)
}

fn error_fixture(name: &str) -> PathBuf {
    fixture("errors").join(name)
}

fn out_dir(label: &str) -> PathBuf {
    let p = workspace().join("target/test-out").join(format!(
        "{}-{}",
        label,
        CASE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).expect("create test output dir");
    p
}

fn compile(src: &Path, out: &Path) -> Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
}

fn emit(ir: &Path, target: &str, out: &Path) -> Output {
    bin()
        .arg("target")
        .arg(ir)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc target")
}

fn assert_diag(stderr: &str, path: &Path) {
    let first = stderr.lines().next().unwrap_or("");
    assert!(
        first.contains(": error: "),
        "missing 'error:' header: {stderr}"
    );
    let head = first.split(": error: ").next().unwrap_or("");
    let pieces: Vec<&str> = head.split(':').collect();
    assert!(pieces.len() >= 3, "bad diagnostic prefix: {first}");
    assert_eq!(pieces[0], path.to_str().unwrap(), "wrong diagnostic path");
    let line: usize = pieces[1].parse().expect("numeric line");
    let col: usize = pieces[2].parse().expect("numeric column");
    assert!(line >= 1 && col >= 1, "bad line/col: {first}");
    assert!(
        stderr.lines().any(|l| l.starts_with("  help: ")),
        "missing help line: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "compiler panicked: {stderr}");
    assert!(!stderr.contains("thread '"), "compiler crashed: {stderr}");
}

fn assert_shape(node: &Json) {
    assert!(node["id"].is_u64());
    assert!(node["kind"].is_string());
    assert!(node["props"].is_object());
    assert!(node["children"].is_array(), "children missing: {node}");
    assert!(node["events"].is_object(), "events missing: {node}");
    for child in node["children"].as_array().unwrap() {
        assert_shape(child);
    }
}

fn compile_ok(src: &Path) -> Json {
    let out = out_dir("r2").join("out.opuir.json");
    let r = compile(src, &out);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    serde_json::from_slice(&fs::read(&out).expect("read compiled IR")).expect("parse IR")
}

fn nodes_of(ir: &Json) -> Vec<&Json> {
    fn walk<'a>(n: &'a Json, out: &mut Vec<&'a Json>) {
        out.push(n);
        for c in n["children"].as_array().unwrap() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(&ir["tree"], &mut out);
    out
}

#[test]
fn golden_counter_ir_is_deep_equal() {
    let out = out_dir("golden").join("counter.opuir.json");
    let r = compile(&spec("counter.opu"), &out);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let got: Json =
        serde_json::from_slice(&fs::read(&out).expect("read compiled IR")).expect("parse IR");
    let expected: Json =
        serde_json::from_slice(&fs::read(spec("counter.opuir.json")).expect("read golden IR"))
            .expect("parse golden IR");
    assert_eq!(got, expected, "compiled IR must be deep-equal to golden IR");
    assert_shape(&got["tree"]);
}

#[test]
fn check_accepts_counter_without_output() {
    let r = bin()
        .arg("check")
        .arg(spec("counter.opu"))
        .output()
        .expect("run opuromc check");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(r.stdout.is_empty(), "check must be diagnostics-only");
    assert!(r.stderr.is_empty(), "check must be quiet on success");
}

#[test]
fn each_error_class_is_a_friendly_diagnostic() {
    let cases: &[(&str, &str)] = &[
        ("duplicate_state.opu", "duplicate state 'count'"),
        ("unknown_state_event.opu", "unknown state 'missing'"),
        ("unknown_state_interpolation.opu", "unknown state 'nope'"),
        ("unknown_state_bind.opu", "unknown state 'nope'"),
        ("add_type_misuse.opu", "requires an int state"),
        ("toggle_type_misuse.opu", "'toggle()' requires a bool state"),
        ("assignment_mismatch.opu", "cannot assign int to bool state"),
        ("non_string_bind.opu", "'bind' requires a string state"),
        ("unknown_node_kind.opu", "unknown node kind 'Fancy'"),
        ("unknown_prop.opu", "unknown prop 'bogus' on Text"),
        (
            "missing_required_prop.opu",
            "Text requires the 'value' prop",
        ),
        (
            "numeric_prop_mismatch.opu",
            "prop 'size' expects an integer",
        ),
        ("syntax_error.opu", "unterminated string literal"),
        ("missing_brace.opu", "unexpected end of file"),
        ("mixed_block.opu", "may not mix children and statements"),
        (
            "unsupported_async.opu",
            "'async' is not yet supported in v0.1",
        ),
        ("unsupported_fn.opu", "'fn' is not yet supported in v0.1"),
        (
            "unsupported_import.opu",
            "'import' is not yet supported in v0.1",
        ),
        // R2 diagnostics.
        ("model_bad_type.opu", "unknown field type 'text'"),
        ("model_two_primaries.opu", "more than one primary field"),
        ("model_default_mismatch.opu", "must be bool, found int"),
        ("model_duplicate_name.opu", "duplicate model 'Todo'"),
        ("api_unknown_model.opu", "unknown model 'Missing'"),
        ("api_unknown_verb.opu", "unknown handler verb 'fetch'"),
        ("api_missing_path_param.opu", "needs a path parameter"),
        (
            "unsupported_else.opu",
            "'else' is not yet supported in v0.2",
        ),
        ("for_count_too_large.opu", "'for' count must be at most 64"),
        ("bad_color.opu", "invalid color '4F46E5'"),
        (
            "opacity_out_of_range.opu",
            "opacity must be between 0 and 1",
        ),
        ("shadow_out_of_range.opu", "shadow must be between 0 and 8"),
        (
            "value_from_type_mismatch.opu",
            "cannot assign string to int state",
        ),
        ("when_non_bool_state.opu", "'if' requires a bool state"),
        ("duplicate_theme.opu", "duplicate 'theme' block"),
        ("unknown_theme_key.opu", "unknown theme key 'spacing'"),
        (
            "unknown_animate_value.opu",
            "unknown animate value 'bounce'",
        ),
        ("unknown_kind_suggestion.opu", "unknown node kind 'Fancy'"),
    ];
    for &(file, message) in cases {
        let path = error_fixture(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail: {r:?}");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(message),
            "{file}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &path);

        let out = out_dir("reject").join("out.opuir.json");
        let r2 = compile(&path, &out);
        assert_eq!(r2.status.code(), Some(1), "{file}: compile must fail");
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn unknown_kind_lists_all_valid_kinds() {
    let path = error_fixture("unknown_kind_suggestion.opu");
    let r = bin().arg("check").arg(&path).output().expect("run check");
    let err = String::from_utf8_lossy(&r.stderr);
    for kind in [
        "Column",
        "Row",
        "Container",
        "Text",
        "Button",
        "Input",
        "Image",
        "Spacer",
        "Card",
        "Divider",
        "Icon",
        "Badge",
        "Progress",
        "Switch",
        "Slider",
        "Checkbox",
        "Radio",
        "Dialog",
        "Tabs",
        "List",
        "Grid",
        "Scroll",
        "Avatar",
        "Spinner",
    ] {
        assert!(err.contains(kind), "help must list kind {kind}: {err}");
    }
}

#[test]
fn feature_program_lowers_every_v0_1_form() {
    let out = out_dir("features").join("features.opuir.json");
    let r = compile(&fixture("features.opu"), &out);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(ir["ir"], "0.1");
    assert_eq!(ir["app"], "Features");
    assert_eq!(
        ir["theme"],
        json!({"mode": "system", "accent": "#4F46E5", "radius": 12})
    );
    let state = ir["state"].as_array().unwrap();
    assert_eq!(state.len(), 3);
    assert_eq!(state[0], json!({"name": "count", "type": "int", "init": 0}));
    assert_eq!(
        state[1],
        json!({"name": "flag", "type": "bool", "init": false})
    );
    assert_eq!(
        state[2],
        json!({"name": "name", "type": "string", "init": "world"})
    );
    assert_shape(&ir["tree"]);
    let children = ir["tree"]["children"].as_array().unwrap();
    assert!(children[0]["props"]["size"].is_number());
    assert_eq!(
        children[1]["props"]["value"],
        "Literal {{brace}} and {count}"
    );
    assert_eq!(
        children[2]["events"]["tap"],
        json!({"op": "set", "target": "count", "value": 0})
    );
    assert_eq!(
        children[3]["events"]["tap"],
        json!({"op": "toggle", "target": "flag"})
    );
    assert_eq!(
        children[4]["events"]["tap"],
        json!({"op": "set", "target": "name", "value": "opurom"})
    );
    assert_eq!(
        children[5]["events"]["input"],
        json!({"op": "set", "target": "name"})
    );
    assert!(children[5]["events"]["input"].get("value").is_none());
    let nested = &children[8]["children"][0]["children"];
    assert_eq!(nested[0]["events"]["tap"]["value"], -2);
    assert_eq!(nested[1]["events"]["tap"]["value"], 2);
}

#[test]
fn ui_only_program_has_no_models_or_api_arrays() {
    let ir = compile_ok(&spec("counter.opu"));
    assert!(
        ir.get("models").is_none(),
        "a UI-only program must not emit a models array"
    );
    assert!(
        ir.get("api").is_none(),
        "a UI-only program must not emit an api array"
    );
}

#[test]
fn r2_features_lower_to_the_documented_ir() {
    let ir = compile_ok(&fixture("r2_features.opu"));
    assert_eq!(ir["ir"], "0.1");
    assert_eq!(ir["app"], "R2Features");
    assert_eq!(
        ir["theme"],
        json!({"mode": "dark", "accent": "#FF4D6D", "radius": 8})
    );

    // models
    let models = ir["models"].as_array().expect("models array");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["name"], "Todo");
    assert_eq!(
        models[0]["fields"],
        json!([
            {"name": "id", "type": "int", "primary": true, "default": null},
            {"name": "title", "type": "string", "primary": false, "default": null},
            {"name": "done", "type": "bool", "primary": false, "default": false},
        ])
    );

    // api
    let api = ir["api"].as_array().expect("api array");
    assert_eq!(api.len(), 2);
    assert_eq!(api[0]["path"], "/todos");
    assert_eq!(api[0]["model"], "Todo");
    assert_eq!(api[0]["handlers"], json!(["list", "create"]));
    assert_eq!(api[1]["path"], "/todos/:id");
    assert_eq!(api[1]["handlers"], json!(["get", "update", "delete"]));

    let nodes = nodes_of(&ir);
    // Multi-statement handler lowers to an actions array.
    assert_eq!(
        nodes[2]["events"]["tap"],
        json!({"actions": [
            {"op": "add", "target": "count", "value": 1},
            {"op": "add", "target": "count", "value": 1},
        ]})
    );
    // valueFrom actions.
    assert_eq!(
        nodes[3]["events"]["tap"],
        json!({"op": "set", "target": "count", "valueFrom": "total"})
    );
    assert_eq!(
        nodes[4]["events"]["tap"],
        json!({"op": "add", "target": "count", "valueFrom": "total"})
    );
    // `if` marks every direct child with `when`.
    assert_eq!(nodes[5]["when"], "dark");
    assert_eq!(nodes[6]["when"], "dark");
    // `for` unrolls with consecutive ids and substitutes {i}.
    assert_eq!(nodes[8]["props"]["value"], "Item 0");
    assert_eq!(nodes[9]["props"]["value"], "Item 1");
    assert_eq!(nodes[10]["props"]["value"], "Item 2");
    assert_eq!(nodes[8]["id"], 8);
    assert_eq!(nodes[10]["id"], 10);
    // Atomic shortcuts lower to canonical props.
    let root = &ir["tree"];
    assert_eq!(root["props"]["padding"], 16);
    assert_eq!(root["props"]["margin"], 8);
    assert_eq!(root["props"]["color"], "#101828");
    assert_eq!(root["props"]["radius"], 12);
    // Visual props pass through.
    let card = &nodes[11];
    assert_eq!(card["props"]["shadow"], 2);
    assert_eq!(card["props"]["gradient"], "#4F46E5,#101828");
    assert_eq!(card["props"]["glass"], true);
    assert_eq!(card["props"]["opacity"], 0.95);
    assert_eq!(card["props"]["border"], "#FFFFFF");
    assert_eq!(card["props"]["animate"], "spring");
    // New kinds lower their bind semantics.
    assert_eq!(
        nodes[13]["events"]["tap"],
        json!({"op": "toggle", "target": "dark"})
    );
    assert_eq!(
        nodes[14]["events"]["input"],
        json!({"op": "set", "target": "count"})
    );
    assert_eq!(
        nodes[15]["events"]["tap"],
        json!({"op": "toggle", "target": "dark"})
    );
    assert_eq!(
        nodes[16]["events"]["tap"],
        json!({"op": "set", "target": "label", "value": "hello"})
    );
}

#[test]
fn targets_lists_all_nine_emitters() {
    let r = bin().arg("targets").output().expect("run targets");
    assert!(r.status.success());
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 9, "targets output: {text}");
    for name in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(name)),
            "missing target {name}"
        );
    }
}

#[test]
fn web_and_mini_emitters_produce_valid_js() {
    let ir = out_dir("emit-ir").join("counter.opuir.json");
    let r = compile(&spec("counter.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let cases: &[(&str, &str, &[&str], &str)] = &[
        ("web", "app.js", &["index.html", "ir.json"], ""),
        (
            "wechat",
            "index.js",
            &["app.json", "index.wxml", "index.wxss"],
            "wx:for",
        ),
        (
            "alipay",
            "index.js",
            &["app.json", "index.axml", "index.acss"],
            "a:for",
        ),
        (
            "qq",
            "index.js",
            &["app.json", "index.qml", "index.qss"],
            "qq:for",
        ),
        (
            "douyin",
            "index.js",
            &["app.json", "index.ttml", "index.ttss"],
            "tt:for",
        ),
        (
            "baidu",
            "index.js",
            &["app.json", "index.swan", "index.css"],
            "s-for",
        ),
        (
            "kuaishou",
            "index.js",
            &["app.json", "index.ksml", "index.css"],
            "ks:for",
        ),
    ];
    for &(target, js, extras, directive) in cases {
        let dir = out_dir(&format!("emit-{target}"));
        let r = emit(&ir, target, &dir);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        for extra in extras {
            assert!(dir.join(extra).is_file(), "{target}: missing {extra}");
        }
        let check = Command::new("node")
            .arg("--check")
            .arg(dir.join(js))
            .output()
            .expect("run node --check");
        assert!(
            check.status.success(),
            "{target}: node --check failed: {}",
            String::from_utf8_lossy(&check.stderr)
        );
        if target == "web" {
            let app_js = fs::read_to_string(dir.join("app.js")).unwrap();
            assert!(
                !app_js.contains("{{"),
                "web JS must have no mustache syntax"
            );
            assert!(
                app_js.contains("String(state.count)"),
                "interpolation not resolved"
            );
            assert!(app_js.contains("fillText"), "missing Canvas 2D drawing");
            assert!(!app_js.contains("ir.json"), "web JS must not interpret IR");
            // opurom-cli state restore depends on this exact shape.
            assert!(
                app_js.contains("const state = {\"count\":0};"),
                "web state declaration shape changed: {app_js}"
            );
        } else {
            let index_js = fs::read_to_string(dir.join("index.js")).unwrap();
            assert!(
                index_js.contains("setData"),
                "{target}: missing state-set API"
            );
            assert!(index_js.contains("Page("), "{target}: missing Page() entry");
            let markup_name = extras[1];
            let markup = fs::read_to_string(dir.join(markup_name)).unwrap();
            assert!(
                markup.contains(directive),
                "{target}: missing loop directive {directive}"
            );
            assert!(markup.contains("{{"), "{target}: missing binding syntax");
        }
    }
}

#[test]
fn rejects_bad_ir_version_and_unknown_target() {
    let dir = out_dir("bad-ir");
    let bad = dir.join("bad.json");
    fs::write(&bad, r#"{"ir":"9.9","app":"X"}"#).unwrap();
    let r = emit(&bad, "web", &dir.join("out"));
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unsupported IR version '9.9'"), "{err}");
    assert!(err.contains("  help: "), "{err}");

    let r = emit(&spec("counter.opuir.json"), "bogus", &dir.join("out2"));
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown target 'bogus'"), "{err}");
    assert!(err.contains("  help: "), "{err}");
}

fn cargo_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME")).join(".cargo/bin/cargo")
}

fn build_native(dir: &Path) -> (bool, String) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!(
        "{home}/.cargo/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    );
    let r = Command::new(cargo_path())
        .arg("build")
        .arg("--release")
        .arg("--offline")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env("PATH", path)
        .output()
        .expect("run cargo build --release");
    (
        r.status.success(),
        String::from_utf8_lossy(&r.stderr).to_string(),
    )
}

fn run_native(exe: &Path, stdin: &str) -> (bool, String) {
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn generated native binary");
    {
        let mut pipe = child.stdin.take().expect("stdin pipe");
        pipe.write_all(stdin.as_bytes()).expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait for native binary");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn native_emitter_builds_and_runs_with_piped_stdin() {
    let ir = out_dir("native-ir").join("features.opuir.json");
    let r = compile(&fixture("features.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("native-out");
    let r = emit(&ir, "native", &dir);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(dir.join("Cargo.toml").is_file());
    assert!(dir.join("src/main.rs").is_file());

    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );

    let (ran, stdout) = run_native(
        &dir.join("target/release/opurom_native"),
        "tap 12\ninput 6 Ada\ntap 4\nstate\nquit\n",
    );
    assert!(ran, "native binary exited nonzero");
    assert!(
        stdout.contains("state count=2 flag=false name=world"),
        "{stdout}"
    );
    assert!(stdout.contains("patch count=2"), "{stdout}");
    assert!(
        stdout.contains("state count=2 flag=false name=Ada"),
        "{stdout}"
    );
    assert!(stdout.contains("patch name=Ada"), "{stdout}");
    assert!(
        stdout.contains("state count=2 flag=true name=Ada"),
        "{stdout}"
    );
    assert!(stdout.contains("patch flag=true"), "{stdout}");
}

#[test]
fn empty_state_program_emits_warning_free_native() {
    let src = out_dir("empty").join("empty.opu");
    fs::write(&src, "app Empty {\n  Text(\"hi\")\n}\n").unwrap();
    let ir = out_dir("empty").join("empty.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("empty-native");
    let r = emit(&ir, "native", &dir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust warnings:\n{stderr}"
    );
    let (ran, stdout) = run_native(&dir.join("target/release/opurom_native"), "state\nquit\n");
    assert!(ran);
    assert!(stdout.contains("state (empty)"), "{stdout}");
}

#[test]
fn usage_error_is_a_diagnostic() {
    let r = bin().output().expect("run opuromc with no args");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains(": error: "), "{err}");
    assert!(err.contains("  help: "), "{err}");
}

// ------------------------------------------------------------------ R3 ----

fn optimize(ir: &Path, out: &Path) -> Output {
    bin()
        .arg("optimize")
        .arg(ir)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc optimize")
}

fn pretty_ir(v: &Json) -> String {
    serde_json::to_string_pretty(v).unwrap() + "\n"
}

fn state_names(ir: &Json) -> Vec<&str> {
    ir["state"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect()
}

fn node_ids(ir: &Json) -> Vec<u64> {
    nodes_of(ir)
        .iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn catalog_components_resolve_with_explicit_props_winning() {
    let dir = out_dir("r3-catalog");
    let src = dir.join("catalog.opu");
    fs::write(
        &src,
        r##"app Catalog {
  state count = 0
  Column {
    PrimaryButton("Go", color: "#000000")
    Heading("Title", size: 40)
    IconButton("X")
    Tile(padding: 4) { Text("Hi") }
  }
}
"##,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let children = ir["tree"]["children"].as_array().unwrap();

    let button = &children[0];
    assert_eq!(button["kind"], "Button");
    assert_eq!(button["props"]["label"], "Go");
    assert_eq!(
        button["props"]["color"], "#000000",
        "explicit prop must beat the #4F46E5 default"
    );

    let heading = &children[1];
    assert_eq!(heading["kind"], "Text");
    assert_eq!(heading["props"]["size"], 40, "explicit size beats 28");
    assert_eq!(heading["props"]["role"], "heading", "default stays");

    // IconButton defaults are not props the Button primitive knows: they must bypass validation.
    let icon = &children[2];
    assert_eq!(icon["kind"], "Button");
    assert_eq!(icon["props"]["width"], 40);
    assert_eq!(icon["props"]["height"], 40);

    let tile = &children[3];
    assert_eq!(tile["kind"], "Card");
    assert_eq!(tile["props"]["padding"], 4, "explicit padding beats 12");
    assert_eq!(tile["props"]["radius"], 8, "default stays");
}

#[test]
fn component_field_is_added_only_for_catalog_names() {
    let dir = out_dir("r3-component");
    let src = dir.join("component.opu");
    fs::write(
        &src,
        "app Catalog {\n  VStack(gap: 2) { Label(\"hi\", size: 9) }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["tree"]["component"], "VStack");
    assert_eq!(ir["tree"]["kind"], "Column");
    assert_eq!(ir["tree"]["props"]["gap"], 2);
    let child = &ir["tree"]["children"][0];
    assert_eq!(child["component"], "Label");
    assert_eq!(child["kind"], "Text");
    assert_eq!(
        child["props"]["size"], 9,
        "explicit size beats Label default 14"
    );

    let plain = compile_ok(&spec("counter.opu"));
    for node in nodes_of(&plain) {
        assert!(
            node.get("component").is_none(),
            "primitive kinds must not gain a component field"
        );
    }
}

#[test]
fn all_r2_primitives_stay_unchanged() {
    let dir = out_dir("r3-primitives");
    let src = dir.join("primitives.opu");
    fs::write(
        &src,
        r#"app Primitives {
  state b = false
  state i = 0
  state s = ""
  Column {
    Row { }
    Container { }
    Text("t")
    Button("b") { i += 1 }
    Input(bind: s)
    Image(src: "a.png")
    Spacer(width: 1)
    Card { }
    Divider()
    Icon(name: "star")
    Badge(label: "x")
    Progress(value: 1)
    Switch(bind: b)
    Slider(bind: i, min: 0, max: 10)
    Checkbox(bind: b)
    Radio(bind: s, value: "a")
    Dialog(title: "d") { }
    Tabs(labels: "a,b", bind: s)
    List { }
    Grid(columns: 2) { }
    Scroll(height: 100) { }
    Avatar(label: "a")
    Spinner()
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let nodes = nodes_of(&ir);
    assert_eq!(nodes.len(), 24, "expected all 24 primitive kinds");
    let mut kinds: Vec<&str> = nodes.iter().map(|n| n["kind"].as_str().unwrap()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 24, "all 24 kinds must be distinct: {kinds:?}");
    for node in &nodes {
        assert!(node.get("component").is_none());
    }
}

#[test]
fn components_lists_at_least_100_rows() {
    let r = bin().arg("components").output().expect("run components");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let last = *lines.last().expect("count line");
    assert!(last.ends_with(" components"), "final count line: {last}");
    let count: usize = last.split_whitespace().next().unwrap().parse().unwrap();
    assert!(count >= 100, "catalog count: {count}");
    assert_eq!(lines.len(), count + 1, "one row per component plus count");
    for row in &lines[..count] {
        let parts: Vec<&str> = row.split(" | ").collect();
        assert_eq!(parts.len(), 3, "row format: {row}");
        assert!(parts.iter().all(|p| !p.is_empty()), "empty row cell: {row}");
    }
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("PrimaryButton | Button | form")),
        "missing known catalog row"
    );
}

#[test]
fn vendored_catalog_matches_spec_when_present() {
    let upstream = spec("components.json");
    if !upstream.is_file() {
        return; // standalone checkout: the sibling spec repo is optional
    }
    let vendored: Json =
        serde_json::from_slice(&fs::read(workspace().join("src/catalog.json")).unwrap()).unwrap();
    let expected: Json = serde_json::from_slice(&fs::read(&upstream).unwrap()).unwrap();
    assert_eq!(
        vendored, expected,
        "vendored catalog drifted from opurom-spec"
    );
}

#[test]
fn unknown_component_kind_is_a_diagnostic() {
    let dir = out_dir("r3-unknown");
    let src = dir.join("unknown.opu");
    fs::write(&src, "app Bad {\n  NotAComponent(\"x\")\n}\n").unwrap();
    let r = bin().arg("check").arg(&src).output().expect("run check");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown node kind 'NotAComponent'"), "{err}");
    assert_diag(&err, &src);

    let out = dir.join("out.opuir.json");
    assert_eq!(compile(&src, &out).status.code(), Some(1));
    assert!(!out.exists(), "no IR may be written on error");
}

#[test]
fn optimizer_is_a_no_op_on_golden_counter_ir() {
    let golden = spec("counter.opuir.json");
    let out = out_dir("r3-opt-golden").join("counter.opuir.json");
    let r = optimize(&golden, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let expected: Json = serde_json::from_slice(&fs::read(&golden).unwrap()).unwrap();
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(got, expected, "golden IR has no dead state to remove");
    assert_eq!(state_names(&got), vec!["count"]);
    assert_shape(&got["tree"]);

    let in_bytes = fs::metadata(&golden).unwrap().len();
    let out_bytes = fs::metadata(&out).unwrap().len();
    assert_eq!(
        String::from_utf8_lossy(&r.stdout).trim().to_string(),
        format!("optimize: {in_bytes} bytes in, {out_bytes} bytes out"),
        "byte counts must come from the real files"
    );
}

#[test]
fn optimizer_removes_dead_state_and_null_props() {
    let dir = out_dir("r3-opt-dead");
    let ir_path = dir.join("dead.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1",
        "app": "DeadState",
        "state": [
            {"name": "count", "type": "int", "init": 0},
            {"name": "dead1", "type": "int", "init": 9},
            {"name": "name", "type": "string", "init": ""},
            {"name": "visible", "type": "bool", "init": false},
            {"name": "dead2", "type": "string", "init": "x"},
            {"name": "source", "type": "int", "init": 3}
        ],
        "tree": {
            "id": 0, "kind": "Column", "component": "VStack",
            "props": {"gap": 8, "shadow": null},
            "children": [
                {"id": 1, "kind": "Text", "props": {"value": "{count}", "label": null},
                 "children": [], "events": {}},
                {"id": 2, "kind": "Input", "props": {"bind": "name"}, "children": [],
                 "events": {"input": {"op": "set", "target": "name"}}},
                {"id": 3, "kind": "Text", "component": "Label", "when": "visible",
                 "props": {"value": "conditional"}, "children": [], "events": {}},
                {"id": 4, "kind": "Button", "props": {"label": "copy"}, "children": [],
                 "events": {"tap": {"actions": [
                     {"op": "set", "target": "count", "valueFrom": "source"}
                 ]}}}
            ],
            "events": {}
        },
        "theme": {"mode": "system", "accent": "#4F46E5", "radius": 12}
    });
    let text = pretty_ir(&ir);
    fs::write(&ir_path, &text).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();

    assert_eq!(
        state_names(&got),
        vec!["count", "name", "visible", "source"]
    );
    assert!(
        fs::metadata(&out).unwrap().len() < text.len() as u64,
        "optimized IR must shrink"
    );
    assert_eq!(node_ids(&got), node_ids(&ir));
    assert_eq!(got["tree"]["props"]["gap"], 8);
    assert!(
        got["tree"]["props"].get("shadow").is_none(),
        "null prop dropped"
    );
    assert!(got["tree"]["children"][0]["props"].get("label").is_none());
    assert_eq!(got["tree"]["component"], "VStack", "component preserved");
    assert_eq!(got["tree"]["children"][2]["component"], "Label");
    assert_eq!(
        got["tree"]["children"][2]["when"], "visible",
        "when not folded"
    );
    assert_eq!(
        got["tree"]["children"][3]["events"]["tap"]["actions"][0]["valueFrom"],
        "source"
    );
}

#[test]
fn optimizer_preserves_node_ids_and_state_order() {
    let dir = out_dir("r3-opt-order");
    let ir_path = dir.join("order.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1",
        "app": "Order",
        "state": [
            {"name": "a", "type": "int", "init": 0},
            {"name": "b", "type": "int", "init": 0},
            {"name": "c", "type": "bool", "init": false},
            {"name": "d", "type": "int", "init": 0},
            {"name": "e", "type": "int", "init": 1}
        ],
        "tree": {
            "id": 0, "kind": "Column", "props": {}, "children": [
                {"id": 1, "kind": "Row", "props": {}, "children": [
                    {"id": 2, "kind": "Text", "props": {"value": "{a}"},
                     "children": [], "events": {}},
                    {"id": 3, "kind": "Text", "when": "c", "props": {"value": "{e}"},
                     "children": [], "events": {}}
                ], "events": {}},
                {"id": 4, "kind": "Button", "props": {"label": "go"}, "children": [],
                 "events": {"tap": {"op": "set", "target": "a", "valueFrom": "e"}}}
            ], "events": {}
        },
        "theme": {"mode": "system", "accent": "#4F46E5", "radius": 12}
    });
    fs::write(&ir_path, pretty_ir(&ir)).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();

    assert_eq!(
        state_names(&got),
        vec!["a", "c", "e"],
        "surviving states keep their order"
    );
    assert_eq!(
        node_ids(&got),
        node_ids(&ir),
        "no nodes removed, no ids changed"
    );
    assert_eq!(nodes_of(&got).len(), 5);
    assert_eq!(got["tree"]["children"].as_array().unwrap().len(), 2);
    assert_eq!(
        got["tree"]["children"][0]["children"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(got["tree"]["children"][0]["children"][1]["when"], "c");
}

#[test]
fn optimizer_rejects_bad_ir() {
    let dir = out_dir("r3-opt-bad");
    let tree = json!({"id": 0, "kind": "Column", "props": {}, "children": [], "events": {}});
    let base = json!({"ir": "0.1", "app": "X", "state": [], "tree": tree, "theme": {}});
    let mut version = base.clone();
    version["ir"] = json!("9.9");
    let mut field = base.clone();
    field["stat"] = json!([]);
    let mut node_field = base.clone();
    node_field["tree"]["bogus"] = json!(1);
    let mut state_field = base.clone();
    state_field["state"] = json!([{"name": "a", "bogus": 1}]);
    let mut missing_tree = base.clone();
    missing_tree.as_object_mut().unwrap().remove("tree");

    let cases: &[(&str, Json, &str)] = &[
        ("version", version, "unsupported IR version '9.9'"),
        ("field", field, "unknown IR field 'stat'"),
        ("node-field", node_field, "unknown IR node field 'bogus'"),
        ("state-field", state_field, "unknown IR state field 'bogus'"),
        (
            "missing-tree",
            missing_tree,
            "IR is missing the 'tree' field",
        ),
    ];
    for &(name, ref ir, expected) in cases {
        let ir_path = dir.join(format!("{name}.json"));
        fs::write(&ir_path, pretty_ir(ir)).unwrap();
        let out = dir.join(format!("{name}.out.json"));
        let r = optimize(&ir_path, &out);
        assert_eq!(r.status.code(), Some(1), "{name} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(expected),
            "{name}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &ir_path);
        assert!(!out.exists(), "{name}: no output may be written on error");
    }
}
