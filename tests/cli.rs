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

fn example(name: &str) -> PathBuf {
    workspace().join("..").join("opurom-examples").join(name)
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

fn assert_error_shape(stderr: &str) {
    let first = stderr.lines().next().unwrap_or("");
    assert!(
        first.contains(": error: "),
        "missing 'error:' header: {stderr}"
    );
    let head = first.split(": error: ").next().unwrap_or("");
    let pieces: Vec<&str> = head.split(':').collect();
    assert!(pieces.len() >= 3, "bad diagnostic prefix: {first}");
    let line: usize = pieces[pieces.len() - 2].parse().expect("numeric line");
    let col: usize = pieces[pieces.len() - 1].parse().expect("numeric column");
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
    let out = out_dir("ui").join("out.opuir.json");
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
        // Interactive UI diagnostics.
        ("model_bad_type.opu", "unknown field type 'text'"),
        ("model_two_primaries.opu", "more than one primary field"),
        ("model_default_mismatch.opu", "must be bool, found int"),
        ("model_duplicate_name.opu", "duplicate model 'Todo'"),
        ("api_unknown_model.opu", "unknown model 'Missing'"),
        ("api_unknown_verb.opu", "unknown handler verb 'fetch'"),
        ("api_missing_path_param.opu", "needs a path parameter"),
        ("else_without_if.opu", "'else' must follow an 'if' block"),
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
        "Table",
        "Chart",
        "Calendar",
        "RichText",
        "Player",
        "Map",
        "Menu",
        "Tooltip",
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
fn features_additive_lower_to_the_documented_ir() {
    let ir = compile_ok(&fixture("features_additive.opu"));
    assert_eq!(ir["ir"], "0.1");
    assert_eq!(ir["app"], "FeaturesAdditive");
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

    let cases: &[(&str, &str, &[&str])] = &[
        ("web", "app.js", &["index.html", "ir.json"]),
        (
            "wechat",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.wxml",
                "pages/index/index.wxss",
                "app.js",
            ],
        ),
        (
            "alipay",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.axml",
                "pages/index/index.acss",
                "app.js",
            ],
        ),
        (
            "qq",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.qml",
                "pages/index/index.qss",
                "app.js",
            ],
        ),
        (
            "douyin",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.ttml",
                "pages/index/index.ttss",
                "app.js",
            ],
        ),
        (
            "baidu",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.swan",
                "pages/index/index.css",
                "app.js",
            ],
        ),
        (
            "kuaishou",
            "pages/index/index.js",
            &[
                "app.json",
                "pages/index/index.ksml",
                "pages/index/index.css",
                "app.js",
            ],
        ),
    ];
    for &(target, js, extras) in cases {
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
            let index_js = fs::read_to_string(dir.join("pages/index/index.js")).unwrap();
            assert!(
                index_js.contains("setData"),
                "{target}: missing state-set API"
            );
            assert!(index_js.contains("Page("), "{target}: missing Page() entry");
            let markup_name = extras[1];
            let markup = fs::read_to_string(dir.join(markup_name)).unwrap();
            assert!(markup.contains("{{"), "{target}: missing binding syntax");
            // The generated page must show the application only. Internal state
            // key names are compiler bookkeeping and never reach the screen.
            assert!(
                !markup.contains("__stateKeys"),
                "{target}: internal state keys leak into the rendered page"
            );
            assert!(
                !index_js.contains("__stateKeys"),
                "{target}: internal state keys leak into the page data"
            );
        }
    }
}

#[test]
fn web_layout_places_children_of_every_container_kind() {
    let ir = out_dir("gallery-ir").join("gallery.opuir.json");
    let r = compile(&example("gallery.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("gallery-web");
    let r = emit(&ir, "web", &dir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(dir.join("app.js")).unwrap();
    let draws: Vec<&str> = app_js
        .lines()
        .filter(|l| l.contains("ctx.fillText(") || l.contains("ctx.fillRect("))
        .collect();
    assert!(draws.len() > 100, "expected the full gallery to be drawn");
    for draw in &draws {
        assert!(
            !draw.contains("(0, 0,") && !draw.contains(", 0, 0)"),
            "a node is still drawn at the origin: {draw}"
        );
    }
    assert!(app_js.contains("Gauge"), "unknown kinds lost their label");
    node_check(&dir.join("app.js"));
}

#[test]
fn mini_pages_declared_in_app_json_resolve_to_real_files() {
    let ir = out_dir("page-ir").join("counter.opuir.json");
    let r = compile(&spec("counter.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let cases: &[(&str, &str, &str)] = &[
        ("wechat", "wxml", "wxss"),
        ("alipay", "axml", "acss"),
        ("qq", "qml", "qss"),
        ("douyin", "ttml", "ttss"),
        ("baidu", "swan", "css"),
        ("kuaishou", "ksml", "css"),
    ];
    for &(target, ext, style_ext) in cases {
        let dir = out_dir(&format!("page-{target}"));
        let r = emit(&ir, target, &dir);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let app_json: Json =
            serde_json::from_slice(&fs::read(dir.join("app.json")).unwrap()).unwrap();
        let pages = app_json["pages"].as_array().expect("pages array");
        assert_eq!(pages.len(), 1, "{target}: expected one page");
        let page = pages[0].as_str().expect("page path string");
        assert_eq!(page, "pages/index/index", "{target}: wrong page path");
        for suffix in ["js", ext, style_ext] {
            let f = dir.join(format!("{page}.{suffix}"));
            assert!(f.is_file(), "{target}: declared page missing {f:?}");
            assert!(fs::metadata(&f).unwrap().len() > 0, "{f:?} is empty");
        }
        let app_js = fs::read_to_string(dir.join("app.js")).unwrap();
        assert!(app_js.contains("App({})"), "{target}: missing App entry");
        node_check(&dir.join("app.js"));
        let markup = fs::read_to_string(dir.join(format!("{page}.{ext}"))).unwrap();
        assert!(
            !markup.contains("__stateKeys"),
            "{target}: internal state keys leak into the page"
        );
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
        .env_remove("OPR_BASE_URL")
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

// ------------------------------------------------------- component catalog ----

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
    let dir = out_dir("catalog");
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
    let dir = out_dir("catalog-component");
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
fn all_core_primitives_stay_unchanged() {
    let dir = out_dir("catalog-primitives");
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
fn components_lists_216_rows() {
    let r = bin().arg("components").output().expect("run components");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let last = *lines.last().expect("count line");
    assert_eq!(last, "216 components", "final count line: {last}");
    let count: usize = last.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(count, 216, "catalog count: {count}");
    assert_eq!(lines.len(), count + 1, "one row per component plus count");
    for row in &lines[..count] {
        let parts: Vec<&str> = row.split(" | ").collect();
        assert_eq!(parts.len(), 3, "row format: {row}");
        assert!(parts.iter().all(|p| !p.is_empty()), "empty row cell: {row}");
    }
    assert_eq!(
        lines
            .iter()
            .filter_map(|l| {
                let p: Vec<&str> = l.split(" | ").collect();
                if p.len() == 3 {
                    Some(p[1])
                } else {
                    None
                }
            })
            .collect::<std::collections::HashSet<_>>()
            .len(),
        100,
        "components must cover 100 distinct primitives"
    );
    for row in [
        "Gauge | Gauge | data",
        "Histogram | Histogram | data",
        "Breadcrumbs | Breadcrumbs | navigation",
        "Rating | Rating | form",
        "Spectrogram | Spectrogram | media",
        "LevelMeter | LevelMeter | feedback",
        "BracketPanel | BracketPanel | layout",
    ] {
        assert!(lines.contains(&row), "missing extended catalog row: {row}");
    }
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("PrimaryButton | Button | form")),
        "missing known catalog row"
    );
    for row in [
        "DataTable | Table | data",
        "BarChart | Chart | data",
        "MonthCalendar | Calendar | data",
        "Article | RichText | typography",
        "MediaPlayer | Player | media",
        "MapView | Map | media",
        "ContextMenu | Menu | navigation",
        "Hint | Tooltip | overlay",
    ] {
        assert!(lines.contains(&row), "missing data catalog row: {row}");
    }
}

#[test]
fn vendored_catalog_matches_spec_when_present() {
    let upstream = spec("components.json");
    if !upstream.is_file() {
        return; // standalone checkout: the sibling spec repo is optional
    }
    let vendored_bytes = fs::read(workspace().join("src/catalog.json")).unwrap();
    let expected_bytes = fs::read(&upstream).unwrap();
    assert_eq!(
        vendored_bytes, expected_bytes,
        "vendored catalog must be byte-identical to opurom-spec"
    );
    let vendored: Json = serde_json::from_slice(&vendored_bytes).unwrap();
    let expected: Json = serde_json::from_slice(&expected_bytes).unwrap();
    assert_eq!(
        vendored, expected,
        "vendored catalog drifted from opurom-spec"
    );
    assert_eq!(vendored["count"], 216);
    assert_eq!(vendored["primitives"], 100);
}

#[test]
fn unknown_component_kind_is_a_diagnostic() {
    let dir = out_dir("catalog-unknown");
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
    let out = out_dir("opt-golden").join("counter.opuir.json");
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
    let dir = out_dir("opt-dead");
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
    let dir = out_dir("opt-order");
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
    let dir = out_dir("opt-bad");
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
    let mut computed_shape = base.clone();
    computed_shape["computed"] = json!("bad");
    let mut computed_entry = base.clone();
    computed_entry["computed"] = json!([{"name": "x", "type": "int", "expr": "1", "bogus": 1}]);

    let cases: &[(&str, Json, &str)] = &[
        ("version", version, "unsupported IR version '9.9'"),
        ("field", field, "unknown IR field 'stat'"),
        ("node-field", node_field, "unknown IR node field 'bogus'"),
        ("state-field", state_field, "unknown IR state field 'bogus'"),
        (
            "computed-shape",
            computed_shape,
            "IR 'computed' must be an array",
        ),
        (
            "computed-entry",
            computed_entry,
            "unknown IR computed field 'bogus'",
        ),
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

// ------------------------------------------------ async language layer ----

fn node_check(file: &Path) {
    let r = Command::new("node")
        .arg("--check")
        .arg(file)
        .output()
        .expect("run node --check");
    assert!(
        r.status.success(),
        "node --check failed for {}: {}",
        file.display(),
        String::from_utf8_lossy(&r.stderr)
    );
}

#[test]
fn golden_counter_and_profile_ir_are_byte_identical() {
    let cases = [
        (spec("counter.opu"), fixture("golden/counter.opuir.json")),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
    ];
    for (index, (src, golden)) in cases.iter().enumerate() {
        let out = out_dir("golden-bytes").join(format!("golden-{index}.opuir.json"));
        let r = compile(src, &out);
        assert!(
            r.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let got = fs::read(&out).expect("read compiled IR");
        let expected = fs::read(golden).expect("read byte-level golden IR");
        assert_eq!(
            got,
            expected,
            "plain program changed its earlier IR bytes: {}",
            src.display()
        );
        let ir: Json = serde_json::from_slice(&got).unwrap();
        assert!(ir.get("imports").is_none());
        assert!(ir.get("models").is_none());
        assert!(ir.get("api").is_none());
        for node in nodes_of(&ir) {
            for event in node["events"].as_object().unwrap().values() {
                assert!(event.get("async").is_none(), "unexpected async key");
                assert!(event.get("effects").is_none(), "unexpected effects key");
            }
        }
    }
}

#[test]
fn async_effects_lower_to_the_documented_ir() {
    let ir = compile_ok(&fixture("async.opu"));
    assert_eq!(
        ir["tree"]["events"]["tap"],
        json!({
            "op": "add",
            "target": "count",
            "value": 1,
            "async": true,
            "effects": [
                {"kind": "delay", "ms": 50},
                {"kind": "fetch", "path": "/todos", "into": "payload"}
            ]
        })
    );
    assert_eq!(
        ir["state"][1],
        json!({"name": "payload", "type": "string", "init": "idle"})
    );
}

#[test]
fn async_diagnostics_are_friendly() {
    let cases: &[(&str, &str)] = &[
        ("await_outside_async.opu", "'await' requires an async block"),
        ("duplicate_async.opu", "duplicate 'async' keyword"),
        ("async_non_tap.opu", "'async' requires a tap-handler node"),
        ("async_no_block.opu", "'async' requires a tap-handler block"),
        (
            "delay_out_of_range.opu",
            "delay must be between 0 and 60000 ms",
        ),
        ("delay_negative.opu", "delay must be between 0 and 60000 ms"),
        ("fetch_non_string.opu", "'fetch' requires a string state"),
        (
            "fetch_unknown_state.opu",
            "unknown state 'payload' in fetch target",
        ),
        ("unknown_effect.opu", "unknown effect 'pause'"),
        ("bad_fetch_target.opu", "expected a string state name"),
    ];
    for &(file, message) in cases {
        let path = error_fixture(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(message),
            "{file}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &path);
        let out = out_dir("async-reject").join("out.opuir.json");
        let r2 = compile(&path, &out);
        assert_eq!(r2.status.code(), Some(1), "{file}: compile must fail");
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn imports_merge_declarations_roots_and_transitive_files() {
    let ir = compile_ok(&fixture("imports/main.opu"));
    assert_eq!(
        ir["imports"],
        json!(["models/todo.opu", "ui/cards.opu", "rooted.opu"])
    );
    assert_eq!(
        state_names(&ir),
        vec!["nested", "payload", "count", "rootValue", "local"]
    );
    assert_eq!(ir["models"][0]["name"], "Todo");
    assert_eq!(ir["api"][0]["path"], "/todos");
    assert_eq!(ir["api"][0]["handlers"], json!(["list"]));
    assert_eq!(
        ir["theme"],
        json!({"mode": "dark", "accent": "#FF4D6D", "radius": 8})
    );
    assert_eq!(
        ir["tree"]["children"][0]["props"]["value"],
        "{payload} {count} {nested} {rootValue} {local}"
    );
    let plain = compile_ok(&spec("counter.opu"));
    assert!(
        plain.get("imports").is_none(),
        "plain IR stays additive-free"
    );
}

#[test]
fn imports_reject_cycles_traversal_duplicates_and_bad_files() {
    let cases: &[(&str, &str)] = &[
        ("cycle_a.opu", "import cycle detected"),
        (
            "traversal.opu",
            "import path may not be absolute or contain '..'",
        ),
        (
            "missing.opu",
            "cannot read imported module 'absent.opu': file not found",
        ),
        ("non_opu.opu", "must end in '.opu'"),
        ("duplicate.opu", "duplicate imported name 'shared'"),
        (
            "disallowed.opu",
            "imported module may declare only state, model, api, theme and imports",
        ),
        (
            "disallowed_async.opu",
            "imported module may declare only state, model, api, theme and imports",
        ),
    ];
    for &(file, message) in cases {
        let path = fixture("imports/errors").join(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(message),
            "{file}: unexpected diagnostic: {err}"
        );
        assert_error_shape(&err);
        let out = out_dir("import-reject").join("out.opuir.json");
        let r2 = compile(&path, &out);
        assert_eq!(r2.status.code(), Some(1), "{file}: compile must fail");
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn async_primitives_and_catalog_names_are_accepted() {
    let dir = out_dir("async-kinds");
    let src = dir.join("kinds.opu");
    fs::write(
        &src,
        r#"app KindsProof {
  Column {
    Table(columns: 2, rows: 3) { }
    Chart(height: 120)
    Calendar(date: "2026-01-01")
    RichText("body", size: 16)
    Player(src: "a.mp4", height: 180)
    Map(center: "0,0", zoom: 2, height: 200)
    Menu(items: "a,b") { }
    Tooltip("hint") { }
    DataTable()
    BarChart()
    MonthCalendar()
    Article()
    MediaPlayer()
    MapView()
    ContextMenu()
    Hint()
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let nodes = nodes_of(&ir);
    assert_eq!(
        nodes.len(),
        17,
        "root plus eight primitives and eight catalog names"
    );
    let kinds: Vec<&str> = nodes.iter().map(|n| n["kind"].as_str().unwrap()).collect();
    for kind in [
        "Table", "Chart", "Calendar", "RichText", "Player", "Map", "Menu", "Tooltip",
    ] {
        assert!(kinds.contains(&kind), "missing primitive kind {kind}");
    }
    for (name, primitive) in [
        ("DataTable", "Table"),
        ("BarChart", "Chart"),
        ("MonthCalendar", "Calendar"),
        ("Article", "RichText"),
        ("MediaPlayer", "Player"),
        ("MapView", "Map"),
        ("ContextMenu", "Menu"),
        ("Hint", "Tooltip"),
    ] {
        let node = nodes
            .iter()
            .find(|n| n.get("component").and_then(Json::as_str) == Some(name))
            .unwrap_or_else(|| panic!("missing catalog component {name}"));
        assert_eq!(node["kind"], primitive);
    }
}

#[test]
fn optimizer_preserves_async_effect_state_and_imports() {
    let mut ir = compile_ok(&fixture("async.opu"));
    ir["imports"] = json!(["models/todo.opu"]);
    let dir = out_dir("opt-async");
    let ir_path = dir.join("async.opuir.json");
    let out = dir.join("optimized.opuir.json");
    fs::write(&ir_path, pretty_ir(&ir)).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(state_names(&got), vec!["count", "payload"]);
    assert_eq!(got["imports"], json!(["models/todo.opu"]));
    assert_eq!(got["tree"]["events"]["tap"]["async"], true);
    assert_eq!(
        got["tree"]["events"]["tap"]["effects"],
        json!([
            {"kind": "delay", "ms": 50},
            {"kind": "fetch", "path": "/todos", "into": "payload"}
        ])
    );
}

#[test]
fn every_target_emits_effect_code() {
    let ir = out_dir("effect-ir").join("async.opuir.json");
    let r = compile(&fixture("async.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let web = out_dir("effect-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let web_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(web_js.contains("const state = {\"count\":0,\"payload\":\"idle\"};"));
    assert!(web_js.contains("async function applyTap(id)"));
    assert!(web_js.contains("setTimeout(resolve, 50)"));
    assert!(web_js.contains("await fetch(\"/todos\")"));
    assert!(web_js.contains("state.payload = await response1.text();"));
    node_check(&web.join("app.js"));

    let native = out_dir("effect-native");
    let r = emit(&ir, "native", &native);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let native_rs = fs::read_to_string(native.join("src/main.rs")).unwrap();
    for marker in [
        "OPR_BASE_URL",
        "TcpStream::connect",
        "Effect::Delay(50)",
        "Effect::Fetch",
        "run_effects",
        "local: ",
    ] {
        assert!(native_rs.contains(marker), "native source missing {marker}");
    }

    let mini_cases: &[(&str, &str)] = &[
        ("wechat", "wx.request"),
        ("alipay", "my.request"),
        ("qq", "qq.request"),
        ("douyin", "tt.request"),
        ("baidu", "swan.request"),
        ("kuaishou", "ks.request"),
    ];
    for &(target, request_api) in mini_cases {
        let dir = out_dir(&format!("effect-{target}"));
        let r = emit(&ir, target, &dir);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let js = fs::read_to_string(dir.join("pages/index/index.js")).unwrap();
        for marker in [
            "async function ()",
            "await new Promise",
            "setTimeout(resolve, 50)",
            request_api,
            "setData",
            "request failed",
        ] {
            assert!(js.contains(marker), "{target} index.js missing {marker}");
        }
        node_check(&dir.join("pages/index/index.js"));
    }

    let wasm = out_dir("effect-wasm");
    let r = emit(&ir, "wasm", &wasm);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let host_js = fs::read_to_string(wasm.join("host.js")).unwrap();
    for marker in [
        "const EFFECTS =",
        "const HOST_CALLBACKS =",
        "setTimeout(resolve, ms)",
        "await fetch(base + path)",
        "async function runEffects(id)",
        "async function applyTap(id)",
        "await runEffects(id)",
    ] {
        assert!(host_js.contains(marker), "wasm host.js missing {marker}");
    }
    let readme = fs::read_to_string(wasm.join("README.md")).unwrap();
    assert!(
        readme.contains("host callbacks"),
        "wasm README missing the host-callback description"
    );
    node_check(&wasm.join("host.js"));
}

#[test]
fn native_effects_build_and_run_locally() {
    let ir = out_dir("native-ir").join("async.opuir.json");
    let r = compile(&fixture("async.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("native-run");
    let r = emit(&ir, "native", &dir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );
    let (ran, stdout) = run_native(
        &dir.join("target/release/opurom_native"),
        "tap 0\nstate\nquit\n",
    );
    assert!(ran, "native binary exited nonzero");
    assert!(
        stdout.contains("state count=1 payload=local: /todos"),
        "{stdout}"
    );
    assert!(stdout.contains("patch count=1"), "{stdout}");
}

// ---------------------------------------------- GPU and transport layer ----

#[test]
fn plain_todo_ir_is_byte_identical() {
    let out = out_dir("golden-todo").join("todo.opuir.json");
    let r = compile(&fixture("golden/todo.opu"), &out);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let got = fs::read(&out).expect("read compiled todo IR");
    let expected = fs::read(fixture("golden/todo.opuir.json")).expect("read todo golden IR");
    assert_eq!(got, expected, "plain todo IR changed its bytes");
    let ir: Json = serde_json::from_slice(&got).unwrap();
    assert!(ir.get("computed").is_none());
    assert!(ir.get("imports").is_none());
    for node in nodes_of(&ir) {
        assert!(node.get("unless").is_none());
    }
}

#[test]
fn else_branch_lowers_when_and_unless() {
    let ir = compile_ok(&fixture("else_branch.opu"));
    let nodes = nodes_of(&ir);
    let find = |value: &str| {
        nodes
            .iter()
            .find(|n| (**n)["props"]["value"].as_str() == Some(value))
            .copied()
            .unwrap()
    };
    assert_eq!(find("dark")["when"], "dark");
    assert_eq!(find("nested dark")["when"], "dark");
    assert_eq!(find("light")["unless"], "dark");
    assert_eq!(find("nested light")["unless"], "dark");
    assert!(find("dark").get("unless").is_none());
    assert!(find("light").get("when").is_none());
    assert!(ir["tree"].get("unless").is_none());
    let when_count = nodes.iter().filter(|n| (**n).get("when").is_some()).count();
    let unless_count = nodes
        .iter()
        .filter(|n| (**n).get("unless").is_some())
        .count();
    assert_eq!(when_count, 3, "the if subtree keeps its when fields");
    assert_eq!(unless_count, 3, "the else subtree gains unless fields");
}

#[test]
fn computed_and_list_lower_to_documented_ir() {
    let ir = compile_ok(&fixture("effects_features.opu"));
    assert_eq!(ir["computed"][0]["name"], "doubled");
    assert_eq!(ir["computed"][0]["type"], "int");
    assert_eq!(ir["computed"][0]["expr"], "count * 2");
    assert_eq!(
        ir["state"][3],
        json!({"name": "items", "type": "list", "init": ["a", "b"]})
    );
    let nodes = nodes_of(&ir);
    let find = |value: &str| {
        nodes
            .iter()
            .find(|n| (**n)["props"]["value"].as_str() == Some(value))
            .copied()
            .unwrap()
    };
    assert_eq!(find("dark")["when"], "dark");
    assert_eq!(find("light")["unless"], "dark");
    assert!(find("light").get("when").is_none());
    assert_eq!(find("visible")["when"], "visible");
    assert_eq!(find("hidden")["unless"], "visible");
    let unrolled: Vec<&str> = nodes
        .iter()
        .filter_map(|n| (**n)["props"]["value"].as_str())
        .filter(|v| *v == "a" || *v == "b")
        .collect();
    assert_eq!(unrolled, vec!["a", "b"]);

    let computed_ir = compile_ok(&fixture("computed_state.opu"));
    assert_eq!(
        computed_ir["computed"],
        json!([
            {"name": "doubled", "type": "int", "expr": "count * 2"},
            {"name": "total", "type": "int", "expr": "count * 2 + step - 1"},
            {"name": "flagValue", "type": "bool", "expr": "flag"},
            {"name": "label", "type": "string", "expr": "name"},
        ])
    );
    assert_eq!(
        computed_ir["state"][4],
        json!({"name": "doubled", "type": "int", "init": 4})
    );
    assert_eq!(
        computed_ir["state"][5],
        json!({"name": "total", "type": "int", "init": 6})
    );
    assert_eq!(
        computed_ir["state"][6],
        json!({"name": "flagValue", "type": "bool", "init": true})
    );
    assert_eq!(
        computed_ir["state"][7],
        json!({"name": "label", "type": "string", "init": "Ada"})
    );

    let list_ir = compile_ok(&fixture("list_state.opu"));
    assert_eq!(
        list_ir["state"][0],
        json!({"name": "items", "type": "list", "init": ["a", "b", "c"]})
    );
    let values: Vec<&str> = nodes_of(&list_ir)
        .into_iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    assert_eq!(values, vec!["a", "b", "c"]);
}

#[test]
fn language_diagnostics_are_friendly() {
    let dir = out_dir("diag");
    let fixture_cases: &[(&str, &str)] = &[
        ("else_without_if.opu", "'else' must follow an 'if' block"),
        ("duplicate_else.opu", "duplicate 'else' block"),
        ("else_non_bool.opu", "'if' requires a bool state"),
        ("else_if.opu", "'else if' is not supported yet"),
        (
            "nested_else_in_else.opu",
            "nested 'else' inside an 'else' block is not supported yet",
        ),
        (
            "computed_bad_expr.opu",
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "computed_unknown_state.opu",
            "unknown state 'missing' in computed expression",
        ),
        ("computed_duplicate_name.opu", "duplicate name 'doubled'"),
        (
            "computed_readonly.opu",
            "computed value 'doubled' is read-only",
        ),
        (
            "computed_non_int_operand.opu",
            "computed expression '+' requires int operands",
        ),
        (
            "computed_top_level.opu",
            "computed declarations belong at the top level of the app",
        ),
        (
            "list_bad_initializer.opu",
            "list initializers accept string literals only",
        ),
        (
            "list_runtime_mutation.opu",
            "list state 'items' is compile-time only",
        ),
        (
            "list_interpolation.opu",
            "list state 'items' is compile-time only",
        ),
        ("list_bind.opu", "list state 'items' is compile-time only"),
        ("list_braces.opu", "list values may not contain braces"),
        ("for_non_list.opu", "'for' requires a list state"),
        (
            "for_unknown_list.opu",
            "unknown list state 'items' in 'for'",
        ),
    ];
    let mut cases: Vec<(String, PathBuf, &str)> = Vec::new();
    for &(file, expected) in fixture_cases {
        cases.push((file.to_string(), error_fixture(file), expected));
    }
    let too_many_items = vec!["\"a\""; 65].join(", ");
    let too_many = format!(
        "app Bad {{\n  state items = [{}]\n  for (item in items) {{ Text(\"{{item}}\") }}\n}}\n",
        too_many_items
    );
    let dynamic: Vec<(String, String, &str)> = vec![
        (
            "computed_division".to_string(),
            r#"app Bad {
  state count = 0
  computed bad = count / 2
  Text("x")
}
"#
            .to_string(),
            "unexpected character '/'",
        ),
        (
            "computed_parentheses".to_string(),
            r#"app Bad {
  state count = 0
  computed bad = (count + 1) * 2
  Text("x")
}
"#
            .to_string(),
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "computed_float".to_string(),
            r#"app Bad {
  computed bad = 1.5
  Text("x")
}
"#
            .to_string(),
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "computed_bool".to_string(),
            r#"app Bad {
  computed bad = true
  Text("x")
}
"#
            .to_string(),
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "computed_trailing_token".to_string(),
            r#"app Bad {
  state count = 0
  computed bad = count * 2 3
  Text("x")
}
"#
            .to_string(),
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "computed_self_reference".to_string(),
            r#"app Bad {
  state count = 0
  computed first = count * 2
  computed second = first + 1
  Text("x")
}
"#
            .to_string(),
            "computed expression cannot read computed value 'first'",
        ),
        (
            "computed_list_reference".to_string(),
            r#"app Bad {
  state items = ["a"]
  computed bad = items + 1
  Text("x")
}
"#
            .to_string(),
            "list state 'items' is compile-time only",
        ),
        (
            "computed_bind".to_string(),
            r#"app Bad {
  state name = "Ada"
  computed label = name
  Input(bind: label)
}
"#
            .to_string(),
            "computed value 'label' is read-only",
        ),
        (
            "computed_fetch_target".to_string(),
            r#"app Bad {
  state payload = "idle"
  computed label = payload
  Button("go") async {
    await fetch("/x", label)
  }
}
"#
            .to_string(),
            "computed value 'label' is read-only",
        ),
        (
            "computed_missing_operand".to_string(),
            r#"app Bad {
  state count = 0
  computed bad = count +
  Text("x")
}
"#
            .to_string(),
            "computed expression supports only +, -, *, integer literals and declared state names",
        ),
        (
            "list_too_long".to_string(),
            too_many,
            "list 'items' has more than 64 values",
        ),
    ];
    for (name, source, expected) in dynamic {
        let path = dir.join(format!("{name}.opu"));
        fs::write(&path, source.as_bytes()).unwrap();
        cases.push((name, path, expected));
    }
    let module = dir.join("computed_module.opu");
    fs::write(&module, b"state name = \"Ada\"\ncomputed label = name\n").unwrap();
    let main = dir.join("computed_module_main.opu");
    fs::write(
        &main,
        b"import \"computed_module.opu\"\napp Bad { Text(\"x\") }\n",
    )
    .unwrap();
    cases.push((
        "computed_in_module".to_string(),
        main,
        "imported module may declare only state, model, api, theme and imports",
    ));

    for (label, path, expected) in &cases {
        let r = bin().arg("check").arg(path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{label} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(expected),
            "{label}: unexpected diagnostic: {err}"
        );
        assert_error_shape(&err);
        let out = out_dir("lang-reject").join(format!("{label}.opuir.json"));
        let r2 = compile(path, &out);
        assert_eq!(r2.status.code(), Some(1), "{label}: compile must fail");
        assert!(!out.exists(), "{label}: no IR may be written on error");
    }
}

#[test]
fn optimizer_preserves_computed_conditions_and_list_state() {
    let dir = out_dir("lang-opt");
    let ir_path = dir.join("features.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let r = compile(&fixture("effects_features.opu"), &ir_path);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let original: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(got["computed"], original["computed"]);
    assert_eq!(state_names(&got), state_names(&original));
    assert_eq!(node_ids(&got), node_ids(&original));
    assert!(
        got["state"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["type"] == "list"),
        "list state must survive dead-state elimination"
    );
    let unless_count = nodes_of(&got)
        .into_iter()
        .filter(|n| n.get("unless").is_some())
        .count();
    assert_eq!(unless_count, 2, "unless fields must survive optimization");
}

#[test]
fn emitters_apply_computed_values_and_honor_unless() {
    let ir = out_dir("emit-ir").join("features.opuir.json");
    let r = compile(&fixture("effects_features.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let native = out_dir("emit-native");
    let r = emit(&ir, "native", &native);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let native_rs = fs::read_to_string(native.join("src/main.rs")).unwrap();
    for marker in [
        "Vec<String>",
        "fn apply_computed",
        "_state.doubled = (_state.count).saturating_mul(2i64);",
        "_state.label = _state.name.clone();",
        "patches.extend(apply_computed(_state));",
    ] {
        assert!(native_rs.contains(marker), "native source missing {marker}");
    }

    let web = out_dir("emit-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    for marker in [
        "\"items\":[\"a\",\"b\"]",
        "\"doubled\":2",
        "\"visible\":false",
        "function applyComputed()",
        "state.doubled =",
        "state.label = state.name;",
        "if (!state.dark)",
        "if (!state.visible)",
        "applyComputed(); render(); return;",
        "applyComputed();\nrender();",
    ] {
        assert!(app_js.contains(marker), "web app.js missing {marker}");
    }
    node_check(&web.join("app.js"));

    let mini_cases: &[(&str, &str)] = &[
        ("wechat", "pages/index/index.wxml"),
        ("alipay", "pages/index/index.axml"),
        ("qq", "pages/index/index.qml"),
        ("douyin", "pages/index/index.ttml"),
        ("baidu", "pages/index/index.swan"),
        ("kuaishou", "pages/index/index.ksml"),
    ];
    for &(target, markup_name) in mini_cases {
        let dir = out_dir(&format!("emit-{target}"));
        let r = emit(&ir, target, &dir);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let index_js = fs::read_to_string(dir.join("pages/index/index.js")).unwrap();
        for marker in [
            "applyComputed: function ()",
            "this.applyComputed();",
            "this.setData({\"doubled\":",
            "this.data.count",
        ] {
            assert!(
                index_js.contains(marker),
                "{target} index.js missing {marker}"
            );
        }
        let markup = fs::read_to_string(dir.join(markup_name)).unwrap();
        assert!(
            markup.contains("!dark"),
            "{target}: missing unless condition"
        );
        assert!(
            markup.contains("!visible"),
            "{target}: missing computed unless condition"
        );
        node_check(&dir.join("pages/index/index.js"));
    }

    let wasm = out_dir("emit-wasm");
    let r = emit(&ir, "wasm", &wasm);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let host_js = fs::read_to_string(wasm.join("host.js")).unwrap();
    for marker in [
        "function applyComputed()",
        "mirror[\"visible\"] = mirror[\"dark\"];",
        "if (node.unless && mirror[node.unless])",
        "applyComputed();\n  render();",
    ] {
        assert!(host_js.contains(marker), "wasm host.js missing {marker}");
    }
    let lib_rs = fs::read_to_string(wasm.join("src/lib.rs")).unwrap();
    for marker in [
        "fn apply_computed()",
        "set_state(1, (state(0)).saturating_mul(2i64));",
        "count += apply_computed();",
    ] {
        assert!(lib_rs.contains(marker), "wasm lib.rs missing {marker}");
    }
    let readme = fs::read_to_string(wasm.join("README.md")).unwrap();
    assert!(readme.contains("Computed values"));
    node_check(&wasm.join("host.js"));
}

#[test]
fn native_emitter_builds_and_applies_computed_values() {
    let ir = out_dir("native-ir").join("features.opuir.json");
    let r = compile(&fixture("effects_features.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir_json: Json = serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap();
    let tap_id = nodes_of(&ir_json)
        .into_iter()
        .find(|n| n["events"].get("tap").is_some())
        .and_then(|n| n["id"].as_u64())
        .expect("tap node id");
    let dir = out_dir("native-run");
    let r = emit(&ir, "native", &dir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );
    let (ran, stdout) = run_native(
        &dir.join("target/release/opurom_native"),
        &format!("state\ntap {tap_id}\nstate\nquit\n"),
    );
    assert!(ran, "native binary exited nonzero");
    assert!(
        stdout.contains(
            "state count=1 dark=false name=Ada items=[\"a\", \"b\"] doubled=2 label=Ada visible=false"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "state count=2 dark=true name=Ada items=[\"a\", \"b\"] doubled=4 label=Ada visible=true"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("patch doubled=4"), "{stdout}");
    assert!(stdout.contains("patch visible=true"), "{stdout}");
}

// ---------------------------------------------------- platform delivery ----

#[test]
fn components_expand_to_ordinary_ir() {
    let one = compile_ok(&fixture("component_one.opu"));
    assert_shape(&one["tree"]);
    assert_eq!(one["tree"]["kind"], "Column");
    assert_eq!(one["tree"]["props"]["padding"], 8);
    let children = one["tree"]["children"].as_array().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0]["kind"], "Text");
    assert_eq!(children[0]["props"]["value"], "Hi");
    assert_eq!(children[1]["props"]["value"], "Hello Hi");
    assert_eq!(children[2]["kind"], "Badge");
    assert_eq!(children[2]["props"]["label"], "Hi");
    assert!(
        one.get("components").is_none(),
        "no component registry in IR"
    );
    for node in nodes_of(&one) {
        assert!(
            node.get("component").is_none(),
            "expansion must not add a catalog-style component key"
        );
        assert_ne!(node["kind"], "Card", "no component node survives");
    }

    let two = compile_ok(&fixture("component_two.opu"));
    assert_shape(&two["tree"]);
    assert_eq!(node_ids(&two), vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    let rows = two["tree"]["children"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["kind"], "Row");
    assert_eq!(rows[0]["children"][0]["props"]["value"], "CPU");
    assert_eq!(rows[0]["children"][1]["props"]["value"], 42);
    assert_eq!(rows[0]["children"][2]["props"]["value"], "on: true");
    assert_eq!(rows[1]["children"][0]["props"]["value"], "RAM");
    assert_eq!(rows[1]["children"][1]["props"]["value"], 8);
    assert_eq!(rows[1]["children"][2]["props"]["value"], "on: false");
    assert!(two.get("components").is_none());

    let nested = compile_ok(&fixture("component_nested.opu"));
    let values: Vec<&str> = nodes_of(&nested)
        .iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    assert_eq!(
        values,
        vec!["Hi!", "Hi", "Hi 0", "Hi 1"],
        "nested expansion plus loop unrolling"
    );

    // The declaration may also sit inside the app body, and `Card` shadows the primitive.
    let in_app = compile_ok(&fixture("component_in_app.opu"));
    assert_eq!(in_app["tree"]["kind"], "Column");
    assert_eq!(in_app["tree"]["children"][0]["kind"], "Text");
    assert_eq!(in_app["tree"]["children"][0]["props"]["value"], "Hi");
    assert!(
        !nodes_of(&in_app).iter().any(|n| n["kind"] == "Card"),
        "the custom Card expands; no Card primitive remains"
    );

    // Parameters also substitute into tap-handler statement values.
    let stmt = compile_ok(&fixture("component_stmt.opu"));
    assert_eq!(stmt["tree"]["kind"], "Button");
    assert_eq!(
        stmt["tree"]["events"]["tap"],
        json!({"op": "add", "target": "count", "value": 2}),
        "parameter literal must reach the event action"
    );
}

#[test]
fn component_diagnostics_are_friendly() {
    let cases: &[(&str, &str)] = &[
        (
            "component_recursive.opu",
            "component cycle detected: Card -> Card",
        ),
        (
            "component_cycle.opu",
            "component cycle detected: Alpha -> Beta -> Alpha",
        ),
        (
            "component_arity.opu",
            "component 'Card' expects 2 arguments, found 1",
        ),
        ("component_unknown.opu", "unknown component 'Missing'"),
        (
            "component_param_type.opu",
            "parameter 'title' of component 'Card' expects string, found int",
        ),
        ("component_duplicate_name.opu", "duplicate component 'Card'"),
        (
            "component_duplicate_param.opu",
            "duplicate parameter 'title' in component 'Card'",
        ),
        (
            "component_top_level_app.opu",
            "component 'Card' cannot be used as a top-level app",
        ),
        (
            "component_bad_param_type.opu",
            "unknown parameter type 'text'",
        ),
        ("component_named_arg.opu", "arguments must be positional"),
        (
            "component_inferred_type_mismatch.opu",
            "expects string, found int",
        ),
        (
            "component_unknown_in_body.opu",
            "unknown component 'Missing' in component 'Card'",
        ),
        (
            "component_if_condition.opu",
            "component parameter 'flag' cannot be used as an 'if' condition",
        ),
        (
            "component_for_iterable.opu",
            "component parameter 'items' cannot be used as a 'for' iterable",
        ),
        (
            "component_write_param.opu",
            "component parameter 'step' is read-only in event statements",
        ),
    ];
    for &(file, message) in cases {
        let path = error_fixture(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(message),
            "{file}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &path);
        let out = out_dir("component-reject").join(format!("{file}.opuir.json"));
        let r2 = compile(&path, &out);
        assert_eq!(r2.status.code(), Some(1), "{file}: compile must fail");
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn expanded_ir_keeps_emitters_catalog_and_optimizer() {
    let ir_path = out_dir("component-emit-ir").join("two.opuir.json");
    let r = compile(&fixture("component_two.opu"), &ir_path);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let original: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();

    let targets: &[(&str, &str)] = &[
        ("native", "src/main.rs"),
        ("web", "app.js"),
        ("wechat", "pages/index/index.js"),
        ("alipay", "pages/index/index.js"),
        ("qq", "pages/index/index.js"),
        ("douyin", "pages/index/index.js"),
        ("baidu", "pages/index/index.js"),
        ("kuaishou", "pages/index/index.js"),
        ("wasm", "host.js"),
    ];
    let mut web_dir = PathBuf::new();
    for &(target, artifact) in targets {
        let dir = out_dir(&format!("component-emit-{target}"));
        if target == "web" {
            web_dir = dir.clone();
        }
        let r = emit(&ir_path, target, &dir);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        assert!(dir.join(artifact).is_file(), "{target}: missing {artifact}");
        if artifact.ends_with(".js") {
            node_check(&dir.join(artifact));
        }
    }
    let app_js = fs::read_to_string(web_dir.join("app.js")).unwrap();
    for marker in ["CPU", "RAM", "on: true", "on: false"] {
        assert!(app_js.contains(marker), "web JS missing {marker}");
    }

    let optimized = out_dir("component-opt").join("two.opuir.json");
    let r = optimize(&ir_path, &optimized);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&optimized).unwrap()).unwrap();
    assert_eq!(got, original, "expanded IR has no dead state to remove");

    let r = bin().arg("components").output().expect("run components");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        String::from_utf8_lossy(&r.stdout).lines().last().unwrap(),
        "216 components",
        "the built-in catalog is unchanged"
    );
}

#[test]
fn component_plain_ir_stays_byte_identical() {
    let cases = [
        (spec("counter.opu"), fixture("golden/counter.opuir.json")),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
    ];
    for (index, (src, golden)) in cases.iter().enumerate() {
        let out = out_dir("component-golden").join(format!("plain-{index}.opuir.json"));
        let r = compile(src, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(
            fs::read(&out).unwrap(),
            fs::read(golden).unwrap(),
            "plain program changed its IR bytes: {}",
            src.display()
        );
        let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert!(ir.get("components").is_none());
        assert!(ir.get("computed").is_none());
        assert!(ir.get("imports").is_none());
    }
}

// ----------------------------------------- component library hardening ----

/// The 68 canonical extended primitive names, derived from `opuromc components`
/// rows where the component name equals its primitive (the extended catalog
/// convention).
fn canonical_extended_kinds() -> Vec<String> {
    let r = bin().arg("components").output().expect("run components");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = String::from_utf8_lossy(&r.stdout);
    let kinds: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split(" | ").collect();
            if p.len() == 3 && p[0] == p[1] {
                Some(p[0].to_string())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        kinds.len(),
        68,
        "expected 68 canonical extended kinds: {kinds:?}"
    );
    kinds
}

#[test]
fn extended_primitives_and_catalog_names_are_accepted() {
    let kinds = canonical_extended_kinds();
    let dir = out_dir("extended-kinds");
    let src_path = dir.join("kinds.opu");
    let mut src = String::from("app ExtendedKindsProof {\n  Column {\n");
    for k in &kinds {
        src.push_str(&format!(
            "    {k}(value: 42, segments: 4, labels: \"a,b\")\n"
        ));
    }
    // Secondary catalog names expand to their primitive and gain `component`.
    src.push_str("    GaugeMini(value: 10)\n    HistogramRounded()\n  }\n}\n");
    fs::write(&src_path, &src).unwrap();
    let ir = compile_ok(&src_path);
    let nodes = nodes_of(&ir);
    assert_eq!(nodes.len(), 1 + kinds.len() + 2);
    let lowered: Vec<&str> = nodes.iter().map(|n| n["kind"].as_str().unwrap()).collect();
    for k in &kinds {
        assert!(lowered.contains(&k.as_str()), "missing primitive kind {k}");
    }
    for name in ["GaugeMini", "HistogramRounded"] {
        let node = nodes
            .iter()
            .find(|n| n.get("component").and_then(Json::as_str) == Some(name))
            .unwrap_or_else(|| panic!("missing catalog component {name}"));
        let expected = if name == "GaugeMini" {
            "Gauge"
        } else {
            "Histogram"
        };
        assert_eq!(node["kind"], expected);
    }
    // Explicit props win over catalog defaults; unknown props stay a diagnostic.
    let explicit = nodes
        .iter()
        .find(|n| n["kind"] == "Gauge" && n.get("component").is_none())
        .unwrap();
    assert_eq!(explicit["props"]["value"], 42);
    assert_eq!(explicit["props"]["segments"], 4);
}

#[test]
fn all_nine_emitters_keep_working() {
    let kinds = canonical_extended_kinds();
    let dir = out_dir("extended-emit");
    let src_path = dir.join("kinds.opu");
    let mut src = String::from("app EmitProof {\n  state n = 0\n  Column {\n");
    for k in kinds.iter().take(12) {
        src.push_str(&format!("    {k}(value: 7)\n"));
    }
    src.push_str("    Button(\"go\") { n += 1 }\n  }\n}\n");
    fs::write(&src_path, &src).unwrap();
    let ir_path = dir.join("kinds.opuir.json");
    let r = compile(&src_path, &ir_path);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm",
    ] {
        let out = dir.join(format!("out-{target}"));
        let e = emit(&ir_path, target, &out);
        assert!(
            e.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&e.stderr)
        );
        assert!(out.is_dir(), "{target}: no output dir");
    }
}

// ------------------------------------------------- static types and inference ---

#[test]
fn every_type_error_class_is_a_friendly_diagnostic() {
    let cases: &[(&str, &str)] = &[
        (
            "state_annotation_mismatch.opu",
            "state 'count' does not match its declared type: expected int, found string",
        ),
        (
            "state_annotation_unknown_type.opu",
            "unknown state type 'number'",
        ),
        (
            "assign_int_to_string.opu",
            "cannot assign int to string state 'name': expected string, found int",
        ),
        (
            "assign_string_to_int.opu",
            "cannot assign string to int state 'count': expected int, found string",
        ),
        (
            "value_from_int_to_string.opu",
            "cannot assign int to string state 'b': expected string, found int",
        ),
        (
            "add_string_state.opu",
            "'+=' / '-=' requires an int state, found string state 'label'",
        ),
        (
            "condition_on_int_state.opu",
            "'if' requires a bool state, found int state 'count': expected bool, found int",
        ),
        (
            "condition_on_string_state.opu",
            "'if' requires a bool state, found string state 'name': expected bool, found string",
        ),
        (
            "computed_arithmetic_on_string.opu",
            "computed expression '+' requires int operands: expected int, found string",
        ),
        (
            "computed_arithmetic_on_bool.opu",
            "computed expression '*' requires int operands: expected int, found bool",
        ),
        (
            "component_param_conflict.opu",
            "parameter 'title' of component 'Card' expects string, found int",
        ),
        (
            "component_param_inferred_first.opu",
            "parameter 'title' of component 'Card' expects int, found string",
        ),
        (
            "unknown_state_in_interpolation.opu",
            "unknown state 'nope' in Text interpolation",
        ),
    ];
    for &(file, message) in cases {
        let path = error_fixture(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(message),
            "{file}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &path);
        let out = out_dir("type-reject").join(format!("{file}.opuir.json"));
        let r2 = compile(&path, &out);
        assert_eq!(r2.status.code(), Some(1), "{file}: compile must fail");
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn type_diagnostics_name_expected_found_and_position() {
    let path = error_fixture("assign_string_to_int.opu");
    let r = bin().arg("check").arg(&path).output().expect("run check");
    let err = String::from_utf8_lossy(&r.stderr);
    let first = err.lines().next().unwrap_or("");
    assert!(
        first.contains(
            ": error: cannot assign string to int state 'count': expected int, found string"
        ),
        "{first}"
    );
    assert!(
        err.contains("  help: use an int literal or change the state initializer"),
        "{err}"
    );
    // The position points at the offending value, not at the declaration.
    let head = first.split(": error: ").next().unwrap_or("");
    let pieces: Vec<&str> = head.split(':').collect();
    let line: usize = pieces[pieces.len() - 2].parse().expect("numeric line");
    assert_eq!(line, 3, "the error points at the statement line: {first}");
}

#[test]
fn typed_and_untyped_source_lower_to_identical_ir() {
    let typed = compile_ok(&fixture("typed_program.opu"));
    assert_eq!(
        typed["state"][0],
        json!({"name": "count", "type": "int", "init": 2})
    );
    assert_eq!(
        typed["state"][1],
        json!({"name": "step", "type": "int", "init": 3})
    );
    assert_eq!(
        typed["state"][2],
        json!({"name": "flag", "type": "bool", "init": true})
    );
    assert_eq!(
        typed["state"][3],
        json!({"name": "name", "type": "string", "init": "Ada"})
    );
    // The computed values are inferred from their expressions.
    assert_eq!(
        typed["computed"],
        json!([
            {"name": "doubled", "type": "int", "expr": "count * 2"},
            {"name": "total", "type": "int", "expr": "count * 2 + step - 1"},
            {"name": "visible", "type": "bool", "expr": "flag"},
            {"name": "label", "type": "string", "expr": "name"},
        ])
    );
    // The same program without annotations lowers to the same IR.
    let dir = out_dir("typed-vs-untyped");
    let untyped = dir.join("untyped.opu");
    fs::write(
        &untyped,
        r#"app TypedProgram {
  state count = 2
  state step = 3
  state flag = true
  state name = "Ada"

  computed doubled = count * 2
  computed total = count * 2 + step - 1
  computed visible = flag
  computed label = name

  Column(gap: 8) {
    Text("Count: {count} doubled: {doubled} total: {total}")
    Text("Label: {label}")
    if (visible) {
      Text("visible")
    } else {
      Text("hidden")
    }
    Button("+") { count += step }
    Button("copy") { count = total }
  }
}
"#,
    )
    .unwrap();
    let typed_ir = compile_ok(&fixture("typed_program.opu"));
    let untyped_ir = compile_ok(&untyped);
    assert_eq!(
        typed_ir, untyped_ir,
        "a type annotation must not change the lowered IR"
    );
}

#[test]
fn types_are_inferred_across_nested_expressions_and_components() {
    let ir = compile_ok(&fixture("typed_program.opu"));
    // A nested computed expression keeps the int type of its operands.
    let total = ir["state"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "total")
        .unwrap();
    assert_eq!(*total, json!({"name": "total", "type": "int", "init": 6}));
    // A computed value that reads a bool state stays bool and drives the condition.
    let visible = ir["state"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "visible")
        .unwrap();
    assert_eq!(
        *visible,
        json!({"name": "visible", "type": "bool", "init": true})
    );
    let nodes = nodes_of(&ir);
    let find = |value: &str| {
        nodes
            .iter()
            .find(|n| (**n)["props"]["value"].as_str() == Some(value))
            .copied()
            .unwrap()
    };
    assert_eq!(find("visible")["when"], "visible");
    assert_eq!(find("hidden")["unless"], "visible");

    // A declared component parameter keeps its type through expansion.
    let component = compile_ok(&fixture("typed_component.opu"));
    let values: Vec<&str> = nodes_of(&component)
        .iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    assert!(values.contains(&"CPU"), "{values:?}");
    assert!(values.contains(&"on: true"), "{values:?}");
    // The inferred parameter of Plain reaches the expanded node.
    let inferred = compile_ok(&fixture("inferred_component.opu"));
    let texts: Vec<&str> = nodes_of(&inferred)
        .iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    assert_eq!(texts, vec!["hello", "world"]);
}

#[test]
fn component_parameter_types_come_from_the_first_call_site() {
    // The first use site fixes the type of an unannotated parameter.
    let dir = out_dir("component-infer");
    let src = dir.join("infer.opu");
    fs::write(
        &src,
        r#"component Plain(title) {
  Text("{title}")
}

app Infer {
  Plain("hello")
  Plain("world")
}
"#,
    )
    .unwrap();
    assert!(compile(&src, &dir.join("infer.opuir.json"))
        .status
        .success());

    // A conflicting later use site is rejected with the inferred type.
    let bad = dir.join("conflict.opu");
    fs::write(
        &bad,
        r#"component Plain(title) {
  Text("{title}")
}

app Conflict {
  Plain("hello")
  Plain(7)
}
"#,
    )
    .unwrap();
    let r = bin().arg("check").arg(&bad).output().expect("run check");
    assert_eq!(
        r.status.code(),
        Some(1),
        "the conflicting use site must fail"
    );
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(
        err.contains("parameter 'title' of component 'Plain' expects string, found int"),
        "{err}"
    );
    assert!(
        err.contains("was inferred as string from the first use"),
        "{err}"
    );
}

#[test]
fn example_programs_keep_byte_identical_ir() {
    let cases = [
        (spec("counter.opu"), fixture("golden/counter.opuir.json")),
        (example("profile.opu"), fixture("golden/profile.opuir.json")),
        (example("todo.opu"), fixture("golden/todo.opuir.json")),
        (
            example("showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (example("gallery.opu"), fixture("golden/gallery.opuir.json")),
    ];
    for (src, golden) in cases {
        if !src.is_file() {
            continue; // standalone checkout: the sibling example repo is optional
        }
        let out = out_dir("example-golden").join("out.opuir.json");
        let r = compile(&src, &out);
        assert!(
            r.status.success(),
            "compile failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        assert_eq!(
            fs::read(&out).unwrap(),
            fs::read(&golden).unwrap(),
            "the program changed its IR bytes: {}",
            src.display()
        );
    }
}
