use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value as Json};

static CASE: AtomicUsize = AtomicUsize::new(0);

/// Mobile-component alias names added to the component
/// catalog; each maps onto an existing drawn primitive, never a new one.
const MOBILE_ALIAS_NAMES: &[&str] = &[
    "Scaffold",
    "AppBar",
    "FloatingActionButton",
    "NavigationBar",
    "NavigationRail",
    "NavigationDrawer",
    "InputChip",
    "SnackBar",
    "BottomSheet",
    "ModalBottomSheet",
    "SegmentedButton",
    "DividerVertical",
    "ListTile",
    "ExpansionTile",
    "TooltipCard",
    "CupertinoButton",
    "CupertinoSwitch",
    "CupertinoSlider",
    "CupertinoTextField",
    "CupertinoNavigationBar",
    "CupertinoAlertDialog",
    "CupertinoTabBar",
    "CupertinoPicker",
    "CupertinoSegmentedControl",
];

const CORE_PRIMITIVES: &[&str] = &[
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
];

const FEEDBACK_DISPLAY_COMPONENTS: &[(&str, &str, &str)] = &[
    ("Confirm", "", "Dialog"),
    ("Drawer", "", "Sidebar"),
    ("Toast", "", "Card"),
    ("SuccessToast", "", "Card"),
    ("WarningToast", "", "Card"),
    ("ErrorToast", "", "Card"),
    ("Tooltip", "", "Tooltip"),
    ("HoverTooltip", "", "Tooltip"),
    ("Notification", "", "Card"),
    ("Badge", "label: \"8\"", "Badge"),
    ("StatusBadge", "", "Badge"),
    ("ProgressBar", "", "Progress"),
    ("Progress", "value: 50", "Progress"),
    ("RadialProgress", "", "ProgressRing"),
    ("Spinner", "", "Spinner"),
    ("LoadingSpinner", "", "Spinner"),
    ("Skeleton", "", "Container"),
    ("SkeletonCard", "", "Card"),
    ("Avatar", "label: \"JD\"", "Avatar"),
    ("StatusAvatar", "", "Avatar"),
];

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract(name: &str) -> PathBuf {
    workspace()
        .join("..")
        .join("opurom-examples")
        .join("golden")
        .join(name)
}

fn example(name: &str) -> PathBuf {
    workspace()
        .join("..")
        .join("opurom-examples")
        .join("golden")
        .join(name)
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

/// Raw lowering with default compile passes disabled; for tests that assert
/// the pre-fold IR shape.
fn compile_raw(src: &Path, out: &Path) -> Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .arg("--no-fold")
        .output()
        .expect("run opuromc compile --no-fold")
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
    let r = bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(&out)
        .arg("--no-fold")
        .output()
        .expect("run opuromc compile --no-fold");
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
    let r = compile(&contract("counter.opu"), &out);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let got: Json =
        serde_json::from_slice(&fs::read(&out).expect("read compiled IR")).expect("parse IR");
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).expect("read golden IR"))
            .expect("parse golden IR");
    assert_eq!(got, expected, "compiled IR must be deep-equal to golden IR");
    assert_shape(&got["tree"]);
}

#[test]
fn check_accepts_counter_without_output() {
    let r = bin()
        .arg("check")
        .arg(contract("counter.opu"))
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
            "'async' is not yet supported in 0.1.0",
        ),
        ("unsupported_fn.opu", "'fn' is not yet supported in 0.1.0"),
        (
            "unsupported_import.opu",
            "'import' is not yet supported in 0.1.0",
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
            "duration_out_of_range.opu",
            "duration must be between 0 and 10000",
        ),
        ("delay_type_misuse.opu", "prop 'delay' expects an integer"),
        (
            "value_from_type_mismatch.opu",
            "cannot assign string to int state",
        ),
        ("when_non_bool_state.opu", "'if' requires a bool state"),
        ("duplicate_theme.opu", "duplicate 'theme' block"),
        ("unknown_theme_key.opu", "unknown theme key 'spacing'"),
        ("theme_font_scale_type.opu", "'fontScale' expects a number"),
        (
            "unknown_animate_value.opu",
            "unknown animate value 'bounce'",
        ),
        ("unknown_kind_suggestion.opu", "unknown node kind 'Fancy'"),
        ("i18n_bad_tag.opu", "invalid locale tag 'EN_us'"),
        (
            "i18n_unsupported_set.opu",
            "set locale target 'fr' is not supported",
        ),
        ("i18n_dup_key.opu", "duplicate catalog key 'hi'"),
        ("i18n_dup_locale.opu", "duplicate catalog for locale 'en'"),
        ("quality_bad_msaa.opu", "unsupported msaa value 2"),
        (
            "quality_bad_gradient.opu",
            "unsupported gradientBits value 8",
        ),
        ("quality_bad_curve.opu", "unknown curve mode 'polygon'"),
        (
            "quality_unknown_field.opu",
            "unknown quality field 'shadow'",
        ),
        (
            "boundary_string.opu",
            "prop 'repaintBoundary' expects true or false",
        ),
        (
            "boundary_number.opu",
            "prop 'relayoutBoundary' expects true or false",
        ),
        ("boundary_missing.opu", "expected a literal or identifier"),
        (
            "platform_unknown_capability.opu",
            "unknown platform capability 'bluetooth'",
        ),
        (
            "platform_bad_field_type.opu",
            "request field 'id' expects int",
        ),
        (
            "platform_bad_result_type.opu",
            "PlatformCall 'result' requires a bool state",
        ),
        (
            "grid_fill_string.opu",
            "prop 'fillHeight' expects true or false",
        ),
        (
            "grid_fill_int.opu",
            "prop 'fillHeight' expects true or false",
        ),
        ("grid_fill_missing.opu", "expected a literal or identifier"),
        (
            "grid_fill_non_grid.opu",
            "unknown prop 'fillHeight' on Column",
        ),
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
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

#[test]
fn unknown_prop_help_names_the_accepted_set() {
    // Container kinds take no kind-specific props; the help must still name
    // the generic set instead of an empty list.
    let dir = out_dir("unknown-prop-help");
    let src = dir.join("help.opu");
    fs::write(&src, "app B { Card(bogus: 1) { Text(\"x\") } }\n").unwrap();
    let r = compile(&src, &dir.join("out.opuir.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("unknown prop 'bogus' on Card"), "{stderr}");
    assert!(stderr.contains("Card accepts: padding"), "{stderr}");
    assert!(!stderr.contains("accepts: ;"), "empty help list: {stderr}");
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
    assert_eq!(ir["ir"], "0.1.0");
    assert_eq!(ir["app"], "Features");
    assert_eq!(
        ir["theme"],
        json!({"mode": "system", "accent": "#097FEB", "radius": 12})
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
    let ir = compile_ok(&contract("counter.opu"));
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
    assert_eq!(ir["ir"], "0.1.0");
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
    assert_eq!(card["props"]["gradient"], "#097FEB,#101828");
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
fn gesture_and_navigation_lower_to_the_documented_ir() {
    let dir = out_dir("gesture-nav");

    // A gesture prop names the event that carries the handler.
    let gesture_src = dir.join("gestures.opu");
    fs::write(
        &gesture_src,
        r#"app GestureDemo {
  state count = 0
  Column(gap: 8) {
    Button("Double", gesture: doubleTap) { count += 1 }
    Button("Long", gesture: longPress) { count += 2 }
    Button("Drag", gesture: drag) { count += 3 }
    Button("Plain") { count += 4 }
    Button("NoHandler", gesture: tap)
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&gesture_src);
    let nodes = nodes_of(&ir);
    assert_eq!(nodes.len(), 6);
    assert_eq!(
        nodes[1]["events"]["doubleTap"],
        json!({"op": "add", "target": "count", "value": 1})
    );
    assert_eq!(
        nodes[2]["events"]["longPress"],
        json!({"op": "add", "target": "count", "value": 2})
    );
    assert_eq!(
        nodes[3]["events"]["drag"],
        json!({"op": "add", "target": "count", "value": 3})
    );
    assert_eq!(
        nodes[4]["events"]["tap"],
        json!({"op": "add", "target": "count", "value": 4})
    );
    // A gesture with no handler still binds a legal no-op action.
    assert_eq!(nodes[5]["events"]["tap"], json!({"actions": []}));
    // `gesture` is a binding, not a visual prop.
    assert!(nodes[1]["props"].get("gesture").is_none());
    assert!(nodes[0]["events"].as_object().unwrap().is_empty());

    // Legacy `page Name;` markers still validate `push:` targets, but no longer
    // emit a names array (retired in favor of per-route trees under `routes`).
    let nav_src = dir.join("navigation.opu");
    fs::write(
        &nav_src,
        r#"app NavDemo {
  page Home
  page Settings
  Button("Go") { push: Settings }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&nav_src);
    assert!(ir.get("pages").is_none(), "pages names array is retired");
    assert_eq!(
        nodes_of(&ir)[0]["events"]["tap"],
        json!({"op": "push", "target": "Settings"})
    );

    // An unknown gesture value is a friendly diagnostic.
    let bad_gesture = dir.join("bad_gesture.opu");
    fs::write(
        &bad_gesture,
        "app Bad {\n  Text(\"x\", gesture: bounce)\n}\n",
    )
    .unwrap();
    let r = bin()
        .arg("check")
        .arg(&bad_gesture)
        .output()
        .expect("run check");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown gesture value 'bounce'"), "{err}");
    assert_diag(&err, &bad_gesture);

    // An unknown page name is a friendly diagnostic.
    let bad_page = dir.join("bad_page.opu");
    fs::write(
        &bad_page,
        "app Bad {\n  page Home\n  Button(\"Go\") { push: Missing }\n}\n",
    )
    .unwrap();
    let r = bin()
        .arg("check")
        .arg(&bad_page)
        .output()
        .expect("run check");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown page 'Missing'"), "{err}");
    assert_diag(&err, &bad_page);
}

#[test]
fn targets_lists_all_eleven_emitters() {
    let expected = [
        "native", "web", "web-dom", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou",
        "wasm", "harmony",
    ];
    let r = bin().arg("targets").output().expect("run targets");
    assert!(r.status.success());
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), expected.len(), "targets output: {text}");
    for name in expected {
        assert!(
            lines.iter().any(|l| l.starts_with(name)),
            "missing target {name}"
        );
    }

    let r = bin()
        .arg("emit")
        .arg("--list")
        .output()
        .expect("run emit --list");
    assert!(r.status.success());
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), expected.len(), "emit --list output: {text}");
    for name in expected {
        assert!(
            lines.iter().any(|l| l.starts_with(name)),
            "emit --list missing target {name}"
        );
    }
}

#[test]
fn harmony_emitter_produces_a_valid_arkts_project() {
    let dir = out_dir("emit-harmony");
    let ir = dir.join("harmony.opuir.json");
    let src = dir.join("harmony.opu");
    fs::write(
        &src,
        "app HarmonyProbe {\n  page Home;\n  page Detail;\n  state count = 0\n  state name = \"\"\n  state visible = true\n  \
         computed doubled = count + count\n  Column(padding: 8) {\n    Row {\n      \
         Text(\"{count}\")\n      Text(\"{doubled}\")\n    }\n    \
         Button(\"Go\", variant: primary, tone: success) { count += 1 }\n    \
         Input(bind: name, placeholder: \"Type\", required: true, minLength: 3, maxLength: 12)\n    \
         Button(\"Open\") { push: Detail }\n    Button(\"Check\") { form.validate() }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let r = emit(&ir, "harmony", &dir.join("out"));
    assert!(
        r.status.success(),
        "harmony emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let out = dir.join("out");

    // The genuine ArkUI project files, not placeholder stubs.
    for artifact in [
        "oh-package.json5",
        "build-profile.json5",
        "hvigorfile.ts",
        "AppScope/app.json5",
        "AppScope/resources/base/element/string.json",
        "src/main/module.json5",
        "src/main/ets/entryability/EntryAbility.ets",
        "src/main/ets/pages/Index.ets",
        "src/main/ets/pages/Index.ir.json",
        "src/main/resources/base/profile/main_pages.json",
    ] {
        assert!(out.join(artifact).is_file(), "missing {artifact}");
    }

    // The heavy artifacts embed the *full* IR.
    let embedded = fs::read_to_string(out.join("src/main/ets/pages/Index.ir.json")).unwrap();
    let original = fs::read_to_string(&ir).unwrap();
    assert_eq!(embedded, original, "Index.ir.json must equal the input IR");

    let ets = fs::read_to_string(out.join("src/main/ets/pages/Index.ets")).unwrap();
    for marker in [
        "@Entry",
        "@Component",
        "struct Index",
        "build()",
        "Column()",
        "Row()",
        "Text(",
        "Button()",
        "TextInput(",
        "@State s0",
        "@State s1",
        "@State s2",
        "Opurom_IR_JSON",
        "this.push(",
        "this.validateForm(",
        "OpuromDesign",
        "OpuromValidator",
        "new OpuromValidator(5, true, 3, 12, ",
        "\"primary\"",
        "\"success\"",
    ] {
        assert!(ets.contains(marker), "Index.ets missing {marker}");
    }
    // Computed values lower to ArkTS arithmetic over the state fields.
    assert!(
        ets.contains("this.s3 = (this.s0 + this.s0);"),
        "computed value was not lowered to ArkTS"
    );
    assert!(
        ets.contains("Text(this.opuText([String(this.s0)]))"),
        "IR text interpolation did not map to a Text() call"
    );
    assert!(!ets.contains("TODO"), "harmony emitter left a TODO stub");

    // Syntax sanity: balanced code braces and the ArkTS entry shape.
    let opens = ets.matches('{').count();
    let closes = ets.matches('}').count();
    assert_eq!(opens, closes, "unbalanced braces in Index.ets");
    assert!(
        ets.contains("@Entry\n@Component\nstruct Index"),
        "ArkUI entry decorators are not adjacent"
    );
}

#[test]
fn web_and_mini_emitters_produce_valid_js() {
    let ir = out_dir("emit-ir").join("counter.opuir.json");
    let r = compile(&contract("counter.opu"), &ir);
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
fn canvas_and_mini_emitters_draw_real_icon_glyphs() {
    // The canvas web emitter and every mini-program emitter must paint the
    // canonical icon glyph (not the bare name as text), matching the native
    // renderer and the web-dom emitter.
    let dir = out_dir("emit-icons");
    let ir = dir.join("icons.opuir.json");
    let src = dir.join("icons.opu");
    fs::write(
        &src,
        "app IconEmit {\n  Column(gap: 8) {\n    Icon(name: \"home\")\n    Icon(name: \"star\")\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    // Canvas web: the glyph is drawn with a Path2D stroke, not fillText.
    let web = out_dir("emit-icons-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        app_js.contains("new Path2D"),
        "canvas web must stroke the icon path"
    );
    // The canonical home glyph path is embedded, not the bare name.
    assert!(
        app_js.contains("M3 11 L12 3 L21 11"),
        "canvas web must embed the canonical icon path"
    );

    // Each mini target: a real inline SVG data-URI <image>, no bare name text.
    let cases: &[(&str, &str)] = &[
        ("wechat", "pages/index/index.wxml"),
        ("alipay", "pages/index/index.axml"),
        ("qq", "pages/index/index.qml"),
        ("douyin", "pages/index/index.ttml"),
        ("baidu", "pages/index/index.swan"),
        ("kuaishou", "pages/index/index.ksml"),
    ];
    for &(target, markup) in cases {
        let out = out_dir(&format!("emit-icons-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let html = fs::read_to_string(out.join(markup)).unwrap();
        assert!(
            html.contains("data:image/svg+xml"),
            "{target}: must emit an inline SVG icon"
        );
        assert!(
            !html.contains("<text>home</text>"),
            "{target}: must not render the icon name as text"
        );
    }
}

#[test]
fn canvas_and_mini_emitters_expose_explicit_a11y_roles() {
    // An explicit `role` prop must reach the canvas web a11y mirror and every
    // mini-program element; a node without a role must not gain any attribute.
    let dir = out_dir("emit-a11y");
    let ir = dir.join("a11y.opuir.json");
    let src = dir.join("a11y.opu");
    fs::write(
        &src,
        "app A11y {\n  Column {\n    Text(\"Title\", role: \"heading\")\n    \
         Button(\"Save\", role: \"button\") { }\n    Text(\"plain\")\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    // Canvas web: a hidden, screen-reader-readable mirror in index.html.
    let web = out_dir("emit-a11y-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let html = fs::read_to_string(web.join("index.html")).unwrap();
    assert!(
        html.contains("id=\"a11y\""),
        "web: missing a11y mirror container"
    );
    assert!(
        html.contains("class=\"sr-only\""),
        "web: mirror must be visually hidden"
    );
    assert!(html.contains(".sr-only{"), "web: missing sr-only CSS rule");
    assert!(
        html.contains("role=\"heading\""),
        "web: missing heading role"
    );
    assert!(html.contains("role=\"button\""), "web: missing button role");
    assert!(
        html.contains("aria-label=\"Title\""),
        "web: missing heading label"
    );
    assert!(
        html.contains("aria-label=\"Save\""),
        "web: missing button label"
    );
    assert_eq!(
        html.matches("role=\"").count(),
        2,
        "web: only roled nodes are mirrored"
    );

    // Each mini target: aria-role + aria-label on the roled elements only.
    let cases: &[(&str, &str)] = &[
        ("wechat", "pages/index/index.wxml"),
        ("alipay", "pages/index/index.axml"),
        ("qq", "pages/index/index.qml"),
        ("douyin", "pages/index/index.ttml"),
        ("baidu", "pages/index/index.swan"),
        ("kuaishou", "pages/index/index.ksml"),
    ];
    for &(target, markup) in cases {
        let out = out_dir(&format!("emit-a11y-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let text = fs::read_to_string(out.join(markup)).unwrap();
        assert!(
            text.contains("aria-role=\"heading\""),
            "{target}: missing heading aria-role"
        );
        assert!(
            text.contains("aria-role=\"button\""),
            "{target}: missing button aria-role"
        );
        assert!(
            text.contains("aria-label=\"Title\""),
            "{target}: missing heading aria-label"
        );
        assert!(
            text.contains("aria-label=\"Save\""),
            "{target}: missing button aria-label"
        );
        assert_eq!(
            text.matches("aria-role=").count(),
            2,
            "{target}: an unroled node must not gain aria-role"
        );
    }

    // Regression: a program without roles still builds and emits valid JS.
    // Use an inline source so the test never depends on a sibling repo (CI
    // clones only opurom-lang).
    let counter = dir.join("plain.opu");
    fs::write(
        &counter,
        "app Plain {\n  Column {\n    Text(\"hello\")\n    Button(\"Go\") { }\n  }\n}\n",
    )
    .unwrap();
    let counter_ir = dir.join("plain.opuir.json");
    let r = compile(&counter, &counter_ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_web = out_dir("emit-a11y-counter-web");
    let r = emit(&counter_ir, "web", &plain_web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_html = fs::read_to_string(plain_web.join("index.html")).unwrap();
    assert!(
        plain_html.contains("<div id=\"a11y\" class=\"sr-only\"></div>"),
        "web: a page without roles emits an empty mirror"
    );
    let check = Command::new("node")
        .arg("--check")
        .arg(plain_web.join("app.js"))
        .output()
        .expect("run node --check");
    assert!(
        check.status.success(),
        "counter web app.js invalid: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let plain_wechat = out_dir("emit-a11y-counter-wechat");
    let r = emit(&counter_ir, "wechat", &plain_wechat);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxml = fs::read_to_string(plain_wechat.join("pages/index/index.wxml")).unwrap();
    assert!(
        !wxml.contains("aria-role"),
        "wechat: a page without roles must not gain aria-role"
    );
}

#[test]
fn canvas_and_mini_emitters_apply_effects_and_animation() {
    // The canvas `web` target and the mini targets must map the per-node
    // visual-effect props and the `animate` entry curve, matching the native
    // renderer and the web-dom emitter.
    let dir = out_dir("emit-effects");
    let ir = dir.join("effects.opuir.json");
    let src = dir.join("effects.opu");
    fs::write(
        &src,
        "app EffectsEmit {\n  Column(gap: 8) {\n    Container(gradient: \"#111111,#eeeeee\", shadow: 3, glass: true, border: \"#000000\", animate: spring)\n    Button(\"Go\", animate: ease) { }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    // Canvas web: a vertical two-stop gradient, a soft shadow, and the entry
    // animation on the canvas element.
    let web = out_dir("emit-effects-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        app_js.contains("createLinearGradient"),
        "canvas web must draw a gradient"
    );
    assert!(
        app_js.contains("shadowColor"),
        "canvas web must draw a shadow"
    );
    let web_html = fs::read_to_string(web.join("index.html")).unwrap();
    assert!(
        web_html.contains("opurom-entry"),
        "canvas web must carry the entry keyframes"
    );
    assert!(
        web_html.contains("opu-anim"),
        "canvas web must animate on load"
    );

    // Mini target: the same effects as CSS plus the entry animation.
    let wx = out_dir("emit-effects-wechat");
    let r = emit(&ir, "wechat", &wx);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxss = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(
        wxss.contains("linear-gradient"),
        "mini must map gradient to CSS"
    );
    assert!(wxss.contains("box-shadow"), "mini must map shadow to CSS");
    assert!(
        wxss.contains("backdrop-filter"),
        "mini must map glass to CSS"
    );
    assert!(
        wxss.contains("opurom-entry"),
        "mini must carry the entry keyframes"
    );
    let wxml = fs::read_to_string(wx.join("pages/index/index.wxml")).unwrap();
    assert!(
        wxml.contains("anim-entry"),
        "mini must animate the annotated nodes"
    );
}

#[test]
fn collision_curve_compiles_and_emits_across_targets() {
    let dir = out_dir("emit-collision");
    let ir = dir.join("collision.opuir.json");
    let src = dir.join("collision.opu");
    fs::write(
        &src,
        "app Collision {\n  Column(gap: 8) {\n    Container(bg: \"#112233\", animate: collision) { Text(\"bump\") }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let lowered = fs::read_to_string(&ir).unwrap();
    assert!(
        lowered.contains("\"collision\""),
        "IR carries the collision curve"
    );

    let wx = out_dir("emit-collision-wechat");
    let r = emit(&ir, "wechat", &wx);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxml = fs::read_to_string(wx.join("pages/index/index.wxml")).unwrap();
    assert!(
        wxml.contains("anim-collision"),
        "mini must carry the collision animation class"
    );
    let wxss = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(
        wxss.contains("anim-collision"),
        "mini must define the collision timing function"
    );

    // An unknown animate value is still a friendly diagnostic.
    let bad = dir.join("bad.opu");
    fs::write(
        &bad,
        "app B { Column() { Container(animate: bouncy) { } } }\n",
    )
    .unwrap();
    let r = compile(&bad, &dir.join("bad.opuir.json"));
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("unknown animate value"));
}

#[test]
fn shared_clock_stagger_is_opt_in_and_validated() {
    let dir = out_dir("shared-clock");
    let src = dir.join("clock.opu");
    fs::write(
        &src,
        "app Home { Column {\n  Text(\"A\", animClock: \"home\", animInterval: [0, 500], duration: 800)\n  Text(\"B\", animClock: \"home\", animInterval: [250, 750], duration: 800)\n  Text(\"plain\")\n} }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let nodes = nodes_of(&ir);
    let a = nodes
        .iter()
        .find(|n| n["props"]["value"] == "A")
        .expect("A node");
    assert_eq!(a["props"]["animClock"], "home");
    assert_eq!(a["props"]["animInterval"][0], 0);
    assert_eq!(a["props"]["animInterval"][1], 500);
    let b = nodes
        .iter()
        .find(|n| n["props"]["value"] == "B")
        .expect("B node");
    assert_eq!(b["props"]["animInterval"][0], 250);
    assert_eq!(b["props"]["animInterval"][1], 750);
    let plain = nodes
        .iter()
        .find(|n| n["props"]["value"] == "plain")
        .expect("plain node");
    assert!(plain["props"].get("animClock").is_none());
    assert!(plain["props"].get("animInterval").is_none());

    let golden = compile_ok(&contract("counter.opu"));
    assert_eq!(
        golden,
        serde_json::from_slice::<Json>(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap()
    );

    for (body, diagnostic) in [
        (
            "app B { Text(\"A\", animClock: \"c\", duration: 800) Text(\"B\", animClock: \"c\", duration: 400) }",
            "conflicting durations",
        ),
        (
            "app B { Text(\"A\", animClock: \"c\") }",
            "requires an explicit duration",
        ),
        (
            "app B { Text(\"A\", animClock: \"c\", animInterval: [500, 500], duration: 800) }",
            "0 <= begin < end <= 1000",
        ),
        (
            "app B { Text(\"A\", animInterval: [0, 1001]) }",
            "0 <= begin < end <= 1000",
        ),
        (
            "app B { Text(\"A\", animInterval: [0]) }",
            "an interval needs exactly two integers",
        ),
    ] {
        let bad = dir.join(format!("bad-{}.opu", diagnostic.split_whitespace().next().unwrap()));
        fs::write(&bad, body).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{body}");
        assert!(
            String::from_utf8_lossy(&r.stderr).contains(diagnostic),
            "{body}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}

#[test]
fn text_typography_is_opt_in_and_validated() {
    let dir = out_dir("text-typography");
    let src = dir.join("typography.opu");
    fs::write(
        &src,
        "app Typography { Column {\n  Text(\"Headline\", size: 24, weight: \"bold\", letterSpacing: -2, fontPx: 34)\n  Text(\"plain\")\n} }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let nodes = nodes_of(&ir);
    let headline = nodes
        .iter()
        .find(|n| n["kind"] == "Text" && n["props"]["value"] == "Headline")
        .expect("headline Text node");
    assert_eq!(headline["props"]["weight"], "bold");
    assert_eq!(headline["props"]["letterSpacing"], -2);
    assert_eq!(headline["props"]["fontPx"], 34);
    let plain = nodes
        .iter()
        .find(|n| n["kind"] == "Text" && n["props"]["value"] == "plain")
        .expect("plain Text node");
    for key in ["weight", "letterSpacing", "fontPx"] {
        assert!(
            plain["props"].get(key).is_none(),
            "{key} leaked to default text"
        );
    }

    let golden = compile_ok(&contract("counter.opu"));
    assert_eq!(
        golden,
        serde_json::from_slice::<Json>(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap()
    );

    for (body, diagnostic) in [
        (
            "app B { Text(\"x\", weight: \"heavy\") }",
            "prop 'weight' expects 'normal' or 'bold'",
        ),
        (
            "app B { Text(\"x\", fontPx: 0) }",
            "fontPx must be a positive integer",
        ),
        (
            "app B { Text(\"x\", fontPx: -3) }",
            "fontPx must be a positive integer",
        ),
        (
            "app B { Text(\"x\", letterSpacing: \"2\") }",
            "prop 'letterSpacing' expects an integer",
        ),
        (
            "app B { Column(weight: \"bold\") { } }",
            "unknown prop 'weight' on Column",
        ),
    ] {
        let bad = dir.join(format!(
            "bad-{}.opu",
            diagnostic.split_whitespace().next().unwrap()
        ));
        fs::write(&bad, body).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{body}");
        assert!(
            String::from_utf8_lossy(&r.stderr).contains(diagnostic),
            "{body}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}

#[test]
fn column_span_is_additive_and_grid_scoped() {
    let dir = out_dir("column-span");
    let src = dir.join("span.opu");
    fs::write(
        &src,
        "app Span { Grid(columns: 4) {\n  Container(columnSpan: 2) { }\n  Container() { }\n  Container(columnSpan: 0) { }\n} }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let grid = ir["tree"].clone();
    let children = grid["children"].as_array().expect("grid children");
    assert_eq!(children[0]["props"]["columnSpan"], 2);
    assert!(children[1]["props"].get("columnSpan").is_none());
    assert_eq!(children[2]["props"]["columnSpan"], 0);

    // The golden path stays byte-identical: no declaration, no prop.
    let plain = compile_ok(&contract("counter.opu"));
    assert_eq!(
        plain,
        serde_json::from_slice::<Json>(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap()
    );

    for (body, diagnostic) in [
        (
            "app B { Column { Container(columnSpan: 2) { } } }",
            "columnSpan requires a Grid parent",
        ),
        (
            "app B { Grid(columns: 4) { Container(columnSpan: -1) { } } }",
            "columnSpan must be non-negative",
        ),
        (
            "app B { state n: int = 1; Grid(columns: 4) { Container(columnSpan: n) { } } }",
            "columnSpan expects a non-negative integer",
        ),
        (
            "app B { Grid(columns: 4) { Container(columnSpan: \"two\") { } } }",
            "columnSpan expects a non-negative integer",
        ),
    ] {
        let bad = dir.join(format!(
            "bad-{}.opu",
            diagnostic.split_whitespace().next().unwrap()
        ));
        fs::write(&bad, body).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{body}");
        assert!(
            String::from_utf8_lossy(&r.stderr).contains(diagnostic),
            "{body}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}

#[test]
fn icon_style_and_flip_rtl_are_additive_opt_ins() {
    // Both props are generic and additive: absent or explicitly default emits
    // no IR key, so nodes without them stay byte-identical.
    let dir = out_dir("icon-style-rtl");
    let src = dir.join("icons.opu");
    fs::write(
        &src,
        "app I {\n  Column {\n    Icon(name: \"star\", iconStyle: filled, flipRtl: true)\n    Icon(name: \"star\")\n    Icon(name: \"star\", iconStyle: outlined, flipRtl: false)\n  }\n}\n",
    )
    .unwrap();
    let doc = compile_ok(&src);
    let icons: Vec<&Json> = nodes_of(&doc)
        .into_iter()
        .filter(|n| n["kind"] == "Icon")
        .collect();
    assert_eq!(icons.len(), 3);
    assert_eq!(icons[0]["props"]["iconStyle"], "filled");
    assert_eq!(icons[0]["props"]["flipRtl"], true);
    for key in ["iconStyle", "flipRtl"] {
        assert!(icons[1]["props"].get(key).is_none(), "{key} absent");
        assert!(
            icons[2]["props"].get(key).is_none(),
            "explicit default {key} must emit no key"
        );
    }

    // Explicit defaults compile to the exact same bytes as omitting them:
    // two otherwise-identical programs, one declaring both defaults.
    let plain = dir.join("plain.opu");
    fs::write(
        &plain,
        "app I {\n  Column {\n    Icon(name: \"star\")\n    Icon(name: \"home\")\n  }\n}\n",
    )
    .unwrap();
    let explicit = dir.join("explicit.opu");
    fs::write(
        &explicit,
        "app I {\n  Column {\n    Icon(name: \"star\", iconStyle: outlined, flipRtl: false)\n    Icon(name: \"home\", iconStyle: outlined)\n  }\n}\n",
    )
    .unwrap();
    let a = dir.join("plain.opuir.json");
    let b = dir.join("explicit.opuir.json");
    assert!(compile(&plain, &a).status.success());
    assert!(compile(&explicit, &b).status.success());
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());

    // Golden unchanged with no declaration.
    let golden = compile_ok(&contract("counter.opu"));
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    assert_eq!(golden, expected);

    for (body, needle) in [
        (
            "app B { Icon(name: \"star\", iconStyle: bold) }",
            "unknown iconStyle 'bold'",
        ),
        (
            "app B { Icon(name: \"star\", iconStyle: 1) }",
            "prop 'iconStyle' expects a keyword",
        ),
        (
            "app B { Icon(name: \"star\", flipRtl: 1) }",
            "prop 'flipRtl' expects true or false",
        ),
        (
            "app B { Icon(name: \"star\", flipRtl: \"yes\") }",
            "prop 'flipRtl' expects true or false",
        ),
    ] {
        let bad = dir.join("bad.opu");
        fs::write(&bad, body).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{body} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(err.contains(needle), "wanted {needle}, got {err}");
        assert_diag(&err, &bad);
    }
}

#[test]
fn motion_tokens_emit_across_targets_without_touching_defaults() {
    let dir = out_dir("motion-tokens");
    let src = dir.join("tokens.opu");
    fs::write(
        &src,
        "app MT {\n  Column(gap: 4) {\n    Container(bg: \"#112233\", animate: fast) { }\n    Container(animate: medium) { }\n    Container(animate: large) { }\n    Container(animate: color) { }\n  }\n}\n",
    )
    .unwrap();
    let ir = dir.join("tokens.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let lowered = fs::read_to_string(&ir).unwrap();
    for token in ["fast", "medium", "large", "color"] {
        assert!(lowered.contains(&format!("\"{token}\"")), "{lowered}");
    }

    let mini = dir.join("wechat");
    let r = emit(&ir, "wechat", &mini);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxss = fs::read_to_string(mini.join("pages/index/index.wxss")).unwrap();
    for rule in [
        "120ms cubic-bezier(0.4, 0, 0.2, 1)",
        "220ms cubic-bezier(0.34, 1.56, 0.64, 1)",
        "280ms cubic-bezier(0.22, 1, 0.36, 1)",
        "180ms ease",
    ] {
        assert!(wxss.contains(rule), "{wxss}");
    }

    let web = dir.join("web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let html = fs::read_to_string(web.join("index.html")).unwrap();
    assert!(
        html.contains("120ms cubic-bezier(0.4, 0, 0.2, 1)"),
        "{html}"
    );

    // No token: the emitted animation CSS stays byte-identical to the default.
    let plain = dir.join("plain");
    let r = emit(&contract("counter.opuir.json"), "wechat", &plain);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_css = fs::read_to_string(plain.join("pages/index/index.wxss")).unwrap();
    assert!(!plain_css.contains("anim-fast"), "{plain_css}");
    let plain_web = dir.join("plain-web");
    let r = emit(&contract("counter.opuir.json"), "web", &plain_web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_html = fs::read_to_string(plain_web.join("index.html")).unwrap();
    assert!(!plain_html.contains("opu-anim"), "{plain_html}");
    assert!(!plain_html.contains("anim-fast"), "{plain_html}");

    // A near miss stays a readable diagnostic.
    let bad = dir.join("bad.opu");
    fs::write(&bad, "app B { Container(animate: fastest) { } }\n").unwrap();
    let r = compile(&bad, &dir.join("bad.opuir.json"));
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("unknown animate value"));
}

#[test]
fn i18n_fields_are_additive_and_absent_without_declarations() {
    let ir = compile_ok(&fixture("features_additive.opu"));
    assert!(
        ir.get("locale").is_none(),
        "no locale field without a declaration"
    );
    assert!(ir.get("directionality").is_none());
    assert!(ir.get("messages").is_none());
    // The counter golden (byte contract) still compiles deep-equal.
    let out = out_dir("i18n-golden");
    let golden = contract("counter.opuir.json");
    let compiled = out.join("counter.opuir.json");
    let r = compile(&example("counter.opu"), &compiled);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        serde_json::from_slice::<Json>(&fs::read(&compiled).unwrap()).unwrap(),
        serde_json::from_slice::<Json>(&fs::read(golden).unwrap()).unwrap()
    );
}

#[test]
fn i18n_ir_emits_across_ten_targets_with_locale_patch() {
    let ir = fixture("i18n.opu");
    let compiled = out_dir("i18n-emit").join("i18n.opuir.json");
    let r = compile(&ir, &compiled);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    // web-dom's independent validator lives in opurom-web and is out of this
    // crate's scope; every emitter owned here accepts the reserved target.
    let mut native_out = None;
    let mut harmony_out = None;
    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let out = out_dir(&format!("i18n-emit-{target}"));
        let r = emit(&compiled, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        if target == "native" {
            native_out = Some(out);
        } else if target == "harmony" {
            harmony_out = Some(out);
        }
    }
    // Native and harmony carry the locale switch as a reserved signal.
    let native = fs::read_to_string(native_out.unwrap().join("src/main.rs")).unwrap();
    assert!(native.contains("@locale"), "{native}");
    let harmony =
        fs::read_to_string(harmony_out.unwrap().join("src/main/ets/pages/Index.ets")).unwrap();
    assert!(
        harmony.contains("@locale") || harmony.contains("setLocale"),
        "{harmony}"
    );
}

#[test]
fn quality_decl_lowers_to_root_render_quality() {
    let ir = compile_ok(&fixture("quality.opu"));
    assert_eq!(
        ir["renderQuality"],
        json!({
            "msaa": 4,
            "gradientBits": 16,
            "curve": "bezier",
            "stroke": "physical",
            "textBaseline": "grid"
        })
    );
    // Additive: a program without `quality` emits no renderQuality field, and
    // the checked-in counter/todo goldens keep their exact bytes.
    let plain = compile_ok(&fixture("features_additive.opu"));
    assert!(plain.get("renderQuality").is_none());
    let dir = out_dir("quality-golden");
    let counter = dir.join("counter.opuir.json");
    let r = compile(&example("counter.opu"), &counter);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        serde_json::from_slice::<Json>(&fs::read(&counter).unwrap()).unwrap(),
        serde_json::from_slice::<Json>(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap()
    );
}

#[test]
fn optimizer_preserves_render_quality_field() {
    let dir = out_dir("opt-render-quality");
    let raw = dir.join("raw.opuir.json");
    let r = compile(&fixture("quality.opu"), &raw);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("optimized.opuir.json");
    let r = optimize(&raw, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(got["renderQuality"]["msaa"], 4);
    assert_eq!(got["renderQuality"]["gradientBits"], 16);

    let plain_raw = dir.join("plain.opuir.json");
    let r = compile(&fixture("features_additive.opu"), &plain_raw);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_out = dir.join("plain-optimized.opuir.json");
    let r = optimize(&plain_raw, &plain_out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain: Json = serde_json::from_slice(&fs::read(&plain_out).unwrap()).unwrap();
    assert!(
        plain.get("renderQuality").is_none(),
        "no quality decl means no renderQuality after optimize"
    );
}

#[test]
fn i18n_v1_lowers_root_fields_catalogs_and_locale_patch() {
    let ir = compile_ok(&fixture("i18n.opu"));
    assert_eq!(
        ir["locale"],
        json!({"active": "en", "supported": ["en", "fr", "zh-CN"]})
    );
    assert_eq!(ir["directionality"], "rtl");
    assert_eq!(
        ir["messages"],
        json!({
            "bye": {"en": "Goodbye"},
            "greet": {"en": "Hello", "fr": "Bonjour", "zh-CN": "Ni hao"}
        })
    );
    let values: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter_map(|n| {
            if n["kind"] == "Text" {
                n["props"]["value"].as_str()
            } else {
                None
            }
        })
        .collect();
    // {@key} resolves by active locale; a missing key renders its literal key;
    // {{ is a literal brace.
    assert_eq!(values[0], "Hello, {name}");
    assert_eq!(values[1], "missing");
    assert_eq!(values[2], "{@greet}");
    let button = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Button")
        .expect("button");
    assert_eq!(
        button["events"]["tap"],
        json!({"op": "set", "target": "@locale", "value": "fr"})
    );
}

/// Fallback to the first supported locale when active has no translation.
#[test]
fn i18n_catalog_falls_back_to_first_supported() {
    let dir = out_dir("i18n-fallback");
    let src = dir.join("fallback.opu");
    fs::write(
        &src,
        "app F {\n  locale { active: \"fr\", supported: [\"fr\", \"en\"] }\n  catalog fr { hi: \"Salut\" }\n  catalog en { hi: \"Hi\", bye: \"Bye\" }\n  Column { Text(\"{@hi}|{@bye}\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let text = nodes_of(&ir)[1]["props"]["value"].as_str().unwrap();
    // Active and the fallback (first supported) are both fr: `hi` resolves
    // there, `bye` is missing in fr and falls back to the literal key.
    assert_eq!(text, "Salut|bye");
}

#[test]
fn duration_and_delay_tune_the_entry_animation() {
    // Additive: duration/delay lower as integer props only when declared.
    let src = fixture("duration_delay.opu");
    let ir = compile_ok(&src);
    let nodes = nodes_of(&ir);
    let card = nodes
        .iter()
        .find(|n| n["kind"] == "Card")
        .expect("Card node in fixture");
    assert_eq!(card["props"]["duration"], 300);
    assert_eq!(card["props"]["delay"], 100);
    let plain = nodes
        .iter()
        .find(|n| n["kind"] == "Text" && n["props"]["value"] == "plain")
        .expect("plain Text node");
    assert!(plain["props"].get("duration").is_none());
    assert!(plain["props"].get("delay").is_none());

    // Bounds match opurom-render's ANIMATE_MAX_MS: 0..=10000 accepted,
    // 10001 rejected at the source position.
    let dir = out_dir("duration-bounds");
    let ok = dir.join("ok.opu");
    fs::write(&ok, "app B { Container(animate: ease, duration: 10000) }\n").unwrap();
    assert!(compile_ok(&ok)["tree"]["props"]["duration"] == 10000);
}

#[test]
fn flutter_curve_and_reverse_props_lower_additively() {
    // Each Flutter curve/reverse/reduce-motion prop lowers only when declared,
    // so the default IR and frames stay byte-identical.
    let dir = out_dir("flutter-curves");
    let src = dir.join("curves.opu");
    fs::write(
        &src,
        "app C {\n  Column {\n    Container(animate: ease, curve: easeOutBack, reverseCurve: easeIn, reverseDuration: 200, animationBehavior: preserve, overshoot: true)\n    Container(animate: ease)\n  }\n}\n",
    )
    .unwrap();
    let doc = compile_ok(&src);
    let containers: Vec<&Json> = nodes_of(&doc)
        .into_iter()
        .filter(|n| n["kind"] == "Container")
        .collect();
    assert_eq!(containers.len(), 2);
    let declared = containers[0]["props"].as_object().unwrap();
    assert_eq!(declared["curve"], "easeOutBack");
    assert_eq!(declared["reverseCurve"], "easeIn");
    assert_eq!(declared["reverseDuration"], 200);
    assert_eq!(declared["animationBehavior"], "preserve");
    assert_eq!(declared["overshoot"], true);
    let plain = containers[1]["props"].as_object().unwrap();
    for key in [
        "curve",
        "reverseCurve",
        "reverseDuration",
        "animationBehavior",
        "overshoot",
    ] {
        assert!(
            plain.get(key).is_none(),
            "{key} must be absent when undeclared"
        );
    }

    let first = dir.join("curves-a.opuir.json");
    let again = dir.join("curves-b.opuir.json");
    assert!(compile(&src, &first).status.success());
    assert!(compile(&src, &again).status.success());
    assert_eq!(fs::read(&first).unwrap(), fs::read(&again).unwrap());

    for (text, needle) in [
        (
            "app B { Container(animate: ease, curve: wobble) }\n",
            "unknown curve 'wobble'",
        ),
        (
            "app B { Container(animate: ease, reverseCurve: 1) }\n",
            "prop 'reverseCurve' expects a curve name",
        ),
        (
            "app B { Container(animate: ease, animationBehavior: fast) }\n",
            "unknown animationBehavior 'fast'",
        ),
        (
            "app B { Container(animate: ease, reverseDuration: 20000) }\n",
            "reverseDuration must be between 0 and 10000",
        ),
        (
            "app B { Container(animate: ease, overshoot: 1) }\n",
            "prop 'overshoot' expects true or false",
        ),
    ] {
        let bad = dir.join("bad.opu");
        fs::write(&bad, text).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{needle}: must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(err.contains(needle), "wanted {needle}, got {err}");
        assert_diag(&err, &bad);
    }
}

#[test]
fn boundary_props_lower_only_when_declared() {
    let ir = compile_ok(&fixture("render_boundaries.opu"));
    let nodes = nodes_of(&ir);
    let card = nodes
        .iter()
        .find(|n| n["kind"] == "Card")
        .expect("Card node in fixture");
    assert_eq!(card["props"]["repaintBoundary"], true);
    assert!(card["props"].get("relayoutBoundary").is_none());
    let container = nodes
        .iter()
        .find(|n| n["kind"] == "Container")
        .expect("Container node in fixture");
    assert_eq!(container["props"]["repaintBoundary"], false);
    assert_eq!(container["props"]["relayoutBoundary"], true);
    let cached = nodes
        .iter()
        .find(|n| n["props"]["value"] == "cached")
        .expect("cached Text node");
    assert_eq!(cached["props"]["relayoutBoundary"], false);
    for n in nodes
        .iter()
        .filter(|n| n["props"]["value"] == "plain" || n["props"]["value"] == "fixed")
    {
        assert!(n["props"].get("repaintBoundary").is_none());
        assert!(n["props"].get("relayoutBoundary").is_none());
    }

    // Additive: programs without boundary props keep their exact IR bytes.
    let dir = out_dir("boundary-golden");
    for (src, golden) in [
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
    ] {
        if !src.is_file() {
            continue;
        }
        let out = dir.join(format!(
            "{}.opuir.json",
            golden.file_stem().unwrap().to_string_lossy()
        ));
        let r = compile(&src, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(fs::read(&out).unwrap(), fs::read(&golden).unwrap());
        let plain: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        for n in nodes_of(&plain) {
            assert!(n["props"].get("repaintBoundary").is_none());
            assert!(n["props"].get("relayoutBoundary").is_none());
        }
    }
}

#[test]
fn grid_fill_height_is_additive_and_grid_only() {
    let dir = out_dir("grid-fill");
    let out = dir.join("fill.opuir.json");
    let r = compile(&fixture("grid_fill_height.opu"), &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let grids: Vec<&Json> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Grid")
        .collect();
    assert_eq!(grids.len(), 2);
    assert_eq!(grids[0]["props"]["fillHeight"], true);
    assert!(grids[1]["props"].get("fillHeight").is_none());

    let again = dir.join("fill-again.opuir.json");
    let r = compile(&fixture("grid_fill_height.opu"), &again);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());

    fn has_fill(v: &Json) -> bool {
        if v.get("props")
            .and_then(Json::as_object)
            .is_some_and(|p| p.contains_key("fillHeight"))
        {
            return true;
        }
        match v {
            Json::Object(o) => o.values().any(has_fill),
            Json::Array(a) => a.iter().any(has_fill),
            _ => false,
        }
    }
    let gold = out_dir("grid-fill-golden");
    let examples = workspace().join("..").join("opurom-examples");
    let cases = [
        (
            contract("counter.opu"),
            Some(fixture("golden/counter.opuir.json")),
        ),
        (
            fixture("golden/todo.opu"),
            Some(fixture("golden/todo.opuir.json")),
        ),
        (examples.join("apps/forum/forum.opu"), None),
        (examples.join("apps/teamhub/teamhub.opu"), None),
        (examples.join("apps/uicatalog/main.opu"), None),
    ];
    for (src, golden) in cases {
        if !src.is_file() {
            continue;
        }
        let compiled = gold.join("out.opuir.json");
        let r = compile(&src, &compiled);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        if let Some(golden) = golden {
            assert_eq!(
                fs::read(&compiled).unwrap(),
                fs::read(&golden).unwrap(),
                "{} changed its bytes",
                golden.display()
            );
        }
        let ir: Json = serde_json::from_slice(&fs::read(&compiled).unwrap()).unwrap();
        assert!(!has_fill(&ir), "{} gained fillHeight", src.display());
    }
}

#[test]
fn platform_calls_lower_typed_capability_nodes() {
    let dir = out_dir("platform-calls");
    let out = dir.join("calls.opuir.json");
    let r = compile(&fixture("platform_calls.opu"), &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let calls: Vec<&Json> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "PlatformCall")
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["props"]["capability"], "notification");
    assert_eq!(
        calls[0]["props"]["request"],
        json!({"id": 1, "channel": "updates", "title": "Hi", "body": "text"})
    );
    assert_eq!(calls[0]["props"]["result"], "notified");
    assert_eq!(calls[0]["props"]["error"], "err");
    assert_eq!(calls[1]["props"]["capability"], "camera");
    assert_eq!(
        calls[1]["props"]["request"],
        json!({"facing": "back", "flash": false})
    );
    assert_eq!(calls[1]["props"]["result"], "photo");
    assert_eq!(calls[2]["props"]["capability"], "location");
    assert_eq!(
        calls[2]["props"]["request"],
        json!({"accuracy": "fine", "timeout": 5000})
    );
    assert_eq!(calls[2]["props"]["result"], "place");
    for call in &calls {
        assert_eq!(call["props"]["error"], "err");
        assert_eq!(call["children"], json!([]));
        assert_eq!(call["events"], json!({}));
    }

    let again = dir.join("calls-again.opuir.json");
    let r = compile(&fixture("platform_calls.opu"), &again);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());

    // Optimizer must not prune the states bound as result/error targets.
    let opt = dir.join("calls.opt.json");
    let r = optimize(&out, &opt);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let optimized: Json = serde_json::from_slice(&fs::read(&opt).unwrap()).unwrap();
    assert_eq!(
        state_names(&optimized),
        vec!["notified", "photo", "place", "err"]
    );
}

#[test]
fn platform_calls_absent_keeps_golden_ir_byte_identical() {
    let dir = out_dir("platform-golden");
    let examples = workspace().join("..").join("opurom-examples");
    let cases = [
        (
            contract("counter.opu"),
            Some(fixture("golden/counter.opuir.json")),
        ),
        (
            fixture("golden/todo.opu"),
            Some(fixture("golden/todo.opuir.json")),
        ),
        (examples.join("apps/forum/forum.opu"), None),
        (examples.join("apps/teamhub/teamhub.opu"), None),
        (examples.join("apps/uicatalog/main.opu"), None),
    ];
    for (src, golden) in cases {
        if !src.is_file() {
            continue;
        }
        let out = dir.join("out.opuir.json");
        let r = compile(&src, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        if let Some(golden) = golden {
            assert_eq!(
                fs::read(&out).unwrap(),
                fs::read(&golden).unwrap(),
                "{} changed its bytes",
                golden.display()
            );
        }
        let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        fn has_call(v: &Json) -> bool {
            if v["kind"] == "PlatformCall" {
                return true;
            }
            match v {
                Json::Object(o) => o.values().any(has_call),
                Json::Array(a) => a.iter().any(has_call),
                _ => false,
            }
        }
        assert!(!has_call(&ir), "{} gained a PlatformCall", src.display());
    }
}

#[test]
fn extended_primitives_render_structured_mini_markup() {
    let dir = out_dir("emit-extended-mini");
    let ir = dir.join("extended.opuir.json");
    let src = dir.join("extended.opu");
    fs::write(
        &src,
        "app Ext {\n  Column(gap: 8) {\n    PieChart(value: 55, segments: 4, label: \"chart\")\n    Gauge(value: 42, min: 0, max: 100)\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let wx = out_dir("emit-extended-mini-wechat");
    let r = emit(&ir, "wechat", &wx);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxml = fs::read_to_string(wx.join("pages/index/index.wxml")).unwrap();
    // Each extended primitive keeps its kind as a class slug and a data-kind,
    // and every numeric prop survives as a data attribute.
    assert!(
        wxml.contains("opu-x-piechart"),
        "PieChart must carry its kind class"
    );
    assert!(
        wxml.contains("opu-x-gauge"),
        "Gauge must carry its kind class"
    );
    assert!(wxml.contains("data-kind=\"PieChart\""));
    assert!(wxml.contains("data-value=\"55\""));
    assert!(wxml.contains("data-segments=\"4\""));
    assert!(wxml.contains("data-kind=\"Gauge\""));
    assert!(wxml.contains("data-value=\"42\""));
    assert!(wxml.contains("data-min=\"0\""));
    assert!(wxml.contains("data-max=\"100\""));
}

#[test]
fn mini_targets_bind_gestures_with_a_deterministic_recognizer() {
    let dir = out_dir("emit-gesture-mini");
    let ir = dir.join("gesture.opuir.json");
    let src = dir.join("gesture.opu");
    fs::write(
        &src,
        "app MG {\n  state n: int = 0;\n  Column(gap: 8) {\n    Button(\"D\", gesture: doubleTap) { n += 1 }\n    Button(\"L\", gesture: longPress) { n += 5 }\n    Button(\"G\", gesture: drag) { n += 10 }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    for (target, ext, start_attr) in [
        ("wechat", "wxml", "bindtouchstart"),
        ("alipay", "axml", "onTouchStart"),
        ("qq", "qml", "bindtouchstart"),
        ("douyin", "ttml", "bindtouchstart"),
        ("baidu", "swan", "bindtouchstart"),
        ("kuaishou", "ksml", "bindtouchstart"),
    ] {
        let out = out_dir(&format!("emit-gesture-mini-{target}"));
        let r = emit(&ir, target, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let markup = fs::read_to_string(out.join(format!("pages/index/index.{ext}"))).unwrap();
        assert!(
            markup.contains(&format!("{start_attr}=\"onGestureStart\"")),
            "{target} binds touch start"
        );
        assert!(markup.contains("data-gid=\"1\""), "{target} tags node id");

        let js = fs::read_to_string(out.join("pages/index/index.js")).unwrap();
        for method in [
            "onDoubleTap1:",
            "onLongPress2:",
            "onDrag3:",
            "onGestureStart:",
            "onGestureMove:",
            "onGestureEnd:",
        ] {
            assert!(js.contains(method), "{target} generates {method}");
        }
        // The recognizer uses the event's own timeStamp, never a wall clock.
        assert!(
            !js.contains("Date.now"),
            "{target} recognizer is clock-free"
        );
        assert!(js.contains("e.timeStamp"), "{target} uses event timeStamp");
        let check = Command::new("node")
            .arg("--check")
            .arg(out.join("pages/index/index.js"))
            .output()
            .expect("run node --check");
        assert!(
            check.status.success(),
            "{target} JS invalid: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
}

#[test]
fn canvas_web_emitter_binds_gestures_with_a_clock_free_recognizer() {
    let dir = out_dir("emit-gesture-web");
    let ir = dir.join("gesture.opuir.json");
    let src = dir.join("gesture.opu");
    fs::write(
        &src,
        "app CG {\n  state n: int = 0;\n  Column(gap: 8) {\n    Button(\"D\", gesture: doubleTap) { n += 1 }\n    Button(\"L\", gesture: longPress) { n += 5 }\n    Button(\"G\", gesture: drag) { n += 10 }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = out_dir("emit-gesture-web-out");
    let r = emit(&ir, "web", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let js = fs::read_to_string(out.join("app.js")).unwrap();
    for method in [
        "async function applyDoubleTap(id)",
        "async function applyLongPress(id)",
        "async function applyDrag(id)",
        "function gestureHit(x, y)",
        "pointerdown",
        "pointermove",
        "pointerup",
    ] {
        assert!(js.contains(method), "canvas web generates {method}");
    }
    assert!(!js.contains("Date.now"), "recognizer is clock-free");
    assert!(
        js.contains("e.timeStamp"),
        "recognizer uses event timeStamp"
    );
    let check = Command::new("node")
        .arg("--check")
        .arg(out.join("app.js"))
        .output()
        .expect("run node --check");
    assert!(
        check.status.success(),
        "canvas web JS invalid: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    // A program without gestures stays byte-identical: no recognizer is emitted.
    let plain = dir.join("plain.opu");
    fs::write(
        &plain,
        "app CP {\n  state n: int = 0;\n  Button(\"T\") { n += 1 }\n}\n",
    )
    .unwrap();
    let plain_ir = dir.join("plain.opuir.json");
    let r = compile(&plain, &plain_ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_out = out_dir("emit-gesture-web-plain");
    let r = emit(&plain_ir, "web", &plain_out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let plain_js = fs::read_to_string(plain_out.join("app.js")).unwrap();
    assert!(!plain_js.contains("gestureHit"));
    assert!(!plain_js.contains("pointerdown"));
    assert!(plain_js.contains("applyTap"));
}

#[test]
fn all_targets_emit_navigation_signals() {
    let dir = out_dir("emit-nav");
    let ir = dir.join("nav.opuir.json");
    let src = dir.join("nav.opu");
    fs::write(
        &src,
        "app Nav {\n  page Home;\n  page Settings;\n  Button(\"Go\") { push: Settings }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    // native: a reserved @nav patch, no state mutation.
    let native = out_dir("emit-nav-native");
    let r = emit(&ir, "native", &native);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let main_rs = fs::read_to_string(native.join("src/main.rs")).unwrap();
    assert!(main_rs.contains("name: \"@nav\", value: \"Settings\".to_string()"));
    let (built, stderr) = build_native(&native);
    assert!(built, "native build failed:\n{stderr}");
    let (ran, stdout) = run_native(
        &native.join("target/release/opurom_native"),
        "tap 0\nquit\n",
    );
    assert!(ran, "native binary exited nonzero");
    assert!(stdout.contains("patch @nav=Settings"), "{stdout}");

    // canvas web and the six mini targets: the same reserved signal.
    let web = out_dir("emit-nav-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        app_js.contains("state[\"@nav\"] = \"Settings\";"),
        "{app_js}"
    );
    for target in ["wechat", "alipay", "qq", "douyin", "baidu", "kuaishou"] {
        let out = out_dir(&format!("emit-nav-{target}"));
        let r = emit(&ir, target, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let js = fs::read_to_string(out.join("pages/index/index.js")).unwrap();
        assert!(js.contains("\"@nav\": \"Settings\""), "{target} nav signal");
        let check = Command::new("node")
            .arg("--check")
            .arg(out.join("pages/index/index.js"))
            .output()
            .expect("run node --check");
        assert!(check.status.success(), "{target} JS invalid");
    }

    // wasm: the host mirror records the signal and the tap returns one patch.
    let wasm = out_dir("emit-nav-wasm");
    let r = emit(&ir, "wasm", &wasm);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let host = fs::read_to_string(wasm.join("host.js")).unwrap();
    assert!(host.contains("{ op: \"push\", target: \"Settings\" }"));
    assert!(host.contains("mirror[\"@nav\"] = target;"));
    let lib = fs::read_to_string(wasm.join("src/lib.rs")).unwrap();
    assert!(lib.contains("fn tap_0() -> u32"));
    let build = Command::new(cargo_path())
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&wasm)
        .env("CARGO_TARGET_DIR", wasm.join("target"))
        .env(
            "PATH",
            format!(
                "{}/.cargo/bin:{}",
                std::env::var("HOME").unwrap_or_default(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("run cargo build --release --target wasm32-unknown-unknown");
    assert!(
        build.status.success(),
        "wasm build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let artifact = wasm.join("target/wasm32-unknown-unknown/release/opurom_wasm.wasm");
    assert!(artifact.is_file(), "no .wasm artifact");
    let script = dir.join("nav.mjs");
    fs::write(
        &script,
        "import { readFile } from \"node:fs/promises\";\nconst bytes = await readFile(process.argv[2]);\nconst m = await WebAssembly.instantiate(bytes, {});\nconst e = m.instance.exports;\nconsole.log(JSON.stringify({ tap: e.opu_tap(0), unknown: e.opu_tap(9999) }));\n",
    )
    .unwrap();
    let run = Command::new("node")
        .arg(&script)
        .arg(&artifact)
        .output()
        .expect("run node nav conformance");
    assert!(
        run.status.success(),
        "nav conformance failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8_lossy(&run.stdout);
    let got: serde_json::Value = serde_json::from_str(text.trim()).expect("parse nav JSON");
    assert_eq!(got["tap"], 1, "{text}");
    assert_eq!(got["unknown"], 0, "{text}");
}

#[test]
fn effects_and_animation_stay_absent_without_the_props() {
    // A source with no visual-effect or entry-animation props must not gain
    // effect drawing, keyframes or an animation class; `animate: none` is
    // static too. This guards the no-effect emission path from regressions.
    let dir = out_dir("emit-no-effects");
    let ir = dir.join("plain.opuir.json");
    let src = dir.join("plain.opu");
    fs::write(
        &src,
        "app Plain {\n  Column(gap: 8) {\n    Container(animate: none)\n    Text(\"hello\")\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let web = out_dir("emit-no-effects-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(!app_js.contains("createLinearGradient"));
    assert!(!app_js.contains("shadowColor"));
    let web_html = fs::read_to_string(web.join("index.html")).unwrap();
    assert!(
        !web_html.contains("opurom-entry"),
        "no curve must stay static"
    );
    assert!(!web_html.contains("opu-anim"), "no curve must stay static");

    let wx = out_dir("emit-no-effects-wechat");
    let r = emit(&ir, "wechat", &wx);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxss = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(!wxss.contains("linear-gradient"));
    assert!(!wxss.contains("box-shadow"));
    assert!(!wxss.contains("backdrop-filter"));
    let wxml = fs::read_to_string(wx.join("pages/index/index.wxml")).unwrap();
    assert!(
        !wxml.contains("anim-entry"),
        "animate:none must not animate"
    );
}

#[test]
fn canvas_and_mini_emitters_apply_high_contrast() {
    // `highContrast` must reach the canvas web target and the mini targets the
    // way the native renderer applies it: a 2px contrast outline on a filled
    // node and a contrast text colour. A node without the prop is unchanged.
    let dir = out_dir("emit-hc");
    let ir = dir.join("hc.opuir.json");
    let src = dir.join("hc.opu");
    fs::write(
        &src,
        "app HC {\n  Column {\n    Container(bg: \"#112233\", highContrast: true) {\n      \
         Text(\"Dark\", highContrast: true)\n    }\n    Container(bg: \"#EEEEEE\", highContrast: true) {\n      \
         Text(\"Light\", highContrast: true)\n    }\n    Container(bg: \"#112233\") {\n      Text(\"Normal\")\n    }\n  }\n}\n",
    )
    .unwrap();
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    // Canvas web: a 2px white outline on the dark fill, a 2px black outline on
    // the light fill, and no extra outline for the node without the prop.
    let web = out_dir("emit-hc-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        app_js.contains("ctx.strokeStyle = \"#ffffff\";\n  ctx.lineWidth = 2;"),
        "canvas web: dark fill needs a 2px white outline: {app_js}"
    );
    assert!(
        app_js.contains("ctx.strokeStyle = \"#000000\";\n  ctx.lineWidth = 2;"),
        "canvas web: light fill needs a 2px black outline: {app_js}"
    );
    // The dark Text has no own fill, so its contrast colour is black on the
    // page surface; either way the text branch uses a contrast colour.
    assert!(
        app_js.contains("ctx.fillStyle = \"#000000\";\n  ctx.fillText(\"Dark\""),
        "canvas web: high-contrast text must use a contrast colour: {app_js}"
    );
    assert_eq!(
        app_js.matches("ctx.lineWidth = 2;").count(),
        2,
        "only the two high-contrast containers get a 2px outline: {app_js}"
    );

    // Mini target: the same mapping as CSS classes.
    let wx = out_dir("emit-hc-wechat");
    let r = emit(&ir, "wechat", &wx);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let wxss = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(
        wxss.contains("border:2px solid #ffffff"),
        "mini: dark fill needs a white border: {wxss}"
    );
    assert!(
        wxss.contains("border:2px solid #000000"),
        "mini: light fill needs a black border: {wxss}"
    );
    assert_eq!(
        wxss.matches("border:2px solid").count(),
        2,
        "only the two high-contrast containers get a border: {wxss}"
    );
}

#[test]
fn web_layout_places_children_of_every_container_kind() {
    // Self-contained: CI clones only the sibling repos a check needs.
    let src = out_dir("containers").join("containers.opu");
    fs::write(
        &src,
        concat!(
            "app Containers {\n",
            "  Scroll(height: 100) {\n",
            "    Column {\n",
            "      Grid(columns: 2) { Text(\"cell\") }\n",
            "      Card(p: 8) { Text(\"card\") }\n",
            "      Dialog(title: \"Hi\") { Text(\"body\") }\n",
            "      List { Text(\"row\") }\n",
            "      Container(p: 4) { Text(\"box\") }\n",
            "      Gauge(label: \"data/Gauge\")\n",
            "    }\n",
            "  }\n",
            "}\n",
        ),
    )
    .unwrap();
    let ir = out_dir("containers-ir").join("containers.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("containers-web");
    let r = emit(&ir, "web", &dir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(dir.join("app.js")).unwrap();
    let draws: Vec<&str> = app_js
        .lines()
        .filter(|l| l.contains("ctx.fillText(") || l.contains("ctx.fillRect("))
        .collect();
    assert!(draws.len() >= 7, "expected every child to be drawn");
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
    let r = compile(&contract("counter.opu"), &ir);
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

    let r = emit(&contract("counter.opuir.json"), "bogus", &dir.join("out2"));
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown target 'bogus'"), "{err}");
    assert!(err.contains("  help: "), "{err}");
}

#[test]
fn web_dom_target_emits_responsive_dom_page() {
    let emitter = workspace()
        .join("..")
        .join("opurom-web")
        .join("bin")
        .join("opurom-web.mjs");
    let ir = out_dir("web-dom-ir").join("counter.opuir.json");
    let r = compile(&contract("counter.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let dir = out_dir("web-dom");
    let r = emit(&ir, "web-dom", &dir);
    if !emitter.is_file() {
        // standalone checkout: the sibling web repo is optional
        assert_eq!(r.status.code(), Some(1));
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(err.contains("responsive DOM emitter"), "{err}");
        assert!(err.contains("  help: "), "{err}");
        return;
    }
    assert!(
        r.status.success(),
        "web-dom emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let html = fs::read_to_string(dir.join("index.html")).expect("responsive DOM page");
    assert!(
        !html.contains("width=\"800\""),
        "fixed canvas width survived:\n{html}"
    );
    assert!(
        !html.contains("height=\"600\""),
        "fixed canvas height survived:\n{html}"
    );
    assert!(
        html.contains("viewport"),
        "missing responsive viewport meta:\n{html}"
    );
    assert!(
        html.contains("<main id=\"app\">"),
        "missing responsive app container:\n{html}"
    );
    assert!(
        html.contains("display: flex"),
        "missing flex layout:\n{html}"
    );
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
fn native_emitter_exposes_ir_override_points() {
    // The generated runner keeps the embedded IR as the default and exposes
    // two override points for hot reload: the Opurom_IR_PATH startup swap
    // and the `reload` stdin command.
    let dir = out_dir("native-reload-src");
    let ir = dir.join("app.opuir.json");
    let r = compile(&fixture("features.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("native");
    let r = emit(&ir, "native", &out);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let src = fs::read_to_string(out.join("src/main.rs")).unwrap();
    assert!(src.contains("Opurom_IR_PATH"), "{src}");
    assert!(src.contains("\"reload\" =>"), "{src}");
    assert!(src.contains("render_frame(&state, &ir_text,"), "{src}");
}

#[test]
fn native_emitter_reloads_ir_from_stdin_and_env() {
    let dir = out_dir("native-reload");
    let ir = dir.join("app.opuir.json");
    let r = compile(&fixture("features.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("native");
    let r = emit(&ir, "native", &out);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let (built, stderr) = build_native(&out);
    assert!(built, "cargo build --release failed:\n{stderr}");
    let exe = out.join("target/release/opurom_native");

    // An alternate tree with a different state shape, to prove the swap.
    let alt_src = dir.join("alt.opu");
    fs::write(
        &alt_src,
        "app NR {\n  state n: int = 0;\n  Column { Button(\"Add\") { n += 1 } }\n}\n",
    )
    .unwrap();
    let alt = dir.join("alt.opuir.json");
    let r = compile(&alt_src, &alt);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let alt_path = alt.to_string_lossy().to_string();

    // A missing file keeps the embedded tree and reports the failure.
    let (ran, stdout) = run_native(&exe, "reload /nonexistent/opuir.json\nstate\nquit\n");
    assert!(ran, "native binary exited nonzero");
    assert!(stdout.contains("error: cannot reload from"), "{stdout}");
    assert!(
        stdout.contains("state count=0 flag=false name=world"),
        "{stdout}"
    );

    // Invalid IR content is rejected and the embedded tree survives.
    let bad = dir.join("bad.opuir.json");
    fs::write(&bad, "{ not json").unwrap();
    let bad_path = bad.to_string_lossy().to_string();
    let (ran, stdout) = run_native(&exe, &format!("reload {bad_path}\nstate\nquit\n"));
    assert!(ran, "native binary exited nonzero");
    assert!(stdout.contains("error: invalid IR in"), "{stdout}");
    assert!(
        stdout.contains("state count=0 flag=false name=world"),
        "{stdout}"
    );

    // A valid reload swaps the rendered tree: a second frame comes from the
    // alternate IR (its a11y sidecar is far smaller), while the compiled
    // state and handlers stay untouched by design.
    let (ran, stdout) = run_native(&exe, &format!("reload {alt_path}\nstate\nquit\n"));
    assert!(ran, "native binary exited nonzero");
    assert_eq!(stdout.matches("frame:").count(), 2, "{stdout}");
    assert!(stdout.contains("a11y: 1074 bytes"), "{stdout}");
    assert!(
        stdout.contains("a11y: 157 bytes"),
        "second render must come from the reloaded tree: {stdout}"
    );
    assert!(
        stdout.contains("state count=0 flag=false name=world"),
        "state must survive the reload: {stdout}"
    );

    // Opurom_IR_PATH swaps the tree at startup the same way.
    let mut child = Command::new(&exe)
        .env("Opurom_IR_PATH", &alt_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn generated native binary");
    {
        let mut pipe = child.stdin.take().expect("stdin pipe");
        pipe.write_all(b"quit\n").expect("write stdin");
    }
    let o = child.wait_with_output().expect("wait for native binary");
    assert!(o.status.success(), "native binary exited nonzero");
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("frame:"), "{stdout}");
}

#[test]
fn native_emitter_dispatches_gesture_commands() {
    let dir = out_dir("native-gesture");
    let src = dir.join("gesture.opu");
    fs::write(
        &src,
        "app NG {\n  state n: int = 0;\n  Column(gap: 8) {\n    Button(\"D\", gesture: doubleTap) { n += 1 }\n    Button(\"L\", gesture: longPress) { n += 5 }\n    Button(\"G\", gesture: drag) { n += 10 }\n    Button(\"T\") { n += 100 }\n  }\n}\n",
    )
    .unwrap();
    let ir = dir.join("gesture.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("native");
    let r = emit(&ir, "native", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let (built, stderr) = build_native(&out);
    assert!(built, "cargo build --release failed:\n{stderr}");

    // Each gesture command fires its node's handler chain: doubleTap +1,
    // longPress +5, drag +10, tap +100, in that order, from state 0.
    let (ran, stdout) = run_native(
        &out.join("target/release/opurom_native"),
        "state\ndoubleTap 1\nstate\nlongPress 2\nstate\ndrag 3\nstate\ntap 4\nstate\nquit\n",
    );
    assert!(ran, "native binary exited nonzero");
    assert!(stdout.contains("state n=0"), "{stdout}");
    assert!(stdout.contains("state n=1"), "{stdout}");
    assert!(stdout.contains("state n=6"), "{stdout}");
    assert!(stdout.contains("state n=16"), "{stdout}");
    assert!(stdout.contains("state n=116"), "{stdout}");
    assert!(stdout.contains("patch n=116"), "{stdout}");
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

/// First node id in pre-order whose `tap` action adds a positive integer to a
/// state, so the frame must actually change when it is tapped.
fn first_positive_add_tap_id(value: &Json) -> Option<u32> {
    if let Some(tap) = value.get("events").and_then(|e| e.get("tap")) {
        if let Some(id) = value.get("id").and_then(Json::as_u64) {
            if tap.get("op").and_then(Json::as_str) == Some("add") {
                if tap.get("value").and_then(Json::as_i64).unwrap_or(0) > 0 {
                    return Some(id as u32);
                }
            }
        }
    }
    for child in value.get("children").and_then(Json::as_array)?.iter() {
        if let Some(id) = first_positive_add_tap_id(child) {
            return Some(id);
        }
    }
    None
}

#[test]
fn native_emitter_renders_and_writes_a_frame() {
    let ir = out_dir("native-frame-ir").join("counter.opuir.json");
    let r = compile(&contract("counter.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let dir = out_dir("native-frame");
    let r = emit(&ir, "native", &dir);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    // The generated app renders the embedded IR and writes the frame. It must
    // depend only on opurom-render so an offline build resolves without a
    // registry (opurom-platform pulls the `png` crate).
    let native_rs = fs::read_to_string(dir.join("src/main.rs")).unwrap();
    let native_toml = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(
        !native_toml.contains("opurom-platform"),
        "native manifest must not depend on opurom-platform"
    );
    for marker in [
        "opurom_render::Renderer",
        "render_frame(&state, &ir_text,",
        "write_ppm",
    ] {
        assert!(native_rs.contains(marker), "native source missing {marker}");
    }

    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );

    let exe = dir.join("target/release/opurom_native");
    let initial_frame = dir.join("initial-frame.ppm");
    let tapped_frame = dir.join("tapped-frame.ppm");
    let run_with = |frame: &Path, stdin: &str| {
        let mut child = Command::new(&exe)
            .arg(frame.to_str().expect("utf8 frame path"))
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
        assert!(
            out.status.success(),
            "native frame render failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            fs::read(frame).expect("read rendered frame"),
        )
    };
    let (stdout, bytes) = run_with(&initial_frame, "quit\n");
    assert!(stdout.contains("frame: 800x600 wrote"), "{stdout}");
    assert!(
        stdout.contains("a11y: "),
        "a11y sidecar line missing: {stdout}"
    );
    assert!(bytes.len() > 1000, "frame too small: {} bytes", bytes.len());
    let header = String::from_utf8_lossy(&bytes[..bytes.len().min(20)]);
    assert!(
        header.starts_with("P6\n800 600\n255\n"),
        "unexpected PPM header: {header}"
    );
    // A program with no explicit roles still produces a valid a11y tree.
    let initial_a11y: Json = serde_json::from_str(
        &fs::read_to_string(dir.join("initial-frame.ppm.a11y.json"))
            .expect("read initial a11y sidecar"),
    )
    .expect("a11y sidecar is valid JSON");
    assert!(initial_a11y.is_array(), "a11y tree must be an array");

    // A tap changes state, so the app must re-render and rewrite the frame.
    let ir_json: Json =
        serde_json::from_str(&fs::read_to_string(&ir).expect("read IR")).expect("IR is valid JSON");
    let tap_id = first_positive_add_tap_id(ir_json.get("tree").expect("IR tree"))
        .expect("counter has a positive add tap");
    let (tap_stdout, tap_bytes) = run_with(&tapped_frame, &format!("tap {tap_id}\nquit\n"));
    assert!(
        tap_stdout.contains("state count=1"),
        "the tap did not change state: {tap_stdout}"
    );
    assert!(
        tap_stdout.contains("frame: 800x600 wrote"),
        "the tap did not re-render: {tap_stdout}"
    );
    assert!(
        tap_stdout.contains("a11y: "),
        "the tap did not rewrite the a11y tree: {tap_stdout}"
    );
    assert!(
        dir.join("tapped-frame.ppm.a11y.json").is_file(),
        "tap must refresh the a11y sidecar"
    );
    assert_ne!(
        bytes, tap_bytes,
        "the frame was not rewritten after the tap"
    );
}

#[test]
fn native_emitter_writes_a11y_tree_with_roles() {
    // The native target must expose explicit roles from the renderer's a11y
    // tree in a sidecar next to the frame, and refresh it on every re-render.
    let dir = out_dir("native-a11y");
    let src = dir.join("roles.opu");
    fs::write(
        &src,
        "app Roles {\n  state count = 0\n  Column {\n    Text(\"Title\", role: \"heading\")\n    \
         Button(\"Add\", role: \"button\") { count += 1 }\n    Text(\"plain\")\n  }\n}\n",
    )
    .unwrap();
    let ir = dir.join("roles.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("native");
    let r = emit(&ir, "native", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let native_rs = fs::read_to_string(out.join("src/main.rs")).unwrap();
    assert!(
        native_rs.contains("opurom_render::a11y_json"),
        "generated app must call the renderer a11y API"
    );
    let (built, stderr) = build_native(&out);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );

    let exe = out.join("target/release/opurom_native");
    let frame = out.join("frame.ppm");
    let sidecar = out.join("frame.ppm.a11y.json");
    let run = |stdin: &str| {
        let mut child = Command::new(&exe)
            .arg(frame.to_str().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn generated native binary");
        {
            let mut pipe = child.stdin.take().unwrap();
            pipe.write_all(stdin.as_bytes()).unwrap();
        }
        let o = child.wait_with_output().expect("wait for native binary");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    };

    let ir_json: Json =
        serde_json::from_str(&fs::read_to_string(&ir).unwrap()).expect("IR is valid JSON");
    let tap_id = first_positive_add_tap_id(ir_json.get("tree").expect("IR tree"))
        .expect("fixture has a positive add tap");

    let stdout = run("quit\n");
    assert!(stdout.contains("frame: 800x600 wrote"), "{stdout}");
    assert!(stdout.contains("a11y: "), "missing a11y line: {stdout}");
    assert!(frame.is_file(), "frame file missing");
    let tree: Json =
        serde_json::from_str(&fs::read_to_string(&sidecar).expect("read a11y sidecar"))
            .expect("a11y sidecar is valid JSON");
    let roles = tree.as_array().expect("a11y tree is an array");
    assert!(!roles.is_empty(), "a11y tree must contain nodes");
    assert!(
        roles.iter().any(|n| n["role"] == "heading"),
        "explicit heading role missing from a11y tree: {tree}"
    );
    assert!(
        roles.iter().any(|n| n["role"] == "button"),
        "explicit button role missing from a11y tree: {tree}"
    );

    // A tap re-renders, so the sidecar is rewritten (delete it first to prove
    // the refresh actually happened rather than reading a stale file).
    fs::remove_file(&sidecar).unwrap();
    let tap_stdout = run(&format!("tap {tap_id}\nquit\n"));
    assert!(tap_stdout.contains("a11y: "), "{tap_stdout}");
    let refreshed: Json =
        serde_json::from_str(&fs::read_to_string(&sidecar).expect("read refreshed a11y sidecar"))
            .expect("refreshed a11y sidecar is valid JSON");
    assert!(refreshed.is_array(), "refreshed a11y tree must be an array");
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

fn compile_nodes_folded(src: &Path, out: &Path) -> Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .arg("--fold-nodes")
        .output()
        .expect("run opuromc compile --fold-nodes")
}

fn compile_folded(src: &Path, out: &Path) -> Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .arg("--fold-constants")
        .output()
        .expect("run opuromc compile --fold-constants")
}

fn optimize_folded(ir: &Path, out: &Path) -> Output {
    bin()
        .arg("optimize")
        .arg(ir)
        .arg("-o")
        .arg(out)
        .arg("--fold-constants")
        .output()
        .expect("run opuromc optimize --fold-constants")
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
        "explicit prop must beat the #097FEB default"
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

    let plain = compile_ok(&contract("counter.opu"));
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
fn icon_names_are_validated_at_compile_time() {
    let dir = out_dir("icon");
    let good = dir.join("good.opu");
    fs::write(
        &good,
        "app Icons {\n  Column {\n    Icon(name: \"star\")\n    SVGIcon(name: \"search\")\n\
         MaterialIcon(name: \"home\")\n  }\n}\n",
    )
    .expect("write icon fixture");
    let ir = compile_ok(&good);
    let icons: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Icon")
        .filter_map(|n| n["props"]["name"].as_str())
        .collect();
    assert_eq!(
        icons,
        vec!["star", "search", "home"],
        "canonical names must lower"
    );

    let bad = dir.join("bad.opu");
    fs::write(&bad, "app Bad {\n  Column { Icon(name: \"nope\") }\n}\n")
        .expect("write bad icon fixture");
    let out = dir.join("bad.opuir.json");
    let r = compile(&bad, &out);
    assert!(!r.status.success(), "an unknown icon must be rejected");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert_diag(&stderr, &bad);
    assert!(
        stderr.contains("unknown icon name 'nope'"),
        "diagnostic must name the bad icon: {stderr}"
    );
    assert!(
        stderr.contains("write a built-in icon name such as 'home'"),
        "diagnostic must suggest a valid name: {stderr}"
    );
    assert!(
        stderr.contains("the set has 31 names"),
        "diagnostic must state the canonical set size: {stderr}"
    );
    assert!(
        stderr.contains("Icon"),
        "diagnostic must name the node kind: {stderr}"
    );
}

#[test]
fn components_lists_260_rows() {
    let r = bin().arg("components").output().expect("run components");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = String::from_utf8_lossy(&r.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let last = *lines.last().expect("count line");
    assert_eq!(last, "260 components", "final count line: {last}");
    let count: usize = last.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(count, 260, "catalog count: {count}");
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
    for name in MOBILE_ALIAS_NAMES {
        assert!(
            lines.iter().any(|l| l.starts_with(&format!("{name} | "))),
            "missing mobile-alias catalog row {name}"
        );
    }
    for name in [
        "Modal",
        "Confirm",
        "Drawer",
        "Toast",
        "Tooltip",
        "Notification",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(&format!("{name} | "))),
            "missing feedback family catalog row {name}"
        );
    }
    for name in [
        "Badge",
        "Tag",
        "ProgressBar",
        "ProgressRing",
        "Spinner",
        "Skeleton",
        "Avatar",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(&format!("{name} | "))),
            "missing display family catalog row {name}"
        );
    }
    for &(name, _, _) in FEEDBACK_DISPLAY_COMPONENTS {
        assert!(
            lines.iter().any(|l| l.starts_with(&format!("{name} | "))),
            "missing new catalog row {name}"
        );
    }
}

#[test]
fn vendored_catalog_matches_spec_when_present() {
    let upstream = contract("components.json");
    if !upstream.is_file() {
        return; // standalone checkout: the sibling contract repo is optional
    }
    let vendored_bytes = fs::read(workspace().join("src/catalog.json")).unwrap();
    let expected_bytes = fs::read(&upstream).unwrap();
    assert_eq!(
        vendored_bytes, expected_bytes,
        "vendored catalog must be byte-identical to the upstream catalog"
    );
    let vendored: Json = serde_json::from_slice(&vendored_bytes).unwrap();
    let expected: Json = serde_json::from_slice(&expected_bytes).unwrap();
    assert_eq!(
        vendored, expected,
        "vendored catalog drifted from the upstream catalog"
    );
    assert_eq!(vendored["count"], 260);
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
    let golden = contract("counter.opuir.json");
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
fn constant_folding_keeps_golden_ir_byte_identical() {
    // Goldens carry no computed declarations, so the explicit fold flag must be
    // a no-op on both compile and optimize paths.
    let dir = out_dir("fold-golden");
    let pairs = [
        (contract("counter.opu"), contract("counter.opuir.json")),
        (
            fixture("golden/gallery.opu"),
            fixture("golden/gallery.opuir.json"),
        ),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
    ];
    for (i, (src, golden)) in pairs.iter().enumerate() {
        let plain = dir.join(format!("{i}-plain.json"));
        let folded = dir.join(format!("{i}-folded.json"));
        assert!(compile(src, &plain).status.success());
        let r = compile_folded(src, &folded);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(fs::read(&folded).unwrap(), fs::read(&plain).unwrap());
        assert_eq!(fs::read(&folded).unwrap(), fs::read(golden).unwrap());

        let opt_plain = dir.join(format!("{i}-opt.json"));
        let opt_folded = dir.join(format!("{i}-opt-folded.json"));
        assert!(optimize(&plain, &opt_plain).status.success());
        assert!(optimize_folded(&plain, &opt_folded).status.success());
        assert_eq!(
            fs::read(&opt_plain).unwrap(),
            fs::read(&opt_folded).unwrap()
        );
    }
}

#[test]
fn constant_folding_emits_literal_target_code() {
    let dir = out_dir("fold-targets");
    let src = dir.join("fold.opu");
    fs::write(
        &src,
        "app Fold {\n  computed total = 2 * 3 + 4\n  Column {\n    Text(\"{total}\")\n  }\n}\n",
    )
    .unwrap();
    let plain = dir.join("plain.opuir.json");
    let folded = dir.join("folded.opuir.json");
    assert!(compile_raw(&src, &plain).status.success());
    let r = compile_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let a: Json = serde_json::from_slice(&fs::read(&plain).unwrap()).unwrap();
    let b: Json = serde_json::from_slice(&fs::read(&folded).unwrap()).unwrap();
    assert_eq!(a["computed"][0]["expr"], "2 * 3 + 4");
    assert_eq!(b["computed"][0]["expr"], "10");
    assert_eq!(a["state"], b["state"], "the init value is already folded");
    // The default path also inlines the constant interpolation, so the tree's
    // text changes with the computed expr; everything else stays equal.
    assert_eq!(
        b["tree"]["children"][0]["props"]["value"], "10",
        "constant text is inlined on the default path"
    );
    let mut a_without = a.clone();
    let mut b_without = b.clone();
    a_without["computed"][0]
        .as_object_mut()
        .unwrap()
        .remove("expr");
    b_without["computed"][0]
        .as_object_mut()
        .unwrap()
        .remove("expr");
    a_without["tree"]["children"][0]["props"]
        .as_object_mut()
        .unwrap()
        .remove("value");
    b_without["tree"]["children"][0]["props"]
        .as_object_mut()
        .unwrap()
        .remove("value");
    assert_eq!(a_without, b_without, "only folded text and expr change");

    let native_plain = dir.join("native-plain");
    let native_folded = dir.join("native-folded");
    assert!(emit(&plain, "native", &native_plain).status.success());
    assert!(emit(&folded, "native", &native_folded).status.success());
    let before = fs::read_to_string(native_plain.join("src/main.rs")).unwrap();
    let after = fs::read_to_string(native_folded.join("src/main.rs")).unwrap();
    assert!(before.contains("saturating_mul"), "{before}");
    assert!(after.contains("_state.total = 10i64;"), "{after}");
    assert!(!after.contains("saturating_mul"), "{after}");

    let web_plain = dir.join("web-plain");
    let web_folded = dir.join("web-folded");
    assert!(emit(&plain, "web", &web_plain).status.success());
    assert!(emit(&folded, "web", &web_folded).status.success());
    let before = fs::read_to_string(web_plain.join("app.js")).unwrap();
    let after = fs::read_to_string(web_folded.join("app.js")).unwrap();
    assert!(before.contains("state.total = ((2 * 3) + 4);"), "{before}");
    assert!(after.contains("state.total = 10;"), "{after}");
}

#[test]
fn constant_folding_leaves_state_expressions() {
    let dir = out_dir("fold-state-expr");
    let src = dir.join("state.opu");
    fs::write(
        &src,
        "app Keep {\n  state count = 2\n  computed doubled = count * 2\n  Column {\n    Text(\"{doubled}\")\n  }\n}\n",
    )
    .unwrap();
    let out = dir.join("out.opuir.json");
    let r = compile_folded(&src, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(ir["computed"][0]["expr"], "count * 2");
}

#[test]
fn inline_constant_text_keeps_ids_and_matches_folded_fixture() {
    let dir = out_dir("fold-nodes-text");
    let src = fixture("folded/text.opu");
    let plain = dir.join("plain.opuir.json");
    let folded = dir.join("folded.opuir.json");
    assert!(compile_raw(&src, &plain).status.success());
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let a: Json = serde_json::from_slice(&fs::read(&plain).unwrap()).unwrap();
    let b: Json = serde_json::from_slice(&fs::read(&folded).unwrap()).unwrap();
    assert_eq!(a["tree"]["children"][0]["props"]["value"], "{label}");
    assert_eq!(b["tree"]["children"][0]["props"]["value"], "ready");
    assert_eq!(
        a["tree"]["children"][1]["props"]["value"], "{count}",
        "a written state must stay interpolated"
    );
    assert_eq!(b["tree"]["children"][1]["props"]["value"], "{count}");
    let ids = |ir: &Json| -> Vec<u64> {
        nodes_of(ir)
            .into_iter()
            .map(|n| n["id"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(ids(&a), ids(&b), "text inlining must not change ids");
    assert_eq!(
        fs::read(&folded).unwrap(),
        fs::read(fixture("folded/text.opuir.json")).unwrap(),
        "folded output must match the committed fixture"
    );
    assert!(
        !folded.with_extension("idmap.json").exists(),
        "text inlining changes no id, so it writes no sidecar"
    );

    // Render parity: the folded tree must produce the same frame bytes.
    let (ok_plain, out_plain) = render_ir(&plain, &dir.join("plain.ppm"));
    let (ok_folded, out_folded) = render_ir(&folded, &dir.join("folded.ppm"));
    assert!(ok_plain, "{out_plain}");
    assert!(ok_folded, "{out_folded}");
    assert_eq!(
        fs::read(dir.join("plain.ppm")).unwrap(),
        fs::read(dir.join("folded.ppm")).unwrap(),
        "folded text must render identically"
    );
}

#[test]
fn fold_node_visibility_renumbers_and_matches_fixture() {
    let dir = out_dir("fold-nodes-vis");
    let src = fixture("folded/visibility.opu");
    let plain = dir.join("plain.opuir.json");
    let folded = dir.join("folded.opuir.json");
    assert!(compile_raw(&src, &plain).status.success());
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let a: Json = serde_json::from_slice(&fs::read(&plain).unwrap()).unwrap();
    let b: Json = serde_json::from_slice(&fs::read(&folded).unwrap()).unwrap();
    assert_eq!(a["tree"]["children"][2]["when"], "banner");
    assert_eq!(b["tree"]["children"].as_array().unwrap().len(), 3);
    for node in nodes_of(&b) {
        assert!(node.get("when").is_none(), "constant when must be resolved");
    }
    let ids: Vec<u64> = nodes_of(&b)
        .into_iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1, 2, 3], "dense pre-order after renumber");
    assert_eq!(
        fs::read(&folded).unwrap(),
        fs::read(fixture("folded/visibility.opuir.json")).unwrap(),
        "folded output must match the committed fixture"
    );

    // The sidecar is a build artifact, not IR: old id 4 maps to 3, the deleted
    // id 3 is absent, and the folded IR itself carries no map.
    let sidecar = folded.with_extension("idmap.json");
    let map: Json = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(map["4"], "3");
    assert!(map.get("3").is_none(), "{map}");
    assert!(b.get("nodeBoundaries").is_none());
    assert!(b.get("stateRefs").is_none());

    // Render's real parser accepts the renumbered tree and paints the same frame.
    let (ok_plain, out_plain) = render_ir(&plain, &dir.join("plain.ppm"));
    let (ok_folded, out_folded) = render_ir(&folded, &dir.join("folded.ppm"));
    assert!(ok_plain, "{out_plain}");
    assert!(ok_folded, "{out_folded}");
    assert_eq!(
        fs::read(dir.join("plain.ppm")).unwrap(),
        fs::read(dir.join("folded.ppm")).unwrap(),
        "folded visibility must render identically"
    );

    let again = dir.join("again.opuir.json");
    let r = compile_nodes_folded(&src, &again);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(fs::read(&folded).unwrap(), fs::read(&again).unwrap());
}

#[test]
fn fold_node_visibility_keeps_ancestors_of_a_removed_branch() {
    let dir = out_dir("fold-nodes-ancestors");
    let src = dir.join("nested.opu");
    fs::write(
        &src,
        "app Nested {\n  state off = false\n  Container {\n    Column {\n      if (off) {\n        Text(\"gone\")\n      }\n      Text(\"keep\")\n    }\n  }\n}\n",
    )
    .unwrap();
    let folded = dir.join("folded.opuir.json");
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&folded).unwrap()).unwrap();
    let ids: Vec<u64> = nodes_of(&ir)
        .into_iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1, 2], "only the hidden leaf is removed");
    assert_eq!(ir["tree"]["kind"], "Container");
    assert_eq!(ir["tree"]["children"][0]["kind"], "Column");
    assert_eq!(
        ir["tree"]["children"][0]["children"][0]["props"]["value"],
        "keep"
    );
}

#[test]
fn folded_ir_renders_without_the_sidecar() {
    // The id map is a write-only migration artifact: deleting it must leave the
    // folded IR fully renderable and frame-identical.
    let dir = out_dir("fold-nodes-sidecar-free");
    let src = fixture("folded/visibility.opu");
    let plain = dir.join("plain.opuir.json");
    let folded = dir.join("folded.opuir.json");
    assert!(compile(&src, &plain).status.success());
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let sidecar = folded.with_extension("idmap.json");
    assert!(sidecar.is_file());
    fs::remove_file(&sidecar).unwrap();
    assert!(!sidecar.exists());

    let (ok_plain, out_plain) = render_ir(&plain, &dir.join("plain.ppm"));
    let (ok_folded, out_folded) = render_ir(&folded, &dir.join("folded.ppm"));
    assert!(ok_plain, "{out_plain}");
    assert!(ok_folded, "{out_folded}");
    assert_eq!(
        fs::read(dir.join("plain.ppm")).unwrap(),
        fs::read(dir.join("folded.ppm")).unwrap()
    );
    assert!(
        !sidecar.exists(),
        "render must not recreate or read the sidecar"
    );
}

#[test]
fn fold_node_visibility_keeps_native_patches_equal_via_id_map() {
    let dir = out_dir("fold-nodes-native");
    let src = fixture("folded/visibility.opu");
    let plain_ir = dir.join("plain.opuir.json");
    let folded_ir = dir.join("folded.opuir.json");
    assert!(compile_raw(&src, &plain_ir).status.success());
    let r = compile_nodes_folded(&src, &folded_ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let map: Json =
        serde_json::from_slice(&fs::read(folded_ir.with_extension("idmap.json")).unwrap()).unwrap();

    let build = |ir: &Path, name: &str| -> PathBuf {
        let out = dir.join(name);
        assert!(emit(ir, "native", &out).status.success());
        let (ok, log) = build_native(&out);
        assert!(ok, "native build failed: {log}");
        out.join("target/release/opurom_native")
    };
    let plain_exe = build(&plain_ir, "native-plain");
    let folded_exe = build(&folded_ir, "native-folded");

    // Plain ids come from the plain tree; the sidecar is the only migration path.
    let plain_tap = format!("tap {}\nquit\n", plain_ir_id(&plain_ir, "Button"));
    let folded_id = map[plain_ir_id(&plain_ir, "Button").to_string().as_str()]
        .as_str()
        .unwrap()
        .to_string();
    let folded_tap = format!("tap {folded_id}\nquit\n");
    let (ok_plain, plain_stdout) = run_native(&plain_exe, &plain_tap);
    let (ok_folded, folded_stdout) = run_native(&folded_exe, &folded_tap);
    assert!(ok_plain && ok_folded, "{plain_stdout}\n{folded_stdout}");
    let patches = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("patch ") || l.starts_with("state "))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(patches(&plain_stdout), patches(&folded_stdout));
}

fn plain_ir_id(ir: &Path, kind: &str) -> u64 {
    let doc: Json = serde_json::from_slice(&fs::read(ir).unwrap()).unwrap();
    nodes_of(&doc)
        .into_iter()
        .find(|n| n["kind"] == kind)
        .and_then(|n| n["id"].as_u64())
        .expect("node kind present")
}

#[test]
fn fold_node_visibility_rebuilds_id_keyed_maps() {
    let dir = out_dir("fold-nodes-maps");
    let src = dir.join("maps.opu");
    fs::write(
        &src,
        "app FoldMaps {\n  state debug = false\n  Column {\n    Text(\"a\")\n    if (debug) {\n      Text(\"b\")\n    }\n    Text(\"c\", repaintBoundary: true)\n  }\n}\n",
    )
    .unwrap();
    let folded = dir.join("folded.opuir.json");
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let optimized = dir.join("optimized.opuir.json");
    let r = optimize(&folded, &optimized);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&optimized).unwrap()).unwrap();
    let ids: Vec<u64> = nodes_of(&ir)
        .into_iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1, 2], "the hidden branch is gone");
    let boundaries = ir["nodeBoundaries"].as_object().unwrap();
    for (key, entry) in boundaries {
        assert!(
            ids.contains(&key.parse().unwrap()),
            "stale boundary id {key}"
        );
        for side in ["repaint", "relayout"] {
            if let Some(boundary) = entry[side].as_u64() {
                assert!(ids.contains(&boundary), "stale boundary target {boundary}");
            }
        }
    }
    for (name, list) in ir["stateRefs"].as_object().unwrap() {
        for id in list.as_array().unwrap() {
            let id = id.as_u64().unwrap();
            assert!(ids.contains(&id), "stale stateRefs id {id} for {name}");
        }
    }
}

#[test]
fn fold_node_visibility_never_renumbers_across_routes() {
    let dir = out_dir("fold-nodes-routes");
    let src = dir.join("multi.opu");
    fs::write(
        &src,
        "app Multi {\n  state debug = false\n  route Home {\n    Column {\n      if (debug) {\n        Text(\"gone\")\n      }\n      Text(\"home\")\n    }\n  }\n  route Detail {\n    Column {\n      Text(\"detail\")\n    }\n  }\n}\n",
    )
    .unwrap();
    let plain = dir.join("plain.opuir.json");
    let folded = dir.join("folded.opuir.json");
    assert!(compile_raw(&src, &plain).status.success());
    let r = compile_nodes_folded(&src, &folded);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let a: Json = serde_json::from_slice(&fs::read(&plain).unwrap()).unwrap();
    let b: Json = serde_json::from_slice(&fs::read(&folded).unwrap()).unwrap();
    let tree_ids = |ir: &Json, name: &str| -> Vec<u64> {
        let mut out = Vec::new();
        fn walk(n: &Json, out: &mut Vec<u64>) {
            out.push(n["id"].as_u64().unwrap());
            for c in n["children"].as_array().unwrap() {
                walk(c, out);
            }
        }
        walk(&ir["routes"][name]["tree"], &mut out);
        out
    };
    let home_plain = tree_ids(&a, "Home");
    let detail_plain = tree_ids(&a, "Detail");
    let home_folded = tree_ids(&b, "Home");
    let detail_folded = tree_ids(&b, "Detail");
    assert_eq!(home_plain.len(), 3, "{home_plain:?}");
    assert_eq!(home_folded.len(), 2, "{home_folded:?}");
    assert_eq!(home_folded[0], home_plain[0], "route root id is preserved");
    assert_eq!(
        detail_folded, detail_plain,
        "an untouched route keeps every id"
    );
    assert_eq!(
        (1..home_folded.len())
            .map(|i| home_folded[i] - home_folded[i - 1])
            .max()
            .unwrap(),
        1,
        "each tree stays dense pre-order from its own root"
    );
}

#[test]
fn fold_nodes_is_a_no_op_without_a_foldable_pattern() {
    let dir = out_dir("fold-nodes-noop");
    for (i, src) in [contract("counter.opu"), fixture("golden/todo.opu")]
        .into_iter()
        .enumerate()
    {
        let plain = dir.join(format!("{i}-plain.json"));
        let folded = dir.join(format!("{i}-folded.json"));
        assert!(compile(&src, &plain).status.success());
        let r = compile_nodes_folded(&src, &folded);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(
            fs::read(&folded).unwrap(),
            fs::read(&plain).unwrap(),
            "no foldable pattern means byte-identical output"
        );
        assert!(!folded.with_extension("idmap.json").exists());
    }
}

#[test]
fn optimizer_exports_state_refs_and_nearest_boundaries() {
    let dir = out_dir("opt-maps");
    let raw = dir.join("raw.opuir.json");
    let r = compile_raw(&fixture("incremental_maps.opu"), &raw);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = dir.join("optimized.opuir.json");
    let r = optimize(&raw, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();

    // Direct references only, sorted and deduplicated; dead state is gone from
    // both the state array and the changed-key map.
    assert_eq!(got["stateRefs"], json!({"a": [2, 5], "b": [4, 7]}));
    assert!(got["stateRefs"].get("dead").is_none());
    assert_eq!(state_names(&got), vec!["a", "b"]);

    // Nearest boundary is inclusive; a false prop inherits; no boundary is null.
    assert_eq!(
        got["nodeBoundaries"],
        json!({
            "0": {"repaint": null, "relayout": null},
            "1": {"repaint": 1, "relayout": null},
            "2": {"repaint": 1, "relayout": null},
            "3": {"repaint": 1, "relayout": 3},
            "4": {"repaint": 1, "relayout": 3},
            "5": {"repaint": 1, "relayout": 3},
            "6": {"repaint": 6, "relayout": 3},
            "7": {"repaint": 6, "relayout": 3},
            "8": {"repaint": 1, "relayout": 3},
            "9": {"repaint": null, "relayout": null}
        })
    );

    // Two runs, plus re-optimizing exported IR, produce identical bytes.
    let again = dir.join("optimized-again.opuir.json");
    let r = optimize(&raw, &again);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());
    let twice = dir.join("optimized-twice.opuir.json");
    let r = optimize(&out, &twice);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(fs::read(&out).unwrap(), fs::read(&twice).unwrap());
}

#[test]
fn optimizer_maps_are_absent_without_boundaries() {
    let dir = out_dir("opt-maps-absent");
    for (src, golden) in [
        (
            contract("counter.opu"),
            Some(fixture("golden/counter.opuir.json")),
        ),
        (
            fixture("golden/todo.opu"),
            Some(fixture("golden/todo.opuir.json")),
        ),
        (
            workspace()
                .join("..")
                .join("opurom-examples/apps/forum/forum.opu"),
            None,
        ),
    ] {
        if !src.is_file() {
            continue;
        }
        let compiled = dir.join("compiled.opuir.json");
        let r = compile(&src, &compiled);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let out = dir.join("optimized.opuir.json");
        let r = optimize(&compiled, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        if let Some(golden) = golden {
            assert_eq!(
                fs::read(&out).unwrap(),
                fs::read(&golden).unwrap(),
                "{} changed its bytes",
                golden.display()
            );
        }
        let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert!(ir.get("stateRefs").is_none());
        assert!(ir.get("nodeBoundaries").is_none());
    }
}

#[test]
fn optimizer_removes_dead_state_and_null_props() {
    let dir = out_dir("opt-dead");
    let ir_path = dir.join("dead.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1.0",
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
        "ir": "0.1.0",
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
    let base = json!({"ir": "0.1.0", "app": "X", "state": [], "tree": tree, "theme": {}});
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
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
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

    // Phase A data path: fetch lands API data directly in a list state.
    let list_ir = compile_ok(&fixture("fetch_list.opu"));
    assert_eq!(
        list_ir["tree"]["events"]["tap"]["effects"],
        json!([{"kind": "fetch", "path": "/api/items", "into": "items"}])
    );
    assert_eq!(
        list_ir["state"][0],
        json!({"name": "items", "type": "list", "init": []})
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
        (
            "fetch_non_string.opu",
            "'fetch' requires a string or list state",
        ),
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
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
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
    let plain = compile_ok(&contract("counter.opu"));
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
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
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
    // A list-typed loop keeps iterating at runtime: one For node carries the
    // template and `{item}` stays the runtime placeholder (DYNAMIC_LISTS.md).
    let for_node = nodes
        .iter()
        .find(|n| (**n)["kind"] == "For")
        .copied()
        .expect("For node for list state");
    assert_eq!(
        for_node["props"],
        json!({"source": "items", "item": "item"})
    );
    assert_eq!(for_node["children"][0]["props"]["value"], "{item}");
    assert!(
        nodes
            .iter()
            .all(|n| (**n)["props"]["value"].as_str() != Some("a")),
        "list rows must not unroll into literal nodes"
    );

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
    // The loop is one runtime For node; `{item}` stays the template
    // placeholder instead of unrolling into per-row literals.
    let for_node = nodes_of(&list_ir)
        .into_iter()
        .find(|n| n["kind"] == "For")
        .expect("For node for list state");
    assert_eq!(
        for_node["props"],
        json!({"source": "items", "item": "item"})
    );
    let values: Vec<&str> = for_node["children"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    assert_eq!(values, vec!["{item}"]);
}

#[test]
fn list_state_mutates_through_assign_append_and_remove_at() {
    let ir = compile_ok(&fixture("list_mutate.opu"));
    let nodes = nodes_of(&ir);
    let events = |label: &str| -> Json {
        nodes
            .iter()
            .find(|n| (**n)["props"]["label"].as_str() == Some(label))
            .copied()
            .expect("button node")["events"]["tap"]
            .clone()
    };
    assert_eq!(
        events("add"),
        json!({"actions": [
            {"op": "append", "target": "items", "value": "d"},
            {"op": "append", "target": "items", "valueFrom": "tag"}
        ]})
    );
    assert_eq!(
        events("drop"),
        json!({"actions": [
            {"op": "removeAt", "target": "items", "value": 0},
            {"op": "removeAt", "target": "items", "valueFrom": "index"}
        ]})
    );
    assert_eq!(
        events("reset"),
        json!({"actions": [
            {"op": "assign", "target": "items", "value": ["x", "y"]},
            {"op": "assign", "target": "items", "valueFrom": "other"}
        ]})
    );
}

#[test]
fn list_state_indexing_lowers_to_literals() {
    let ir = compile_ok(&fixture("list_index_state.opu"));
    assert_eq!(
        ir["state"][0],
        json!({"name": "items", "type": "list", "init": ["alpha", "beta", "gamma"]})
    );
    let values: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .collect();
    // A count loop indexes by position; a literal reads one element.
    assert_eq!(values, vec!["alpha", "beta", "gamma", "first=alpha"]);
}

#[test]
fn list_index_renders_through_the_web_emitter() {
    let ir = out_dir("index-ir").join("index.opuir.json");
    let r = compile(&fixture("list_index_state.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = out_dir("index-web");
    let e = emit(&ir, "web", &out);
    assert!(e.status.success(), "{}", String::from_utf8_lossy(&e.stderr));
    let app_js = fs::read_to_string(out.join("app.js")).unwrap();
    // Each element resolved at compile time, so the canvas draws its text.
    for text in ["alpha", "beta", "gamma", "first=alpha"] {
        assert!(
            app_js.contains(&format!("fillText(\"{text}\"")),
            "missing rendered '{text}' in web output: {app_js}"
        );
    }
    assert!(
        !app_js.contains("{{"),
        "unresolved interpolation in web output: {app_js}"
    );
    let check = Command::new("node")
        .arg("--check")
        .arg(out.join("app.js"))
        .output()
        .expect("run node --check");
    assert!(
        check.status.success(),
        "emitted JS is invalid: {}",
        String::from_utf8_lossy(&check.stderr)
    );
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
            "computed expression '+' requires int operands",
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
            "this mutation is not valid on list state 'items'",
        ),
        (
            "list_interpolation.opu",
            "list state 'items' is not allowed here",
        ),
        ("list_bind.opu", "list state 'items' is not allowed here"),
        ("append_non_list.opu", "'append()' requires a list state"),
        (
            "remove_at_non_list.opu",
            "'removeAt()' requires a list state",
        ),
        (
            "append_type_misuse.opu",
            "'append()' requires a string element",
        ),
        (
            "remove_at_type_misuse.opu",
            "'removeAt()' requires an int index",
        ),
        ("list_braces.opu", "list values may not contain braces"),
        ("for_non_list.opu", "'for' requires a list state"),
        (
            "for_unknown_list.opu",
            "unknown list state 'items' in 'for'",
        ),
        (
            "list_index_out_of_range.opu",
            "list index 5 is out of range for 'items' with 2 values",
        ),
        (
            "list_index_non_list.opu",
            "'count[...]' reads a list, found int state 'count'",
        ),
        (
            "list_index_non_int.opu",
            "list index 'v' must be an int, found string",
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
            "computed expression supports +, -, *, comparisons, boolean operators, integer literals, string literals and declared state names",
        ),
        (
            "computed_float".to_string(),
            r#"app Bad {
  computed bad = 1.5
  Text("x")
}
"#
            .to_string(),
            "computed expression supports +, -, *, comparisons, boolean operators, integer literals, string literals and declared state names",
        ),
        (
            "computed_bool".to_string(),
            r#"app Bad {
  computed bad = true
  Text("x")
}
"#
            .to_string(),
            "computed expression supports +, -, *, comparisons, boolean operators, integer literals, string literals and declared state names",
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
            "computed expression supports +, -, *, comparisons, boolean operators, integer literals, string literals and declared state names",
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
            "list state 'items' is not allowed in a computed expression",
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
            "computed expression supports +, -, *, comparisons, boolean operators, integer literals, string literals and declared state names",
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
        let capability = compile(path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{label}: compile must fail"
        );
        assert!(!out.exists(), "{label}: no IR may be written on error");
    }
}

#[test]
fn optimizer_preserves_computed_conditions_and_list_state() {
    let dir = out_dir("lang-opt");
    let ir_path = dir.join("features.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let r = compile_raw(&fixture("effects_features.opu"), &ir_path);
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
fn optimizer_removes_dead_computed_entries_and_state() {
    let dir = out_dir("opt-dead-computed");
    let ir_path = dir.join("dead_computed.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1.0",
        "app": "DeadComputed",
        "state": [
            {"name": "count", "type": "int", "init": 2},
            {"name": "spare", "type": "int", "init": 7},
            {"name": "shown", "type": "int", "init": 4},
            {"name": "unused", "type": "int", "init": 14}
        ],
        "computed": [
            {"name": "shown", "type": "int", "expr": "count * 2"},
            {"name": "unused", "type": "int", "expr": "spare + 1"}
        ],
        "tree": {
            "id": 0, "kind": "Column", "props": {}, "children": [
                {"id": 1, "kind": "Text", "props": {"value": "Total: {shown}"},
                 "children": [], "events": {}},
                {"id": 2, "kind": "Button", "props": {"label": "go"}, "children": [],
                 "events": {"tap": {"op": "add", "target": "count", "value": 1}}}
            ], "events": {}
        },
        "theme": {"mode": "system", "accent": "#097FEB", "radius": 12}
    });
    let text = pretty_ir(&ir);
    fs::write(&ir_path, &text).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();

    assert_eq!(
        got["computed"],
        json!([{"name": "shown", "type": "int", "expr": "count * 2"}]),
        "the unobserved computed declaration is removed"
    );
    assert_eq!(state_names(&got), vec!["count", "shown"]);
    assert_eq!(node_ids(&got), node_ids(&ir), "nodes and ids survive");
    assert_eq!(
        got["tree"]["children"][0]["props"]["value"], "Total: {shown}",
        "the surviving interpolation is untouched"
    );
    assert_eq!(
        got["tree"]["children"][1]["events"]["tap"]["target"], "count",
        "the action that feeds the surviving computed declaration is untouched"
    );
    assert!(
        fs::metadata(&out).unwrap().len() < text.len() as u64,
        "optimized IR must shrink"
    );
}

#[test]
fn optimizer_drops_empty_computed_array_and_is_idempotent() {
    let dir = out_dir("opt-all-dead-computed");
    let ir_path = dir.join("all_dead.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1.0",
        "app": "AllDead",
        "state": [
            {"name": "count", "type": "int", "init": 2},
            {"name": "spare", "type": "int", "init": 7},
            {"name": "unused", "type": "int", "init": 14}
        ],
        "computed": [{"name": "unused", "type": "int", "expr": "spare + 1"}],
        "tree": {
            "id": 0, "kind": "Column", "props": {}, "children": [
                {"id": 1, "kind": "Text", "props": {"value": "{count}"},
                 "children": [], "events": {}}
            ], "events": {}
        },
        "theme": {"mode": "system", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir_path, pretty_ir(&ir)).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert!(got.get("computed").is_none(), "an emptied array is removed");
    assert_eq!(state_names(&got), vec!["count"]);
    assert_eq!(node_ids(&got), node_ids(&ir));

    let second = dir.join("twice.opuir.json");
    let capability = optimize(&out, &second);
    assert!(
        capability.status.success(),
        "{}",
        String::from_utf8_lossy(&capability.stderr)
    );
    assert_eq!(
        fs::read(&second).unwrap(),
        fs::read(&out).unwrap(),
        "a second pass is a no-op"
    );
}

#[test]
fn optimizer_keeps_computed_chains_and_conditions() {
    let dir = out_dir("opt-computed-chain");
    let ir_path = dir.join("chain.opuir.json");
    let out = dir.join("optimized.opuir.json");
    let ir = json!({
        "ir": "0.1.0",
        "app": "Chain",
        "state": [
            {"name": "flag", "type": "bool", "init": false},
            {"name": "base", "type": "int", "init": 3},
            {"name": "scaled", "type": "int", "init": 4},
            {"name": "shown", "type": "int", "init": 8},
            {"name": "ready", "type": "bool", "init": false},
            {"name": "orphan", "type": "int", "init": 11},
            {"name": "hidden", "type": "int", "init": 22}
        ],
        "computed": [
            {"name": "scaled", "type": "int", "expr": "base+1"},
            {"name": "shown", "type": "int", "expr": "scaled * 2"},
            {"name": "ready", "type": "bool", "expr": "flag"},
            {"name": "hidden", "type": "int", "expr": "orphan * 2"}
        ],
        "tree": {
            "id": 0, "kind": "Column", "props": {}, "children": [
                {"id": 1, "kind": "Text", "when": "ready",
                 "props": {"value": "Value: {shown}"}, "children": [], "events": {}}
            ], "events": {}
        },
        "theme": {"mode": "system", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir_path, pretty_ir(&ir)).unwrap();
    let r = optimize(&ir_path, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let got: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();

    let computed: Vec<&str> = got["computed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(computed, vec!["scaled", "shown", "ready"]);
    assert_eq!(
        state_names(&got),
        vec!["flag", "base", "scaled", "shown", "ready"]
    );
    assert_eq!(got["tree"]["children"][0]["when"], "ready");
    assert_eq!(node_ids(&got), node_ids(&ir));
}

#[test]
fn emitters_apply_computed_values_and_honor_unless() {
    let ir = out_dir("emit-ir").join("features.opuir.json");
    let r = compile_raw(&fixture("effects_features.opu"), &ir);
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
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
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
        ("harmony", "src/main/ets/pages/Index.ets"),
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
        "260 components",
        "the built-in catalog is unchanged"
    );
}

#[test]
fn component_plain_ir_stays_byte_identical() {
    let cases = [
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
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
            if p.len() == 3 && p[0] == p[1] && !CORE_PRIMITIVES.contains(&p[0]) {
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
fn all_ten_emitters_keep_working() {
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
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
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

#[test]
fn mobile_alias_catalog_components_compile_and_emit_every_target() {
    let dir = out_dir("mobile-alias-catalog");
    let src_path = dir.join("mobile-alias.opu");
    let mut src = String::from(
        "app MobileAliasCatalog {\n  state enabled = false\n  state volume = 0\n  \
         state selection = \"Surfing\"\n  Column {\n",
    );
    for name in MOBILE_ALIAS_NAMES {
        src.push_str(&format!("    {name}()\n"));
    }
    src.push_str("  }\n}\n");
    fs::write(&src_path, src).unwrap();

    let ir_path = dir.join("mobile-alias.opuir.json");
    let r = compile(&src_path, &ir_path);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let ir: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    let nodes = nodes_of(&ir);
    for name in MOBILE_ALIAS_NAMES {
        assert!(
            nodes
                .iter()
                .any(|n| n.get("component").and_then(Json::as_str) == Some(name)),
            "missing compiled component {name}"
        );
    }
    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
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

#[test]
fn unknown_prop_on_new_component_is_a_friendly_diagnostic() {
    let dir = out_dir("mobile-alias-badprop");
    let src_path = dir.join("bad.opu");
    fs::write(
        &src_path,
        "app BadProp {\n  Column {\n    Scaffold(bogus: 1)\n  }\n}\n",
    )
    .unwrap();
    let r = bin()
        .arg("check")
        .arg(&src_path)
        .output()
        .expect("run check");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("unknown prop 'bogus' on Scaffold"), "{err}");
    assert_diag(&err, &src_path);

    let out = dir.join("out.opuir.json");
    assert_eq!(compile(&src_path, &out).status.code(), Some(1));
    assert!(!out.exists(), "no IR may be written on error");
}

#[test]
fn feedback_and_display_catalog_components_compile_and_emit_every_target() {
    let dir = out_dir("feedback-display-catalog");
    let src_path = dir.join("components.opu");
    let mut src = String::from("app FeedbackDisplayCatalog {\n  Column {\n");
    for &(name, args, _) in FEEDBACK_DISPLAY_COMPONENTS {
        src.push_str(&format!("    {name}({args})\n"));
    }
    src.push_str("  }\n}\n");
    fs::write(&src_path, src).unwrap();

    let ir_path = dir.join("components.opuir.json");
    let r = compile(&src_path, &ir_path);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let ir: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    let nodes = nodes_of(&ir);
    assert_eq!(nodes.len(), FEEDBACK_DISPLAY_COMPONENTS.len() + 1);
    for (index, &(name, _, primitive)) in FEEDBACK_DISPLAY_COMPONENTS.iter().enumerate() {
        let node = &nodes[index + 1];
        assert_eq!(node["kind"], primitive, "wrong primitive for {name}");
        if CORE_PRIMITIVES.contains(&name) {
            assert!(node.get("component").is_none(), "{name} is a primitive");
        } else {
            assert_eq!(node["component"], name, "missing catalog component {name}");
        }
    }

    let cases: &[(&str, &str)] = &[
        ("native", "src/main.rs"),
        ("web", "app.js"),
        ("wechat", "pages/index/index.js"),
        ("alipay", "pages/index/index.js"),
        ("qq", "pages/index/index.js"),
        ("douyin", "pages/index/index.js"),
        ("baidu", "pages/index/index.js"),
        ("kuaishou", "pages/index/index.js"),
        ("wasm", "host.js"),
        ("harmony", "src/main/ets/pages/Index.ets"),
    ];
    for &(target, artifact) in cases {
        let out = out_dir(&format!("feedback-display-{target}"));
        let e = emit(&ir_path, target, &out);
        assert!(
            e.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&e.stderr)
        );
        let file = out.join(artifact);
        assert!(file.is_file(), "{target}: missing {artifact}");
        if artifact.ends_with(".js") {
            node_check(&file);
        }
    }
}

#[test]
fn unknown_props_on_feedback_and_display_components_are_friendly() {
    let dir = out_dir("feedback-display-badprop");
    for &(name, args, _) in FEEDBACK_DISPLAY_COMPONENTS {
        let call_args = if args.is_empty() {
            String::from("bogus: 1")
        } else {
            format!("{args}, bogus: 1")
        };
        let src_path = dir.join(format!("{name}-bad.opu"));
        fs::write(
            &src_path,
            format!("app BadProp {{\n  Column {{\n    {name}({call_args})\n  }}\n}}\n"),
        )
        .unwrap();
        let r = bin()
            .arg("check")
            .arg(&src_path)
            .output()
            .expect("run check");
        assert_eq!(r.status.code(), Some(1), "{name} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(&format!("unknown prop 'bogus' on {name}")),
            "{name}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &src_path);

        let out = dir.join(format!("{name}-out.opuir.json"));
        let capability = compile(&src_path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{name}: compile must fail"
        );
        assert!(!out.exists(), "{name}: no IR may be written on error");
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
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
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
fn int_boundary_values_stay_valid() {
    let ir = compile_ok(&fixture("int_boundary.opu"));
    assert_eq!(
        ir["state"],
        json!([
            {"name": "max", "type": "int", "init": i64::MAX},
            {"name": "min", "type": "int", "init": -i64::MAX},
            {"name": "atMax", "type": "int", "init": i64::MAX},
            {"name": "atMin", "type": "int", "init": i64::MIN},
            {"name": "maxTimesOne", "type": "int", "init": i64::MAX},
        ])
    );
}

#[test]
fn integer_overflow_is_rejected_with_exact_message() {
    let cases: &[(&str, &str)] = &[
        (
            "computed_overflow_add.opu",
            "2:18: error: computed expression '+' overflows int: 9223372036854775807 + 1\n  help: use smaller operands, e.g. '9223372036854775807 + 0'; int results must fit between -9223372036854775808 and 9223372036854775807\n",
        ),
        (
            "computed_overflow_sub.opu",
            "2:18: error: computed expression '-' overflows int: -9223372036854775807 - 2\n  help: use smaller operands, e.g. '-9223372036854775807 - 1'; int results must fit between -9223372036854775808 and 9223372036854775807\n",
        ),
        (
            "computed_overflow_mul.opu",
            "2:19: error: computed expression '*' overflows int: 9223372036854775807 * 2\n  help: use smaller operands, e.g. '9223372036854775807 * 1'; int results must fit between -9223372036854775808 and 9223372036854775807\n",
        ),
        (
            "computed_overflow_state.opu",
            "3:21: error: computed expression '+' overflows int: 9223372036854775807 + 1\n  help: use smaller operands, e.g. '9223372036854775807 + 0'; int results must fit between -9223372036854775808 and 9223372036854775807\n",
        ),
        (
            "state_int_out_of_range.opu",
            "2:15: error: integer literal '9223372036854775808' is out of range\n  help: use a value between -9223372036854775808 and 9223372036854775807\n",
        ),
    ];
    for &(file, tail) in cases {
        let path = error_fixture(file);
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{file} must fail");
        let expected = format!("{}:{}", path.display(), tail);
        assert_eq!(String::from_utf8_lossy(&r.stderr), expected, "{file}");
        let out = out_dir("overflow-reject").join(format!("{file}.opuir.json"));
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
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
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
        (
            fixture("golden/showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (
            fixture("golden/gallery.opu"),
            fixture("golden/gallery.opuir.json"),
        ),
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

// ---------------------------------------------- state-reference arithmetic ----

/// A state reference cannot be negated in place, so `-=` lowers to the "sub"
/// action while `+=` keeps "add"; both read the value at event time.
#[test]
fn state_subtraction_lowers_to_sub_and_addition_to_add() {
    let sub = compile_ok(&fixture("sub_state.opu"));
    let nodes = nodes_of(&sub);
    assert_eq!(
        nodes[1]["events"]["tap"],
        json!({"op": "sub", "target": "total", "valueFrom": "step"})
    );

    let add = compile_ok(&fixture("add_state.opu"));
    let nodes = nodes_of(&add);
    assert_eq!(
        nodes[1]["events"]["tap"],
        json!({"op": "add", "target": "total", "valueFrom": "step"})
    );
}

const WEB_ACTION_DRIVER: &str = r#"
const fs = require('node:fs');
const src = fs.readFileSync(process.argv[1], 'utf8');
const ctx = new Proxy({}, { get: () => () => {}, set: () => true });
const canvas = { width: 800, height: 600, getContext: () => ctx,
  addEventListener: () => {}, getBoundingClientRect: () => ({ left: 0, top: 0 }) };
const stub = { getElementById: () => canvas };
const load = new Function('document', src + '\nreturn { state, applyTap };');
const { state, applyTap } = load(stub);
applyTap(Number(process.argv[2]));
console.log('total=' + state.total);
"#;

fn run_web_action(app_js: &Path, tap_id: u64) -> String {
    let r = Command::new("node")
        .arg("-e")
        .arg(WEB_ACTION_DRIVER)
        .arg(app_js)
        .arg(tap_id.to_string())
        .output()
        .expect("run node web action driver");
    assert!(
        r.status.success(),
        "web action driver failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).to_string()
}

fn find_tap_id(ir: &Json) -> u64 {
    nodes_of(ir)
        .into_iter()
        .find(|n| n["events"].get("tap").is_some())
        .and_then(|n| n["id"].as_u64())
        .expect("tap node id")
}

#[test]
fn every_emitter_lowers_the_sub_action() {
    let ir = out_dir("sub-emit-ir").join("sub.opuir.json");
    let r = compile(&fixture("sub_state.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let native = out_dir("sub-native");
    let r = emit(&ir, "native", &native);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let native_rs = fs::read_to_string(native.join("src/main.rs")).unwrap();
    assert!(
        native_rs.contains("_state.total = _state.total.saturating_sub(_state.step);"),
        "native source missing saturating_sub: {native_rs}"
    );

    let web = out_dir("sub-web");
    let r = emit(&ir, "web", &web);
    assert!(
        r.status.success(),
        "web emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(app_js.contains("state.total -= state.step;"), "{app_js}");
    node_check(&web.join("app.js"));

    for target in ["wechat", "alipay", "qq", "douyin", "baidu", "kuaishou"] {
        let dir = out_dir(&format!("sub-{target}"));
        let r = emit(&ir, target, &dir);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let js = fs::read_to_string(dir.join("pages/index/index.js")).unwrap();
        assert!(
            js.contains("this.data.total - (this.data.step)"),
            "{target} index.js missing subtraction: {js}"
        );
        node_check(&dir.join("pages/index/index.js"));
    }

    let wasm = out_dir("sub-wasm");
    let r = emit(&ir, "wasm", &wasm);
    assert!(
        r.status.success(),
        "wasm emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let lib_rs = fs::read_to_string(wasm.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("state(0).saturating_sub(state(1))"),
        "wasm lib.rs missing SubFrom: {lib_rs}"
    );
    let host_js = fs::read_to_string(wasm.join("host.js")).unwrap();
    assert!(host_js.contains("action.op === \"sub\""), "{host_js}");
    node_check(&wasm.join("host.js"));
}

#[test]
fn native_sub_and_add_state_values_apply_at_tap() {
    // One process per fixture: 10 - 3 = 7 and 10 + 3 = 13 from the initial state.
    for (name, expected_state, expected_patch) in [
        ("sub_state.opu", "state total=7 step=3", "patch total=7"),
        ("add_state.opu", "state total=13 step=3", "patch total=13"),
    ] {
        let ir = out_dir("arith-ir").join("out.opuir.json");
        let r = compile(&fixture(name), &ir);
        assert!(
            r.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let tap_id = find_tap_id(&serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap());
        let dir = out_dir("arith-native");
        let r = emit(&ir, "native", &dir);
        assert!(
            r.status.success(),
            "{name}: native emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let (built, stderr) = build_native(&dir);
        assert!(built, "{name}: cargo build --release failed:\n{stderr}");
        let (ran, stdout) = run_native(
            &dir.join("target/release/opurom_native"),
            &format!("tap {tap_id}\nstate\nquit\n"),
        );
        assert!(ran, "{name}: native binary exited nonzero");
        assert!(stdout.contains(expected_state), "{name}: {stdout}");
        assert!(stdout.contains(expected_patch), "{name}: {stdout}");
    }
}

#[test]
fn web_sub_and_add_state_values_apply_at_tap() {
    for (name, expected) in [("sub_state.opu", "total=7"), ("add_state.opu", "total=13")] {
        let ir = out_dir("web-arith-ir").join("out.opuir.json");
        let r = compile(&fixture(name), &ir);
        assert!(
            r.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let tap_id = find_tap_id(&serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap());
        let dir = out_dir("web-arith");
        let r = emit(&ir, "web", &dir);
        assert!(
            r.status.success(),
            "{name}: web emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let stdout = run_web_action(&dir.join("app.js"), tap_id);
        assert!(stdout.contains(expected), "{name}: {stdout}");
    }
}

#[test]
fn theme_font_scale_lowers_to_the_documented_ir() {
    let ir = compile_ok(&fixture("theme_font_scale.opu"));
    assert_eq!(
        ir["theme"],
        json!({"mode": "system", "accent": "#097FEB", "radius": 12, "fontScale": 1.25})
    );
}

#[test]
fn theme_without_font_scale_stays_unchanged() {
    let ir = compile_ok(&fixture("features.opu"));
    assert_eq!(ir["theme"].get("fontScale"), None);
}

#[test]
fn theme_transition_lowers_to_the_top_level_ir_key() {
    let dir = out_dir("theme-transition");
    let src = dir.join("transition.opu");
    fs::write(
        &src,
        "app Transition {\n  theme { transition: fade }\n  Column { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["transition"], json!("fade"));

    // A source without a transition declares no key at all, so the IR schema
    // and the pinned goldens are unchanged.
    let plain = dir.join("plain.opu");
    fs::write(&plain, "app Plain { Column { Text(\"hi\") } }\n").unwrap();
    let ir = compile_ok(&plain);
    assert_eq!(ir.get("transition"), None);
}

#[test]
fn theme_transition_rejects_unknown_kinds() {
    let dir = out_dir("theme-transition-bad");
    let src = dir.join("bad.opu");
    fs::write(
        &src,
        "app Bad { theme { transition: zoom } Column { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&src, &dir.join("bad.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("unknown transition 'zoom'"), "{stderr}");
}

#[test]
fn particles_prop_lowers_to_the_bounded_ir() {
    let ir = compile_ok(&fixture("particles.opu"));
    let node = nodes_of(&ir)
        .into_iter()
        .find(|n| n["props"].get("particles").is_some())
        .expect("a node carries the particles prop");
    assert_eq!(
        node["props"]["particles"],
        json!({"count": 64, "seed": 7, "radius": 2, "alpha": 512, "color": "#101828"})
    );
}

#[test]
fn particles_prop_applies_the_documented_defaults() {
    let ir = compile_ok(&fixture("particles_defaults.opu"));
    let node = nodes_of(&ir)
        .into_iter()
        .find(|n| n["props"].get("particles").is_some())
        .expect("a node carries the particles prop");
    assert_eq!(
        node["props"]["particles"],
        json!({"count": 3, "seed": 1, "radius": 2, "alpha": 1024})
    );
    assert_eq!(node["props"]["particles"].get("color"), None);
}

#[test]
fn particles_field_errors_are_precise() {
    let cases: &[(&str, &str)] = &[
        (
            "particles_value_type.opu",
            "prop 'particles' expects a field list, found int",
        ),
        (
            "particles_shape.opu",
            "particles field '64' must be written name=value",
        ),
        (
            "particles_missing_count.opu",
            "particles requires the 'count' field",
        ),
        (
            "particles_count_range.opu",
            "particles.count 513 must be 0..=512",
        ),
        (
            "particles_count_type.opu",
            "particles.count must be an integer 0..=512, found 'many'",
        ),
        (
            "particles_seed_range.opu",
            "particles.seed 4294967296 must be 0..=4294967295",
        ),
        (
            "particles_unknown_field.opu",
            "unknown particles field 'cnt'",
        ),
        (
            "particles_duplicate_field.opu",
            "duplicate particles field 'count'",
        ),
        (
            "particles_bad_color.opu",
            "invalid color 'red' for 'particles.color'",
        ),
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

        let out = out_dir("particles-reject").join("out.opuir.json");
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{file}: compile must fail"
        );
        assert!(!out.exists(), "{file}: no IR may be written on error");
    }
}

/// Run the sibling renderer on one IR file at 320x240. The offline attempt
/// keeps the common case network-free; a cold cargo cache falls back to a
/// regular run, which the CI checkout provides.
fn render_ir(ir: &Path, ppm: &Path) -> (bool, String) {
    let manifest = workspace()
        .join("..")
        .join("opurom-render")
        .join("Cargo.toml");
    let run = |offline: bool| {
        let mut cmd = Command::new(cargo_path());
        cmd.arg("run").arg("--quiet");
        if offline {
            cmd.arg("--offline");
        }
        cmd.arg("--manifest-path")
            .arg(&manifest)
            .arg("--")
            .arg(ir)
            .arg("320")
            .arg("240")
            .arg(ppm);
        cmd.output().expect("run opurom-render")
    };
    let out = run(true);
    let out = if out.status.success() {
        out
    } else {
        run(false)
    };
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn render_digest(text: &str) -> &str {
    text.lines()
        .find_map(|l| l.split_whitespace().find(|t| t.starts_with("digest=")))
        .map(|t| &t["digest=".len()..])
        .unwrap_or("")
}

fn strip_particles(node: &mut Json) {
    if let Some(props) = node.get_mut("props").and_then(Json::as_object_mut) {
        props.remove("particles");
    }
    if let Some(children) = node.get_mut("children").and_then(Json::as_array_mut) {
        for child in children {
            strip_particles(child);
        }
    }
}

#[test]
fn particles_prop_changes_the_rendered_frame() {
    let dir = out_dir("particles-render");
    let with_ir = dir.join("with.opuir.json");
    let r = compile(&fixture("particles.opu"), &with_ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let mut doc: Json =
        serde_json::from_slice(&fs::read(&with_ir).expect("read compiled IR")).expect("parse IR");
    strip_particles(doc.get_mut("tree").expect("IR tree"));
    let base_ir = dir.join("without.opuir.json");
    fs::write(&base_ir, serde_json::to_vec(&doc).expect("serialize IR")).expect("write IR");

    let (ok_with, with_out) = render_ir(&with_ir, &dir.join("with.ppm"));
    let (ok_without, without_out) = render_ir(&base_ir, &dir.join("without.ppm"));
    assert!(
        ok_with && ok_without,
        "render failed:\n{with_out}\n{without_out}"
    );
    let with_digest = render_digest(&with_out);
    let base_digest = render_digest(&without_out);
    assert!(
        with_digest.len() == 16 && base_digest.len() == 16,
        "missing digest:\n{with_out}\n{without_out}"
    );
    assert_ne!(
        with_digest, base_digest,
        "particles must change the rendered digest"
    );
    assert_ne!(
        fs::read(dir.join("with.ppm")).expect("read with.ppm"),
        fs::read(dir.join("without.ppm")).expect("read without.ppm"),
        "particles must change the rendered pixels"
    );
}

/// Every canonical icon name, mirroring `crate::icons::ICON_NAMES`.
const ALL_ICONS: &[&str] = &[
    "home",
    "settings",
    "star",
    "search",
    "plus",
    "minus",
    "check",
    "close",
    "arrow_back",
    "arrow_forward",
    "refresh",
    "lock",
    "user",
    "bell",
    "heart",
    "info",
    "warning",
    "chevron_right",
    "chevron_down",
    "download",
    "upload",
    "play",
    "pause",
    "edit",
    "delete",
    "eye",
    "calendar",
    "location",
    "folder",
    "mail",
    "chat",
];

#[test]
fn canvas_web_icon_draws_the_canonical_path() {
    let dir = out_dir("icon-canvas");
    let src = dir.join("icon.opu");
    fs::write(
        &src,
        "app IconCanvas {\n  Column {\n    Icon(name: \"star\")\n  }\n}\n",
    )
    .expect("write icon canvas fixture");
    let ir = dir.join("icon.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let web = dir.join("web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let js = fs::read_to_string(web.join("app.js")).expect("read web app.js");
    assert!(
        js.contains("ctx.stroke(new Path2D("),
        "canvas icon must stroke the canonical path: {js}"
    );
    assert!(
        js.contains("new Path2D(\"M12 3 L14.9 9.1"),
        "the star path must be embedded in the canvas output"
    );
    assert!(
        !js.contains("fillText(\"star\""),
        "the icon name must not be drawn as text: {js}"
    );
}

#[test]
fn mini_targets_emit_real_icon_elements() {
    let dir = out_dir("icon-mini");
    let src = dir.join("icon.opu");
    fs::write(
        &src,
        "app IconMini {\n  Column {\n    Icon(name: \"star\")\n  }\n}\n",
    )
    .expect("write icon mini fixture");
    let ir = dir.join("icon.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let cases: &[(&str, &str)] = &[
        ("wechat", "pages/index/index.wxml"),
        ("alipay", "pages/index/index.axml"),
        ("qq", "pages/index/index.qml"),
        ("douyin", "pages/index/index.ttml"),
        ("baidu", "pages/index/index.swan"),
        ("kuaishou", "pages/index/index.ksml"),
    ];
    for &(target, markup) in cases {
        let out = dir.join(target);
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let text = fs::read_to_string(out.join(markup)).expect("read mini markup");
        assert!(
            text.contains("<image"),
            "{target}: icon must emit an image element"
        );
        assert!(
            text.contains("data:image/svg+xml;base64,"),
            "{target}: icon must embed an SVG data URI"
        );
        assert!(
            !text.contains("<text>star</text>"),
            "{target}: the icon name must not render as text"
        );
    }
}

#[test]
fn icon_path_table_covers_every_canonical_name() {
    let dir = out_dir("icon-all");
    let body = ALL_ICONS
        .iter()
        .map(|n| format!("    Icon(name: \"{n}\")"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &dir.join("all.opu"),
        format!("app AllIcons {{\n  Column(gap: 4) {{\n{body}\n  }}\n}}\n"),
    )
    .expect("write all-icons fixture");
    let ir = dir.join("all.opuir.json");
    let r = compile(&dir.join("all.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let web = dir.join("web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let js = fs::read_to_string(web.join("app.js")).expect("read web app.js");
    let drawn = js.matches("ctx.stroke(new Path2D(").count();
    assert_eq!(
        drawn,
        ALL_ICONS.len(),
        "every canonical icon must draw a path"
    );

    let wechat = dir.join("wechat");
    let r = emit(&ir, "wechat", &wechat);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let markup =
        fs::read_to_string(wechat.join("pages/index/index.wxml")).expect("read wechat wxml");
    let images = markup.matches("<image").count();
    assert_eq!(
        images,
        ALL_ICONS.len(),
        "every canonical icon must emit an image"
    );
}

// ------------------------------------------- repeatable lists and forms ----

const FORMS_SOURCE: &str = r#"app Forms {
  state items = ["alpha", "beta"]
  state name = ""
  Column {
    for item in state.items {
      Text("{item}")
    }
    for note in items {
      Text("{note}")
    }
    Input(bind: name, required: true, minLength: 3, maxLength: 12, pattern: "^[A-Za-z0-9_]+$")
    Button("Check") { form.validate() }
  }
}
"#;

fn input_source(args: &str) -> String {
    format!("app Bad {{ Column {{ Input({args}) }} }}\n")
}

fn mini_markup_path(target: &str) -> Option<&'static str> {
    Some(match target {
        "wechat" => "pages/index/index.wxml",
        "alipay" => "pages/index/index.axml",
        "qq" => "pages/index/index.qml",
        "douyin" => "pages/index/index.ttml",
        "baidu" => "pages/index/index.swan",
        "kuaishou" => "pages/index/index.ksml",
        _ => return None,
    })
}

#[test]
fn repeatable_for_lowers_to_additive_ir() {
    let dir = out_dir("forms-ir");
    let src = dir.join("forms.opu");
    fs::write(&src, FORMS_SOURCE).unwrap();
    let ir_path = dir.join("forms.opuir.json");
    let r = compile(&src, &ir_path);
    assert!(
        r.status.success(),
        "forms compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let ir: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    assert_eq!(
        ir["state"][0],
        json!({"name": "items", "type": "list", "init": ["alpha", "beta"]})
    );
    let repeat_nodes: Vec<&Json> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n.get("repeat").is_some())
        .collect();
    assert_eq!(repeat_nodes.len(), 2, "the two loops stay as templates");
    assert_eq!(
        repeat_nodes[0]["repeat"],
        json!({"item": "item", "list": "items"})
    );
    assert_eq!(
        repeat_nodes[1]["repeat"],
        json!({"item": "note", "list": "items"})
    );
    assert_eq!(repeat_nodes[0]["kind"], "Column");
    assert_eq!(repeat_nodes[0]["children"][0]["props"]["value"], "{item}");
    assert_eq!(repeat_nodes[1]["children"][0]["props"]["value"], "{note}");
    let unrolled: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter_map(|n| n["props"]["value"].as_str())
        .filter(|v| *v == "alpha" || *v == "beta")
        .collect();
    assert!(unrolled.is_empty(), "repeatable loops must not unroll");

    let optimized_path = dir.join("optimized.opuir.json");
    let o = optimize(&ir_path, &optimized_path);
    assert!(
        o.status.success(),
        "forms optimize failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let optimized: Json = serde_json::from_slice(&fs::read(&optimized_path).unwrap()).unwrap();
    assert_eq!(
        nodes_of(&optimized)
            .into_iter()
            .filter(|n| n.get("repeat").is_some())
            .count(),
        2
    );
    assert_eq!(optimized["tree"]["children"][2]["props"]["required"], true);

    let plain = compile_ok(&fixture("features.opu"));
    for node in nodes_of(&plain) {
        assert!(node.get("repeat").is_none(), "plain IR gained repeat");
        for key in ["required", "minLength", "maxLength", "pattern"] {
            assert!(node["props"].get(key).is_none(), "plain IR gained {key}");
        }
    }
}

#[test]
fn validator_props_lower_to_documented_ir_and_action() {
    let dir = out_dir("forms-valid-ir");
    let src = dir.join("valid.opu");
    fs::write(
        &src,
        r#"app Validation {
  state name = ""
  Column {
    Input(bind: name, required: true, minLength: 2, maxLength: 8, pattern: "^[A-Za-z]+$")
    Button("Check") { form.validate() }
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let input = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Input")
        .expect("Input node");
    assert_eq!(
        input["props"],
        json!({
            "bind": "name",
            "required": true,
            "minLength": 2,
            "maxLength": 8,
            "pattern": "^[A-Za-z]+$"
        })
    );
    let button = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Button")
        .expect("Button node");
    assert_eq!(
        button["events"]["tap"],
        json!({"op": "validate", "target": "form"})
    );

    let bad = dir.join("unknown-prop.opu");
    fs::write(
        &bad,
        "app BadProp { Column { Input(required: true, bogus: 1) } }\n",
    )
    .unwrap();
    let r = bin().arg("check").arg(&bad).output().expect("run check");
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(
        err.contains("unknown prop 'bogus' on Input"),
        "unknown prop diagnostic changed: {err}"
    );
    assert_error_shape(&err);
}

#[test]
fn unknown_repeat_list_is_a_friendly_diagnostic() {
    let dir = out_dir("forms-unknown-list");
    let cases: &[(&str, &str, &str)] = &[
        (
            "state",
            "app Bad { for item in state.missing { Text(\"{item}\") } }\n",
            "unknown list state 'missing' in 'for'",
        ),
        (
            "bare",
            "app Bad { for item in missing { Text(\"{item}\") } }\n",
            "unknown list state 'missing' in 'for'",
        ),
        (
            "non_list",
            "app Bad { state count = 0 for item in state.count { Text(\"{item}\") } }\n",
            "'for' requires a list state, found int state 'count'",
        ),
    ];
    for &(name, source, expected) in cases {
        let path = dir.join(format!("{name}.opu"));
        fs::write(&path, source).unwrap();
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{name} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(expected),
            "{name}: unexpected diagnostic: {err}"
        );
        assert_diag(&err, &path);
        let out = dir.join(format!("{name}.opuir.json"));
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{name}: compile must fail"
        );
        assert!(!out.exists(), "{name}: no IR may be written on error");
    }
}

#[test]
fn invalid_validator_values_are_rejected_with_help() {
    let dir = out_dir("forms-invalid");
    let cases: &[(&str, &str, &str)] = &[
        (
            "negative_min",
            "minLength: -1",
            "minLength must not be negative, found -1",
        ),
        (
            "negative_max",
            "maxLength: -2",
            "maxLength must not be negative, found -2",
        ),
        (
            "reversed_range",
            "minLength: 8, maxLength: 2",
            "maxLength 2 is less than minLength 8",
        ),
        (
            "required_type",
            "required: 1",
            "prop 'required' expects true or false, found int",
        ),
        (
            "length_type",
            "minLength: \"3\"",
            "prop 'minLength' expects a non-negative integer, found string",
        ),
        (
            "pattern_type",
            "pattern: 5",
            "prop 'pattern' expects a string, found int",
        ),
        (
            "pattern_group",
            "pattern: \"(a|b)\"",
            "invalid regex-lite pattern '(a|b)': unsupported regex syntax '('",
        ),
        (
            "pattern_count",
            "pattern: \"a{2,4}\"",
            "invalid regex-lite pattern 'a{2,4}': unsupported regex syntax '{'",
        ),
        (
            "pattern_reversed_class",
            "pattern: \"[z-a]\"",
            "invalid regex-lite pattern '[z-a]': reversed character range 'z-a'",
        ),
    ];
    for &(name, args, expected) in cases {
        let path = dir.join(format!("{name}.opu"));
        fs::write(&path, input_source(args)).unwrap();
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{name} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(expected),
            "{name}: unexpected diagnostic: {err}"
        );
        assert_error_shape(&err);
        let out = dir.join(format!("{name}.opuir.json"));
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{name}: compile must fail"
        );
        assert!(!out.exists(), "{name}: no IR may be written on error");
    }
}

#[test]
fn all_ten_emitters_preserve_forms_metadata() {
    let dir = out_dir("forms-emit");
    let src = dir.join("forms.opu");
    fs::write(&src, FORMS_SOURCE).unwrap();
    let ir_path = dir.join("forms.opuir.json");
    let r = compile(&src, &ir_path);
    assert!(
        r.status.success(),
        "forms compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let ir_doc: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    let cases: &[(&str, &str)] = &[
        ("native", "src/main.rs"),
        ("web", "ir.json"),
        ("wechat", "pages/index/index.js"),
        ("alipay", "pages/index/index.js"),
        ("qq", "pages/index/index.js"),
        ("douyin", "pages/index/index.js"),
        ("baidu", "pages/index/index.js"),
        ("kuaishou", "pages/index/index.js"),
        ("wasm", "host.js"),
        ("harmony", "src/main/ets/pages/Index.ets"),
    ];
    for &(target, artifact) in cases {
        let out = dir.join(format!("out-{target}"));
        let e = emit(&ir_path, target, &out);
        let stderr = String::from_utf8_lossy(&e.stderr);
        assert!(e.status.success(), "{target} emit failed: {stderr}");
        assert!(!stderr.contains("panicked"), "{target} emitter panicked");
        assert!(out.is_dir(), "{target}: no output directory");
        let text = fs::read_to_string(out.join(artifact))
            .unwrap_or_else(|_| panic!("{target}: missing {artifact}"));
        for marker in [
            "repeat",
            "item",
            "list",
            "items",
            "required",
            "minLength",
            "maxLength",
            "pattern",
            "validate",
            "form",
        ] {
            assert!(
                text.contains(marker),
                "{target}/{artifact} silently dropped '{marker}'"
            );
        }
        match target {
            "native" => assert!(text.contains("@validate"), "native lost validate signal"),
            "web" => {
                let app_js = fs::read_to_string(out.join("app.js")).unwrap();
                assert!(app_js.contains("state[\"@validate\"]"));
                node_check(&out.join("app.js"));
                let html = fs::read_to_string(out.join("index.html")).unwrap();
                assert!(html.contains("data-required=\"true\""));
                assert!(html.contains("data-pattern="));
            }
            "wasm" => {
                assert!(text.contains("Opurom_META"), "wasm lost metadata");
                assert!(text.contains("@validate"), "wasm lost validate signal");
                node_check(&out.join("host.js"));
            }
            "harmony" => {
                assert!(text.contains("Opurom_META_JSON"), "harmony lost metadata");
                assert!(text.contains("@validate"), "harmony lost validate signal");
                assert!(text.contains("OpuromValidator"), "harmony lost validators");
                assert!(
                    text.contains("new OpuromValidator(5, true, 3, 12, "),
                    "harmony lost the Input validator fields"
                );
            }
            _ => {
                assert!(text.contains("Opurom_META"), "{target} lost metadata");
                assert!(text.contains("@validate"), "{target} lost validate signal");
                let markup_path = mini_markup_path(target).unwrap();
                let markup = fs::read_to_string(out.join(markup_path)).unwrap();
                assert!(markup.contains("data-repeat"), "{target} lost repeat attr");
                assert!(
                    markup.contains("data-required"),
                    "{target} lost validator attrs"
                );
                node_check(&out.join("pages/index/index.js"));
            }
        }
    }

    let native_dir = dir.join("out-native");
    let (built, build_log) = build_native(&native_dir);
    assert!(built, "generated native forms crate failed: {build_log}");
    let tap_id = find_tap_id(&ir_doc);
    let (ran, stdout) = run_native(
        &native_dir.join("target/release/opurom_native"),
        &format!("tap {tap_id}\nstate\nquit\n"),
    );
    assert!(ran, "generated native forms app exited nonzero");
    assert!(
        stdout.contains("@validate"),
        "native validate patch missing: {stdout}"
    );
}

#[test]
fn forms_layer_keeps_golden_ir_byte_identical() {
    let dir = out_dir("forms-golden");
    let cases = [
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
        (
            fixture("golden/showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (
            fixture("golden/gallery.opu"),
            fixture("golden/gallery.opuir.json"),
        ),
    ];
    for (index, (src, golden)) in cases.iter().enumerate() {
        if !src.is_file() {
            continue; // standalone checkout: the sibling example repo is optional
        }
        let out = dir.join(format!("golden-{index}.opuir.json"));
        let r = compile(src, &out);
        assert!(
            r.status.success(),
            "{} compile failed: {}",
            src.display(),
            String::from_utf8_lossy(&r.stderr)
        );
        let got = fs::read(&out).expect("read compiled golden IR");
        let expected = fs::read(golden).expect("read byte-level golden IR");
        assert_eq!(got, expected, "{} changed its bytes", golden.display());
        let ir: Json = serde_json::from_slice(&got).unwrap();
        for node in nodes_of(&ir) {
            assert!(node.get("repeat").is_none(), "golden IR gained repeat");
            for key in ["required", "minLength", "maxLength", "pattern"] {
                assert!(node["props"].get(key).is_none(), "golden IR gained {key}");
            }
        }
    }
}

#[test]
fn image_fit_and_radius_lower_to_the_canonical_props() {
    // The native PNG decoder consumes `fit` and `radius`; the source layer must
    // accept them on Image so a project can request cover/contain/fill without
    // hand-editing IR. Documents without them emit no such keys.
    let dir = out_dir("image-fit");
    let src = dir.join("fit.opu");
    fs::write(
        &src,
        r##"app Fitted {
  theme { mode: light, accent: "#097FEB", radius: 8 }
  Column {
    Image(src: "assets/photo1.png", width: 120, height: 60, fit: "cover", rounded: 8)
  }
}
"##,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let image = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Image")
        .expect("an Image node is emitted");
    assert_eq!(image["props"]["fit"], "cover");
    assert_eq!(image["props"]["radius"], 8);

    let plain = dir.join("plain.opu");
    fs::write(
        &plain,
        "app Plain { Column { Image(src: \"x.png\", width: 10, height: 10) } }\n",
    )
    .unwrap();
    let ir2 = compile_ok(&plain);
    let image2 = nodes_of(&ir2)
        .into_iter()
        .find(|n| n["kind"] == "Image")
        .unwrap();
    assert!(
        image2["props"].get("fit").is_none(),
        "no fit without the prop"
    );
}

#[test]
fn grid_compact_columns_lower_as_a_numeric_prop() {
    let dir = out_dir("grid-compact");
    let src = dir.join("grid.opu");
    fs::write(
        &src,
        "app GridApp { Column { Grid(columns: 4, compactColumns: 2) { Text(\"a\") } } }\n",
    )
    .unwrap();
    let doc = compile_ok(&src);
    let grid = nodes_of(&doc)
        .into_iter()
        .find(|n| n["kind"] == "Grid")
        .expect("a Grid node is emitted");
    assert_eq!(grid["props"]["columns"], 4);
    assert_eq!(grid["props"]["compactColumns"], 2);

    let bad = dir.join("bad.opu");
    fs::write(
        &bad,
        "app Bad { Column { Grid(columns: 4, compactColumns: \"two\") { Text(\"a\") } } }\n",
    )
    .unwrap();
    let r = compile(&bad, &dir.join("bad.opuir.json"));
    assert!(
        !r.status.success(),
        "a non-numeric compactColumns must be rejected"
    );
    assert!(String::from_utf8_lossy(&r.stderr).contains("compactColumns"));
}

#[test]
fn icon_size_props_lower_as_numeric_and_opt_in() {
    let dir = out_dir("icon-size");
    let src = dir.join("icons.opu");
    fs::write(
        &src,
        "app Icons {\n  Icon(name: \"star\", width: 48, height: 48)\n  Icon(name: \"home\")\n}\n",
    )
    .unwrap();
    let doc = compile_ok(&src);
    let icons: Vec<&Json> = nodes_of(&doc)
        .into_iter()
        .filter(|n| n["kind"] == "Icon")
        .collect();
    assert_eq!(icons.len(), 2);
    assert_eq!(icons[0]["props"]["name"], "star");
    assert_eq!(icons[0]["props"]["width"], 48);
    assert_eq!(icons[0]["props"]["height"], 48);
    assert!(icons[1]["props"].get("width").is_none());
    assert!(icons[1]["props"].get("height").is_none());

    let out = dir.join("icons.opuir.json");
    let again = dir.join("icons-again.opuir.json");
    assert!(compile(&src, &out).status.success());
    assert!(compile(&src, &again).status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());

    for (key, text) in [
        (
            "width",
            "app Bad {\n  Icon(name: \"star\", width: \"big\")\n}\n",
        ),
        (
            "height",
            "app Bad {\n  Icon(name: \"star\", height: 24.5)\n}\n",
        ),
    ] {
        let bad = dir.join("bad.opu");
        fs::write(&bad, text).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{key}: a non-integer must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(&format!("prop '{key}' expects an integer")),
            "{err}"
        );
        assert_diag(&err, &bad);
    }
}

#[test]
fn text_max_lines_lowers_as_a_numeric_opt_in() {
    let dir = out_dir("text-max-lines");
    let src = dir.join("text.opu");
    fs::write(
        &src,
        "app T {\n  Text(\"a\\nb\", maxLines: 2)\n  Text(\"plain\")\n}\n",
    )
    .unwrap();
    let doc = compile_ok(&src);
    let texts: Vec<&Json> = nodes_of(&doc)
        .into_iter()
        .filter(|n| n["kind"] == "Text")
        .collect();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0]["props"]["maxLines"], 2);
    assert!(texts[1]["props"].get("maxLines").is_none());

    let out = dir.join("text.opuir.json");
    let again = dir.join("text-again.opuir.json");
    assert!(compile(&src, &out).status.success());
    assert!(compile(&src, &again).status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());

    for (label, text) in [
        ("string", "app Bad { Text(\"x\", maxLines: \"two\") }\n"),
        ("float", "app Bad { Text(\"x\", maxLines: 1.5) }\n"),
    ] {
        let bad = dir.join("bad.opu");
        fs::write(&bad, text).unwrap();
        let r = compile(&bad, &dir.join("bad.opuir.json"));
        assert!(!r.status.success(), "{label}: a non-integer must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(err.contains("prop 'maxLines' expects an integer"), "{err}");
        assert_diag(&err, &bad);
    }
}

#[test]
fn mini_program_grids_lower_column_counts_to_percentage_cells() {
    // WXSS/ACSS have no CSS Grid, so every mini-program target must emit a
    // wrapping flex row with equal-width cells. A phone-width page prefers a
    // declared compactColumns over the desktop columns count, and a cell with
    // an explicit width keeps it instead of being overwritten.
    for (target, style_ext) in [
        ("wechat", "wxss"),
        ("alipay", "acss"),
        ("qq", "qss"),
        ("douyin", "ttss"),
        ("baidu", "css"),
        ("kuaishou", "css"),
    ] {
        let dir = out_dir(&format!("mini-grid-{target}"));
        let src = dir.join("grid.opu");
        fs::write(
            &src,
            r##"app GridApp {
  theme { mode: light, accent: "#097FEB", radius: 8 }
  Column {
    Grid(columns: 4, compactColumns: 2) {
      Text("a")
      Text("b")
      Container(width: 40) { Text("c") }
      Text("d")
    }
  }
}
"##,
        )
        .unwrap();
        let ir = dir.join("grid.opuir.json");
        let compiled = compile(&src, &ir);
        assert!(
            compiled.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let out = dir.join("page");
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let css = fs::read_to_string(out.join(format!("pages/index/index.{style_ext}"))).unwrap();
        assert!(
            css.contains(".grid {"),
            "{target} needs the wrapping grid rule"
        );
        assert!(
            css.contains("width: 50.0000%; box-sizing: border-box;"),
            "{target} must honor compactColumns: 2"
        );
        assert_eq!(
            css.matches("width: 50.0000%").count(),
            3,
            "{target}: only the three cells without an explicit width get 50%"
        );
        assert!(
            css.contains("width:40px;"),
            "{target} keeps the authored cell width"
        );
    }
}

#[test]
fn mini_program_pages_paint_the_theme_stage_backdrop() {
    // A glass lamina refracts the flat page backdrop, so every mini-program
    // target must emit `page { background-color: ... }` from theme.tokens.stage.
    for (target, style_ext) in [
        ("wechat", "wxss"),
        ("alipay", "acss"),
        ("qq", "qss"),
        ("douyin", "ttss"),
        ("baidu", "css"),
        ("kuaishou", "css"),
    ] {
        let dir = out_dir(&format!("mini-stage-{target}"));
        let src = dir.join("stage.opu");
        fs::write(
            &src,
            "app Stage { theme { mode: light, accent: \"#0F766E\", radius: 8, stage: \"#EDF0F7\" } Column { Text(\"hi\") } }\n",
        )
        .unwrap();
        let ir = dir.join("stage.opuir.json");
        assert!(compile(&src, &ir).status.success(), "{target} compile");
        let out = dir.join("page");
        assert!(emit(&ir, target, &out).status.success(), "{target} emit");
        let css = fs::read_to_string(out.join(format!("pages/index/index.{style_ext}"))).unwrap();
        assert!(
            css.starts_with("page { background-color: #EDF0F7; }"),
            "{target} must lead with the stage backdrop"
        );
    }
}

#[test]
fn mini_program_stage_falls_back_and_rejects_injected_colours() {
    let dir = out_dir("mini-stage-fallback");
    let src = dir.join("src.opu");
    fs::write(&src, "app Plain { Column { Text(\"hi\") } }\n").unwrap();
    let ir = dir.join("src.opuir.json");
    assert!(compile(&src, &ir).status.success());
    let out = dir.join("out");
    assert!(emit(&ir, "wechat", &out).status.success());
    let css = fs::read_to_string(out.join("pages/index/index.wxss")).unwrap();
    assert!(
        css.starts_with("page { background-color: #ffffff; }"),
        "an IR without tokens falls back to white"
    );

    // A hostile stage must never reach the stylesheet as CSS.
    let evil = dir.join("evil.opuir.json");
    let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap();
    doc["theme"]["tokens"] = serde_json::json!({ "stage": "#FFF; } body { display:none" });
    fs::write(&evil, serde_json::to_vec(&doc).unwrap()).unwrap();
    let out2 = dir.join("out2");
    assert!(emit(&evil, "wechat", &out2).status.success());
    let css2 = fs::read_to_string(out2.join("pages/index/index.wxss")).unwrap();
    assert!(
        css2.starts_with("page { background-color: #ffffff; }"),
        "a non-hex stage is rejected"
    );
    assert!(
        !css2.contains("display:none"),
        "no injected CSS reaches the page"
    );
}

#[test]
fn mini_program_glass_uses_the_normative_lamina_in_both_modes() {
    // the default UI rules framework rules overrides the base glass tokens.
    // Mini-program stylesheets cannot resolve the DOM emitter's custom
    // properties, so the composed v2 channels are inlined at compile time and
    // must match web-dom on every target. Dark swaps only fill and border.
    for (target, style_ext) in [
        ("wechat", "wxss"),
        ("alipay", "acss"),
        ("qq", "qss"),
        ("douyin", "ttss"),
        ("baidu", "css"),
        ("kuaishou", "css"),
    ] {
        let dir = out_dir(&format!("mini-glass-{target}"));
        let src = dir.join("glass.opu");
        fs::write(
            &src,
            "app Glass { theme { mode: light, accent: \"#0F766E\", radius: 8 } Column { Card(glass: true) { Text(\"hi\") } Modal(title: \"sheet\", glass: true) { Text(\"sheet\") } } }\n",
        )
        .unwrap();
        let ir = dir.join("glass.opuir.json");
        assert!(compile(&src, &ir).status.success(), "{target} compile");
        let out = dir.join("page");
        assert!(emit(&ir, target, &out).status.success(), "{target} emit");
        let css = fs::read_to_string(out.join(format!("pages/index/index.{style_ext}"))).unwrap();
        assert!(
            css.contains("backdrop-filter:blur(12px) saturate(140%)"),
            "{target} must use the compact blur"
        );
        assert!(
            css.contains("backdrop-filter:blur(20px) saturate(160%)"),
            "{target} elevated glass must use the desktop blur"
        );
        assert!(
            css.contains("background:rgba(255, 255, 255, 0.42)"),
            "{target} light lamina"
        );
        assert!(
            css.contains("0 2px 8px rgba(17, 24, 39, 0.06), inset 0 1px 0 rgba(255, 255, 255, 0.75), inset 0 -1px 0 rgba(0, 0, 0, 0.06)"),
            "{target} v2 tier2 shadow, highlight and thickness"
        );
        assert!(
            css.contains("border:1px solid rgba(255, 255, 255, 0.5)"),
            "{target} light glass border"
        );
        assert!(
            !css.contains("blur(6px)") && !css.contains("blur(2px) saturate(1.5)"),
            "{target} must not use a legacy frost"
        );

        // A dark theme swaps the lamina and border, leaving blur and edges.
        let dark = dir.join("dark.opuir.json");
        let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap();
        doc["theme"]["mode"] = serde_json::json!("dark");
        fs::write(&dark, serde_json::to_vec(&doc).unwrap()).unwrap();
        let out2 = dir.join("page-dark");
        assert!(
            emit(&dark, target, &out2).status.success(),
            "{target} dark emit"
        );
        let css2 = fs::read_to_string(out2.join(format!("pages/index/index.{style_ext}"))).unwrap();
        assert!(
            css2.contains("background:rgba(30, 35, 46, 0.38)"),
            "{target} dark lamina"
        );
        assert!(
            css2.contains("border:1px solid rgba(255, 255, 255, 0.12)"),
            "{target} dark glass border"
        );
        assert!(
            css2.contains("inset 0 1px 0 rgba(255, 255, 255, 0.75)"),
            "{target} dark keeps the highlight channel"
        );
        assert!(
            css2.contains("backdrop-filter:blur(12px) saturate(140%)"),
            "{target} dark keeps the same blur"
        );
    }
}

#[test]
fn every_emitter_rejects_an_undeclared_event_name() {
    // the IR format declares a closed event-name set. The check lives in the
    // shared read_node so all ten emitters reject a typo instead of silently
    // dropping the binding, matching the web-dom validator.
    let dir = out_dir("unknown-event");
    let ir = dir.join("bad.opuir.json");
    let doc = serde_json::json!({
        "ir": "0.1.0", "app": "Bad", "state": [], "theme": { "mode": "light", "accent": "#097FEB", "radius": 8 },
        "tree": {
            "id": 0, "kind": "Column", "props": {}, "events": {},
            "children": [{
                "id": 1, "kind": "Button", "props": { "label": "x" },
                "events": { "bogusEvent": { "op": "set", "target": "n", "value": 1 } },
                "children": []
            }]
        }
    });
    fs::write(&ir, serde_json::to_vec(&doc).unwrap()).unwrap();
    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let out = dir.join(target);
        let r = emit(&ir, target, &out);
        assert!(
            !r.status.success(),
            "{target} must reject the unknown event name"
        );
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains("unknown event name 'bogusEvent'"),
            "{target}: {err}"
        );
        assert!(
            !out.exists(),
            "{target} must not create an output directory on failure"
        );
    }
}

#[test]
fn emitters_accept_every_declared_event_name() {
    let dir = out_dir("declared-events");
    let ir = dir.join("events.opuir.json");
    let events = serde_json::json!({
        "tap": { "op": "set", "target": "n", "value": 1 },
        "input": { "op": "set", "target": "n" },
        "doubleTap": { "op": "set", "target": "n", "value": 2 },
        "longPress": { "op": "set", "target": "n", "value": 3 },
        "drag": { "op": "set", "target": "n", "value": 4 }
    });
    let doc = serde_json::json!({
        "ir": "0.1.0", "app": "Events",
        "state": [{ "name": "n", "type": "int", "init": 0 }],
        "theme": { "mode": "light", "accent": "#097FEB", "radius": 8 },
        "tree": { "id": 0, "kind": "Button", "props": { "label": "x" }, "events": events, "children": [] }
    });
    fs::write(&ir, serde_json::to_vec(&doc).unwrap()).unwrap();
    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let r = emit(&ir, target, &dir.join(target));
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}

// ------------------------------- design-system node props ------------------

/// One `.opu` document exercising every accepted value of `state`, `variant`
/// and `tone` across the primitives the renderer draws them on, alongside
/// declared state variables. Reused by the lowering, emitter and render tests.
const DESIGN_SOURCE: &str = r#"app DesignProbe {
  state flag = false
  state label = "hi"
  state level = 5
  Column(gap: 6) {
    Button("OK", state: hover, variant: primary, tone: success) { flag.toggle() }
    Text("Caption", state: focus, variant: danger, tone: error)
    Card(state: disabled, variant: ghost, tone: info) { Text("inside") }
    Input(placeholder: "name", bind: label, state: active, variant: secondary, tone: warning)
    Checkbox(bind: flag, state: default, variant: text, tone: neutral)
    Switch(bind: flag, state: hover, variant: fab, tone: success)
    Radio(bind: label, value: "hi", state: active, variant: primary, tone: info)
    Slider(bind: level, min: 0, max: 10, state: focus, variant: secondary, tone: warning)
    Image(src: "pic.png", state: disabled, variant: text, tone: error)
  }
}
"#;

#[test]
fn design_props_lower_under_node_props_and_coexist_with_state() {
    let dir = out_dir("design-lower");
    let src = dir.join("design.opu");
    fs::write(&src, DESIGN_SOURCE).unwrap();
    let ir = compile_ok(&src);

    // The top-level `state` array keeps its documented shape: only the three
    // declared states, each with name/type/init. The `state` node prop is never
    // hoisted into it, and no design value leaks in as a state name.
    let states = ir["state"].as_array().expect("top-level state array");
    let names: Vec<&str> = states.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["flag", "label", "level"]);
    for entry in states {
        assert!(entry.get("name").is_some());
        assert!(entry.get("type").is_some());
        assert!(entry.get("init").is_some());
    }

    // Every drawn primitive carries all three design props under its own
    // `props` map with string values the renderer accepts.
    let drawn = [
        "Button", "Input", "Checkbox", "Switch", "Radio", "Slider", "Card", "Image", "Text",
    ];
    for kind in drawn {
        let node = nodes_of(&ir)
            .into_iter()
            .find(|n| n["kind"] == kind)
            .unwrap_or_else(|| panic!("missing {kind} node"));
        for key in ["state", "variant", "tone"] {
            assert!(
                node["props"][key].is_string(),
                "{kind} lost the '{key}' design prop"
            );
        }
    }
}

#[test]
fn state_prop_does_not_collide_with_state_or_repeat_lowering() {
    // `state` is a top-level keyword for declarations and `for item in
    // state.items`; using it as a node prop must leave both intact.
    let dir = out_dir("design-coexist");
    let src = dir.join("coexist.opu");
    fs::write(
        &src,
        r#"app Coexist {
  state flag = false
  state items = ["a", "b"]
  Column(gap: 4) {
    Button("OK", state: hover, variant: primary, tone: success) { flag.toggle() }
    for item in state.items {
      Text("{item}", state: focus, variant: danger, tone: info)
    }
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);

    let states = ir["state"].as_array().expect("top-level state array");
    assert_eq!(states.len(), 2, "only the two declared states remain");
    assert_eq!(states[0]["name"], "flag");
    assert_eq!(states[1]["type"], "list");

    // The `for` subtree still lowers to a repeat node whose child keeps its
    // design props and its `{item}` interpolation.
    let repeat = nodes_of(&ir)
        .into_iter()
        .find(|n| n.get("repeat").is_some())
        .expect("for/repeat node must survive");
    assert_eq!(repeat["repeat"]["list"], "items");
    let button = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Button")
        .unwrap();
    assert_eq!(button["props"]["state"], "hover");
    assert_eq!(button["props"]["variant"], "primary");
    assert_eq!(button["props"]["tone"], "success");
}

#[test]
fn unknown_design_values_are_rejected_with_help() {
    let dir = out_dir("design-invalid");
    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "state",
            "app Bad { Text(\"x\", state: bogus) }",
            "unknown state value 'bogus'",
            "default, hover, active, focus, disabled, loading, success, error",
        ),
        (
            "variant",
            "app Bad { Button(\"x\", variant: bogus) }",
            "unknown variant value 'bogus'",
            "primary, secondary, text, danger, ghost, fab",
        ),
        (
            "tone",
            "app Bad { Card { Text(\"x\", tone: bogus) } }",
            "unknown tone value 'bogus'",
            "success, warning, error, info, neutral",
        ),
        (
            "type",
            "app Bad { Button(\"x\", tone: 7) }",
            "prop 'tone' expects a keyword, found int",
            "success, warning, error, info, neutral",
        ),
    ];
    for &(name, source, expected, allowed) in cases {
        let path = dir.join(format!("{name}.opu"));
        fs::write(&path, source).unwrap();
        let r = bin().arg("check").arg(&path).output().expect("run check");
        assert_eq!(r.status.code(), Some(1), "{name} must fail");
        let err = String::from_utf8_lossy(&r.stderr);
        assert!(
            err.contains(expected),
            "{name}: unexpected diagnostic: {err}"
        );
        assert!(
            err.contains(allowed),
            "{name}: help must list allowed values: {err}"
        );
        assert_error_shape(&err);
        let out = dir.join(format!("{name}.opuir.json"));
        let capability = compile(&path, &out);
        assert_eq!(
            capability.status.code(),
            Some(1),
            "{name}: compile must fail"
        );
        assert!(!out.exists(), "{name}: no IR may be written on error");
    }
}

#[test]
fn business_states_compile_and_emit_verbatim() {
    // loading/success/error are accepted node-prop states; they must reach IR
    // unchanged so render can parse the eight-state NodeState set.
    let dir = out_dir("business-states");
    let src = dir.join("states.opu");
    fs::write(
        &src,
        r#"app States {
  Column {
    Button("load", state: loading)
    Button("ok", state: success)
    Button("bad", state: error)
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let states: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Button")
        .map(|n| n["props"]["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        vec!["loading", "success", "error"],
        "business states must lower verbatim into node props"
    );

    // The same states must survive emit into every target's design carrier.
    let ir_path = dir.join("states.opuir.json");
    assert!(compile(&src, &ir_path).status.success());
    for (target, file) in [
        ("wechat", "pages/index/index.wxml"),
        ("harmony", "src/main/ets/pages/Index.ets"),
    ] {
        let out = dir.join(format!("emit-{target}"));
        assert!(emit(&ir_path, target, &out).status.success());
        let text = fs::read_to_string(out.join(file)).unwrap();
        for state in ["loading", "success", "error"] {
            assert!(
                text.contains(state),
                "{target}: emitted {state} missing: {text}"
            );
        }
    }
}

#[test]
fn all_ten_emitters_preserve_design_props() {
    let dir = out_dir("design-emit");
    let src = dir.join("design.opu");
    fs::write(&src, DESIGN_SOURCE).unwrap();
    let ir_path = dir.join("design.opuir.json");
    let r = compile(&src, &ir_path);
    assert!(
        r.status.success(),
        "design compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let ir_doc: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();

    // The artifact that carries the node props for each of the ten targets.
    let cases: &[(&str, &str)] = &[
        ("native", "src/main.rs"),
        ("web", "ir.json"),
        ("wechat", "pages/index/index.wxml"),
        ("alipay", "pages/index/index.axml"),
        ("qq", "pages/index/index.qml"),
        ("douyin", "pages/index/index.ttml"),
        ("baidu", "pages/index/index.swan"),
        ("kuaishou", "pages/index/index.ksml"),
        ("wasm", "host.js"),
        ("harmony", "src/main/ets/pages/Index.ets"),
    ];
    for &(target, artifact) in cases {
        let out = dir.join(format!("out-{target}"));
        let e = emit(&ir_path, target, &out);
        let stderr = String::from_utf8_lossy(&e.stderr);
        assert!(e.status.success(), "{target} emit failed: {stderr}");
        assert!(!stderr.contains("panicked"), "{target} emitter panicked");
        let text = fs::read_to_string(out.join(artifact))
            .unwrap_or_else(|_| panic!("{target}: missing {artifact}"));
        for marker in ["state", "variant", "tone", "hover", "primary", "success"] {
            assert!(
                text.contains(marker),
                "{target}/{artifact} silently dropped '{marker}'"
            );
        }
        match target {
            "native" => {
                // The full IR is embedded as an escaped Rust string literal
                // (`\"variant\"`), so the marker loop above is the reliable
                // proof that the three keys and their values survive.
            }
            "web" => {
                assert!(
                    text.contains("\"variant\"") && text.contains("\"tone\""),
                    "web ir.json lost design prop keys"
                );
                node_check(&out.join("app.js"));
            }
            "wasm" => {
                node_check(&out.join("host.js"));
                assert!(text.contains("\"design\""), "wasm lost design metadata");
                assert!(
                    text.contains("variant: \"primary\""),
                    "wasm TREE lost design props"
                );
            }
            "harmony" => {
                assert!(
                    text.contains("new OpuromDesign(")
                        && text.contains("\"primary\"")
                        && text.contains("\"success\""),
                    "harmony lost design metadata"
                );
            }
            _ => {
                node_check(&out.join("pages/index/index.js"));
                assert!(
                    text.contains("data-state=")
                        && text.contains("data-variant=")
                        && text.contains("data-tone="),
                    "{target} markup lost design data attributes"
                );
                let index_js = fs::read_to_string(out.join("pages/index/index.js")).unwrap();
                assert!(
                    index_js.contains("\"design\""),
                    "{target} lost design metadata"
                );
            }
        }
    }

    // The native target still builds and runs with the new props present.
    let native_dir = dir.join("out-native");
    let (built, build_log) = build_native(&native_dir);
    assert!(built, "generated native design crate failed: {build_log}");
    let tap_id = find_tap_id(&ir_doc);
    let (ran, stdout) = run_native(
        &native_dir.join("target/release/opurom_native"),
        &format!("tap {tap_id}\nstate\nquit\n"),
    );
    assert!(ran, "generated native design app exited nonzero");
    assert!(
        stdout.contains("flag=true"),
        "tap handler did not run: {stdout}"
    );
}

#[test]
fn design_props_keep_golden_ir_byte_identical() {
    let dir = out_dir("design-golden");
    let cases = [
        (
            contract("counter.opu"),
            fixture("golden/counter.opuir.json"),
        ),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
        (
            fixture("golden/showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (
            fixture("golden/gallery.opu"),
            fixture("golden/gallery.opuir.json"),
        ),
    ];
    for (index, (src, golden)) in cases.iter().enumerate() {
        if !src.is_file() {
            continue; // standalone checkout: the sibling example repo is optional
        }
        let out = dir.join(format!("golden-{index}.opuir.json"));
        let r = compile(src, &out);
        assert!(
            r.status.success(),
            "{} compile failed: {}",
            src.display(),
            String::from_utf8_lossy(&r.stderr)
        );
        let got = fs::read(&out).expect("read compiled golden IR");
        let expected = fs::read(golden).expect("read byte-level golden IR");
        assert_eq!(got, expected, "{} changed its bytes", golden.display());
        // A program that does not use the new props emits no such keys, and the
        // top-level `state` array keeps exactly its original shape.
        let ir: Json = serde_json::from_slice(&got).unwrap();
        assert!(
            ir["state"].is_array(),
            "{} lost its top-level state array",
            golden.display()
        );
        for node in nodes_of(&ir) {
            for key in ["state", "variant", "tone"] {
                assert!(
                    node["props"].get(key).is_none(),
                    "{} gained {key}",
                    golden.display()
                );
            }
        }
    }
}

/// Remove the three design props from every node of an IR tree.
fn strip_design_props(node: &mut Json) {
    if let Some(props) = node.get_mut("props").and_then(Json::as_object_mut) {
        for key in ["state", "variant", "tone"] {
            props.remove(key);
        }
    }
    if let Some(children) = node.get_mut("children").and_then(Json::as_array_mut) {
        for child in children {
            strip_design_props(child);
        }
    }
}

/// Locate a prebuilt sibling renderer binary, if one is available.
fn sibling_renderer_bin() -> Option<PathBuf> {
    let root = workspace().join("..").join("opurom-render");
    for rel in ["target/release/opurom-render", "target/debug/opurom-render"] {
        let p = root.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            let p = Path::new(dir).join("opurom-render");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[test]
fn design_props_render_end_to_end_with_stable_digest() {
    let Some(renderer) = sibling_renderer_bin() else {
        eprintln!("skip: sibling opurom-render binary is absent");
        return;
    };
    let dir = out_dir("design-render");
    let src = dir.join("design.opu");
    fs::write(&src, DESIGN_SOURCE).unwrap();
    let ir_path = dir.join("design.opuir.json");
    let r = compile(&src, &ir_path);
    assert!(
        r.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );

    let run = |ir: &Path, ppm: &Path| -> (bool, String) {
        let out = Command::new(&renderer)
            .arg(ir)
            .arg("320")
            .arg("240")
            .arg(ppm)
            .output()
            .expect("run opurom-render");
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };
    let a = dir.join("a.ppm");
    let b = dir.join("b.ppm");
    let (ok1, out1) = run(&ir_path, &a);
    let (ok2, out2) = run(&ir_path, &b);
    assert!(ok1 && ok2, "render failed:\n{out1}\n{out2}");
    let d1 = render_digest(&out1);
    let d2 = render_digest(&out2);
    assert_eq!(d1.len(), 16, "missing 16-hex digest:\n{out1}");
    assert_eq!(d1, d2, "rendered digest must be stable across runs");
    assert_eq!(
        fs::read(&a).unwrap(),
        fs::read(&b).unwrap(),
        "rendered pixels must be byte-identical"
    );

    // The renderer must actually draw the props: stripping them changes the frame.
    let mut doc: Json = serde_json::from_slice(&fs::read(&ir_path).unwrap()).unwrap();
    strip_design_props(doc.get_mut("tree").unwrap());
    let base_ir = dir.join("stripped.opuir.json");
    fs::write(&base_ir, serde_json::to_vec(&doc).unwrap()).unwrap();
    let (ok3, out3) = run(&base_ir, &dir.join("c.ppm"));
    assert!(ok3, "stripped render failed:\n{out3}");
    assert_ne!(
        render_digest(&out3),
        d1,
        "design props must change the rendered digest"
    );
}

#[test]
fn route_blocks_lower_to_per_route_trees() {
    let dir = out_dir("routes");
    let src = dir.join("routes.opu");
    fs::write(
        &src,
        r#"app MultiPage {
  state tab = 0
  route Home {
    Column {
      Text("Home")
      Button("Open") { push: Detail }
    }
  }
  route Detail {
    Column {
      Text("Detail")
      Button("Back") { pop }
    }
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    // 0.1.0 stays 0.1.0; multi-page routing is an additive 0.1.0 capability.
    assert_eq!(ir["ir"], "0.1.0");
    assert_eq!(ir["initialRoute"], "Home");
    // No single top-level tree when routes are present.
    assert!(
        ir.get("tree").is_none(),
        "multi-page IR has no top-level tree"
    );
    let routes = ir["routes"].as_object().expect("routes object");
    assert_eq!(routes.len(), 2, "exactly the two declared routes");
    let home = &ir["routes"]["Home"];
    let detail = &ir["routes"]["Detail"];
    assert_eq!(home["title"], "Home");
    assert_eq!(detail["title"], "Detail");
    assert_shape(&home["tree"]);
    assert_shape(&detail["tree"]);
    // Node ids share ONE global pre-order space across every route tree; the
    // runtime dispatches taps by node id alone.
    fn collect_ids(node: &Json, ids: &mut Vec<u64>) {
        ids.push(node["id"].as_u64().expect("numeric node id"));
        for child in node["children"].as_array().unwrap() {
            collect_ids(child, ids);
        }
    }
    let mut ids = Vec::new();
    collect_ids(&home["tree"], &mut ids);
    collect_ids(&detail["tree"], &mut ids);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "node ids must be globally unique");
    assert_eq!(
        ids,
        (0..ids.len() as u64).collect::<Vec<_>>(),
        "ids stay pre-order across routes"
    );
    // `push: Detail` in Home lowers to the documented push action.
    let home_tap = &home["tree"]["children"][1]["events"]["tap"];
    assert_eq!(home_tap, &json!({"op": "push", "target": "Detail"}));
    // `pop` in Detail lowers to the pop action.
    let detail_tap = &detail["tree"]["children"][1]["events"]["tap"];
    assert_eq!(detail_tap, &json!({"op": "pop"}));
}

#[test]
fn program_without_routes_stays_single_page_ir() {
    let dir = out_dir("single-page");
    let src = dir.join("single.opu");
    fs::write(
        &src,
        r#"app Counter {
  state count = 0
  Column(gap: 8) {
    Text("Count: {count}")
    Button("+1") { count += 1 }
  }
}
"#,
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["ir"], "0.1.0");
    assert!(
        ir.get("tree").is_some(),
        "single-page keeps the top-level tree"
    );
    assert!(
        ir.get("routes").is_none(),
        "single-page emits no routes map"
    );
    assert!(ir.get("initialRoute").is_none());
    assert!(
        ir.get("pages").is_none(),
        "retired pages array is not emitted"
    );
}

#[test]
fn duplicate_route_name_is_a_friendly_error() {
    let dir = out_dir("dup-route");
    let src = dir.join("dup.opu");
    fs::write(
        &src,
        r#"app Dup {
  route Home { Text("a") }
  route Home { Text("b") }
}
"#,
    )
    .unwrap();
    let out = dir.join("out.opuir.json");
    let r = compile(&src, &out);
    assert!(!r.status.success(), "duplicate route must fail");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert_diag(&stderr, &src);
    assert!(stderr.contains("duplicate route 'Home'"), "got: {stderr}");
}

#[test]
fn push_to_undeclared_route_is_a_friendly_error() {
    let dir = out_dir("bad-push-route");
    let src = dir.join("badpush.opu");
    fs::write(
        &src,
        r#"app BadPush {
  route Home { Button("x") { push: Nope } }
}
"#,
    )
    .unwrap();
    let out = dir.join("out.opuir.json");
    let r = compile(&src, &out);
    assert!(!r.status.success(), "push to undeclared route must fail");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert_diag(&stderr, &src);
    assert!(stderr.contains("unknown route 'Nope'"), "got: {stderr}");
}

/// A minimal two-route IR matching the additive multi-page contract: no
/// top-level `tree`, an `initialRoute`, and per-route `{tree,title}` records.
fn two_route_ir(name: &str) -> PathBuf {
    let dir = out_dir(name);
    let ir = dir.join("routes.opuir.json");
    let doc = json!({
        "ir": "0.1.0",
        "app": "Shop",
        "state": [],
        "initialRoute": "Home",
        "routes": {
            "Home": {
                "title": "Home Screen",
                "tree": {"id": 1, "kind": "Column", "props": {"gap": 8}, "events": {}, "children": [
                    {"id": 2, "kind": "Text", "props": {"value": "Welcome"}, "events": {}, "children": []},
                    {"id": 3, "kind": "Button", "props": {"label": "Detail"},
                     "events": {"tap": {"op": "push", "target": "Detail"}}, "children": []}
                ]}
            },
            "Detail": {
                "title": "Detail Screen",
                "tree": {"id": 4, "kind": "Column", "props": {"gap": 8}, "events": {}, "children": [
                    {"id": 5, "kind": "Text", "props": {"value": "Item"}, "events": {}, "children": []},
                    {"id": 6, "kind": "Button", "props": {"label": "Back"},
                     "events": {"tap": {"op": "pop"}}, "children": []}
                ]}
            }
        },
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    ir
}

/// Two routes, Feed (pull-refresh) and Detail (subpackage d2), for §7 emission.
fn pull_sub_route_ir(name: &str) -> PathBuf {
    let dir = out_dir(name);
    let ir = dir.join("pr.opuir.json");
    let doc = json!({
        "ir": "0.1.0",
        "app": "PR",
        "state": [{"name": "items", "type": "list", "init": []}],
        "initialRoute": "Feed",
        "routes": {
            "Feed": {
                "title": "Feed",
                "tree": {"id": 1, "kind": "Column", "props": {}, "events": {}, "children": [
                    {"id": 2, "kind": "Text", "props": {"value": "feed"}, "events": {}, "children": []}
                ]},
                "pullRefresh": {"effects": [{"kind": "fetch", "path": "/api/items", "into": "items"}]}
            },
            "Detail": {
                "title": "Detail",
                "tree": {"id": 3, "kind": "Column", "props": {}, "events": {}, "children": [
                    {"id": 4, "kind": "Text", "props": {"value": "detail"}, "events": {}, "children": []}
                ]},
                "subpackage": "d2"
            }
        },
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    ir
}

#[test]
fn mini_size_thresholds_warn_at_2mb_and_error_at_20mb() {
    // F2: a single package above 2 MB is a warning line; above 20 MB total the
    // emit fails. Real bytes: the oversized payload rides in page data.
    let big = "x".repeat(2 * 1024 * 1024 + 512);
    let dir = out_dir("mini-size-warn");
    let ir = dir.join("warn.opuir.json");
    let doc = json!({
        "ir": "0.1.0", "app": "Big", "state": [{"name": "blob", "type": "string", "init": big}],
        "initialRoute": "Home",
        "routes": {"Home": {"title": "Home", "tree": {"id": 1, "kind": "Text",
            "props": {"value": "{blob}"}, "events": {}, "children": []}}},
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let out = dir.join("warn-out");
    let r = emit(&ir, "wechat", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let report = fs::read_to_string(out.join("size-report.txt")).unwrap();
    assert!(
        report.contains("warning: main package"),
        "must warn above 2 MB: {report}"
    );

    // Total above 20 MB fails the emit.
    let huge = "y".repeat(20 * 1024 * 1024 + 512);
    let doc = json!({
        "ir": "0.1.0", "app": "Huge", "state": [{"name": "blob", "type": "string", "init": huge}],
        "initialRoute": "Home",
        "routes": {"Home": {"title": "Home", "tree": {"id": 1, "kind": "Text",
            "props": {"value": "{blob}"}, "events": {}, "children": []}}},
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    let ir = dir.join("huge.opuir.json");
    fs::write(&ir, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let r = emit(&ir, "wechat", &dir.join("huge-out"));
    assert!(!r.status.success(), "total above 20 MB must fail");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("20 MB total limit"), "{stderr}");
}

/// Behavioral harness (checklist C4/D3): load the generated app.js and page js
/// in a Node `vm` sandbox with mocked App/Page/getApp/wx, then drive real
/// handler calls — no string matching.
fn mini_behavioral_harness(out: &Path, request_mode: &str, prefix: &str) -> (bool, String) {
    let harness = out.parent().unwrap().join("mini_behavior.js");
    fs::write(
        &harness,
        r#"const fs = require("fs"), path = require("path"), vm = require("vm");
const out = process.argv[2];
const mode = process.argv[3];
function fail(msg) { console.error("BEHAVIOR FAIL: " + msg); process.exit(1); }
let sandbox = {};
let appInstance = null;
const navUrls = [];
let stopRefresh = 0;
const prefix = process.argv[4] || "wx";
const api = {
  navigateTo: (o) => navUrls.push(o.url),
  navigateBack: (o) => navUrls.push("back:" + (o.delta || 1)),
  request: (o) => {
    if (mode === "fail") o.fail();
    else o.success({ data: mode });
  },
  stopPullDownRefresh: () => { stopRefresh += 1; },
};
sandbox[prefix] = api;
const env = Object.assign({}, sandbox);
env.wx = api;
sandbox = env;
function run(file, extra) {
  const code = fs.readFileSync(path.join(out, file), "utf8");
  const ctx = Object.assign({}, env, {
    App: (c) => { sandbox.appCfg = c; },
    Page: (c) => { sandbox.pageCfg = c; },
    getApp: () => appInstance,
    console,
  }, extra || {});
  sandbox = ctx; vm.createContext(ctx); vm.runInContext(code, ctx);
  return ctx;
}
run("app.js");
appInstance = { globalData: JSON.parse(JSON.stringify(sandbox.appCfg.globalData)) };
function makeInstance(file) {
  run(file);
  const cfg = sandbox.pageCfg;
  const inst = Object.assign({}, cfg, { data: JSON.parse(JSON.stringify(cfg.data)) });
  inst.setData = (patch) => Object.assign(inst.data, patch);
  return inst;
}
const home = makeInstance("pages/home/index.js");
const detail = makeInstance("packages/../pages/detail/index.js".replace("packages/../", ""));
// C1: globalData.states starts at the IR init values.
if (appInstance.globalData.states.n !== 0) fail("C1 n=" + appInstance.globalData.states.n);
// C4 part 1: tap +1 writes through to globalData.
home.onTap2.call(home);
if (appInstance.globalData.states.n !== 1) fail("C4 writeback n=" + appInstance.globalData.states.n);
// B1 behavior: push records the target page url.
home.onTap3.call(home);
if (navUrls[0] !== "pages/detail/index") fail("B1 url=" + navUrls[0]);
// C4 part 2: the pushed page mirrors fresh global state on onShow.
detail.onShow.call(detail);
if (detail.data.n !== 1) fail("C4 freshness n=" + detail.data.n);
if (JSON.stringify(detail.data.items) !== JSON.stringify(["a", "b"]))
  fail("C4 items=" + JSON.stringify(detail.data.items));
// D3: pull-refresh with the mocked request mode.
detail.onPullDownRefresh.call(detail).then(() => {
  if (stopRefresh !== 1) fail("D2 stopPullDownRefresh not called");
  let expected = ["a", "b"];
  if (mode !== "fail") {
    try { expected = JSON.parse(mode); } catch (e) { expected = ["a", "b"]; }
  }
  if (JSON.stringify(detail.data.items) !== JSON.stringify(expected))
    fail("D3 expected " + JSON.stringify(expected) + " got " + JSON.stringify(detail.data.items));
  console.log("BEHAVIOR OK " + mode);
  process.exit(0);
}).catch((e) => fail("async error: " + e));
"#,
    )
    .unwrap();
    let r = Command::new("node")
        .arg(&harness)
        .arg(out)
        .arg(request_mode)
        .arg(prefix)
        .output()
        .expect("run node harness");
    (
        r.status.success(),
        String::from_utf8_lossy(&r.stderr).to_string(),
    )
}

/// C4/D3 behavioral gates over the generated wechat bundle.
#[test]
fn mini_pages_behave_under_a_node_sandbox() {
    let dir = out_dir("mini-behavior");
    let ir = dir.join("behavior.opuir.json");
    let doc = json!({
        "ir": "0.1.0", "app": "Flow",
        "state": [
            {"name": "n", "type": "int", "init": 0},
            {"name": "items", "type": "list", "init": ["a", "b"]}
        ],
        "initialRoute": "Home",
        "routes": {
            "Home": {"title": "Home", "tree": {"id": 1, "kind": "Column", "props": {}, "events": {}, "children": [
                {"id": 2, "kind": "Button", "props": {"label": "+"}, "events": {"tap": {"op": "add", "target": "n", "value": 1}}, "children": []},
                {"id": 3, "kind": "Button", "props": {"label": "Go"}, "events": {"tap": {"op": "push", "target": "Detail"}}, "children": []}
            ]}},
            "Detail": {"title": "Detail", "tree": {"id": 4, "kind": "Column", "props": {}, "events": {}, "children": [
                {"id": 5, "kind": "Text", "props": {"value": "{n}"}, "events": {}, "children": []},
                {"id": 6, "kind": "For", "props": {"source": "items", "item": "item"}, "events": {}, "children": [
                    {"id": 7, "kind": "Text", "props": {"value": "{item}"}, "events": {}, "children": []}
                ]}
            ]}, "pullRefresh": {"effects": [{"kind": "fetch", "path": "/api/items", "into": "items"}]}}
        },
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    for (target, prefix) in [
        ("wechat", "wx"),
        ("alipay", "my"),
        ("qq", "qq"),
        ("douyin", "tt"),
        ("baidu", "swan"),
        ("kuaishou", "ks"),
    ] {
        let out = dir.join(format!("{target}-out"));
        let r = emit(&ir, target, &out);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let (ok, err) = mini_behavioral_harness(&out, "fail", prefix);
        assert!(ok, "{target} fail-mode: {err}");
        let (ok, err) = mini_behavioral_harness(&out, "[\"c\"]", prefix);
        assert!(ok, "{target} success-mode: {err}");
        let (ok, err) = mini_behavioral_harness(&out, "not-json", prefix);
        assert!(ok, "{target} guard-mode: {err}");
        let report = fs::read_to_string(out.join("size-report.txt")).unwrap();
        assert!(report.contains("main package "), "{report}");
    }
}

#[test]
fn mini_pull_refresh_and_subpackage_follow_the_spec() {
    let ir = pull_sub_route_ir("mini-pr-sub");
    let out = out_dir("mini-pr-sub-out");
    let r = emit(&ir, "wechat", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    // D2: onPullDownRefresh runs the effects then stops the animation with the
    // platform API; a failed fetch keeps the old value (no error writeback).
    let feed = fs::read_to_string(out.join("pages/feed/index.js")).unwrap();
    assert!(feed.contains("onPullDownRefresh"), "{feed}");
    assert!(feed.contains("wx.stopPullDownRefresh(); "), "{feed}");
    assert!(feed.contains("/api/items"), "{feed}");
    assert!(feed.contains("fail: function () { resolve(); }"), "{feed}");
    assert!(!feed.contains("error: request failed"), "{feed}");
    // D1: only the declaring page enables pull-down in its page config.
    let feed_cfg: Json =
        serde_json::from_str(&fs::read_to_string(out.join("pages/feed/index.json")).unwrap())
            .unwrap();
    assert_eq!(feed_cfg["enablePullDownRefresh"], true);
    let detail_cfg: Json = serde_json::from_str(
        &fs::read_to_string(out.join("packages/d2/pages/detail/index.json")).unwrap(),
    )
    .unwrap();
    assert!(detail_cfg.get("enablePullDownRefresh").is_none());
    // D4: no onPullDownRefresh on undeclared pages.
    let detail_js = fs::read_to_string(out.join("packages/d2/pages/detail/index.js")).unwrap();
    assert!(!detail_js.contains("onPullDownRefresh"), "{detail_js}");
    // E2/E1: Detail lives under packages/d2; app.json maps root/pages 1:1.
    assert!(
        out.join("packages/d2/pages/detail/index.wxml").exists(),
        "Detail must live under its subpackage root"
    );
    let app_json: Json =
        serde_json::from_str(&fs::read_to_string(out.join("app.json")).unwrap()).unwrap();
    let subs = app_json["subPackages"].as_array().expect("subPackages");
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0]["root"], "packages/d2");
    assert_eq!(subs[0]["pages"], json!(["pages/detail/index"]));
    let main_pages: Vec<&str> = app_json["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // E2: the main package does not list subpackaged routes.
    assert!(main_pages.contains(&"pages/feed/index"));
    assert!(!main_pages.iter().any(|p| p.contains("detail")));
    // F1: "main package <N> B" plus "<name> <N> B" per subpackage.
    let report = fs::read_to_string(out.join("size-report.txt")).expect("size report");
    assert!(report.contains("main package "), "{report}");
    assert!(report.contains("d2 "), "{report}");
    assert!(report.contains("total "), "{report}");

    // E3: the initialRoute must not declare a subpackage.
    let dir = out_dir("mini-pr-bad");
    let bad = dir.join("bad.opuir.json");
    let mut doc: Json = serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap();
    doc["routes"]["Feed"]["subpackage"] = json!("x");
    fs::write(&bad, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let r = emit(&bad, "wechat", &dir.join("bad-out"));
    assert!(!r.status.success());
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("must not declare a subpackage"),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
}

#[test]
fn mini_targets_render_list_for_as_native_loop() {
    // DYNAMIC_LISTS.md Phase B: a runtime For node maps to the platform loop
    // directive, with the item variable bound to the template placeholder.
    let dir = out_dir("mini-wx-for");
    let src = dir.join("wxfor.opu");
    fs::write(
        &src,
        "app MPFor {\n  state items = [\"a\", \"b\"]\n  Column {\n    for (item in items) {\n      Text(\"{item}\")\n    }\n  }\n}\n",
    )
    .unwrap();
    let ir = dir.join("wxfor.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    for (target, ext, ns) in [
        ("wechat", "wxml", "wx"),
        ("alipay", "axml", "a"),
        ("qq", "qml", "qq"),
        ("douyin", "ttml", "tt"),
        ("baidu", "swan", "s"),
        ("kuaishou", "ksml", "ks"),
    ] {
        let out = out_dir(&format!("mini-wx-for-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let markup = fs::read_to_string(out.join(format!("pages/index/index.{ext}"))).unwrap();
        assert!(
            markup.contains(&format!("{ns}:for=\"{{{{items}}}}\"")),
            "{target} loop directive: {markup}"
        );
        assert!(
            markup.contains(&format!("{ns}:for-item=\"item\"")),
            "{target} item var"
        );
        assert!(markup.contains("{{item}}"), "{target} template placeholder");
    }
}

#[test]
fn mini_emitters_emit_one_page_quad_per_route() {
    // PLATFORM_DELIVERY.md section 7 + web-dev checklist A/B/G: one page quad
    // per route, the initial route first and lowercase, push/pop on the
    // platform navigation API, no routeStack/currentRoute simulation.
    let ir = two_route_ir("emit-mp-mini");
    let prefixes = [
        ("wechat", "wx", "wxml", "navigationBarTitleText"),
        ("alipay", "my", "axml", "defaultTitle"),
        ("qq", "qq", "qml", "navigationBarTitleText"),
        ("douyin", "tt", "ttml", "navigationBarTitleText"),
        ("baidu", "swan", "swan", "navigationBarTitleText"),
        ("kuaishou", "ks", "ksml", "navigationBarTitleText"),
    ];
    for (target, nav, ext, title_key) in prefixes {
        let out = out_dir(&format!("emit-mp-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        // A2: one entry per route; A1: the initial route first, lowercase.
        let app_json = fs::read_to_string(out.join("app.json")).unwrap();
        let doc: Json = serde_json::from_str(&app_json).unwrap();
        let pages: Vec<&str> = doc["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(pages, vec!["pages/home/index", "pages/detail/index"]);
        // E4: a bundle without declared subpackages has no subPackages key.
        assert!(doc.get("subPackages").is_none(), "{app_json}");
        // A3: every route has the full quad including a page-level index.json.
        for route in ["home", "detail"] {
            assert!(
                out.join(format!("pages/{route}/index.{ext}")).exists(),
                "{target} missing markup quad for {route}"
            );
            assert!(
                out.join(format!("pages/{route}/index.js")).exists(),
                "{target} missing js quad for {route}"
            );
            assert!(
                out.join(format!("pages/{route}/index.json")).exists(),
                "{target} missing page config for {route}"
            );
        }
        let home_cfg: Json =
            serde_json::from_str(&fs::read_to_string(out.join("pages/home/index.json")).unwrap())
                .unwrap();
        assert_eq!(home_cfg[title_key], "Home Screen");
        assert!(
            home_cfg.get("enablePullDownRefresh").is_none(),
            "D1: undeclared page must not enable pull-down"
        );

        // A4/A5: each page's own markup only, no cross-route node ids.
        let home = fs::read_to_string(out.join(format!("pages/home/index.{ext}"))).unwrap();
        assert!(home.contains("Welcome"), "{target} Home content");
        assert!(
            !home.contains("Item") && !home.contains("n5") && !home.contains("n6"),
            "{target} Home must not carry Detail nodes"
        );
        let detail = fs::read_to_string(out.join(format!("pages/detail/index.{ext}"))).unwrap();
        assert!(detail.contains("Item"), "{target} Detail content");
        assert!(
            !detail.contains("n2") && !detail.contains("n3"),
            "{target} Detail must not carry Home nodes"
        );

        // B1: push lowers to the platform navigate API, lowercase target path.
        let home_js = fs::read_to_string(out.join("pages/home/index.js")).unwrap();
        assert!(
            home_js.contains(&format!(
                "{nav}.navigateTo({{url: \"pages/detail/index\"}})"
            )),
            "{target} push navigate: {home_js}"
        );
        // B3: pop lowers to navigateBack({delta: 1}).
        let detail_js = fs::read_to_string(out.join("pages/detail/index.js")).unwrap();
        assert!(
            detail_js.contains(&format!("{nav}.navigateBack({{delta: 1}})")),
            "{target} pop navigateBack: {detail_js}"
        );
        // B2: no routeStack/currentRoute simulation remains anywhere.
        for js in [&home_js, &detail_js] {
            assert!(!js.contains("routeStack"), "{target} no routeStack");
            assert!(!js.contains("currentRoute"), "{target} no currentRoute");
        }
        // C1: globalData.states seeded with the IR state init values.
        // C2: pages mirror globalData on onShow.
        let app_js = fs::read_to_string(out.join("app.js")).unwrap();
        assert!(app_js.contains("globalData: { states: {} }"), "{app_js}");
        assert!(home_js.contains("onShow"), "{target} onShow mirror");

        let check = Command::new("node")
            .arg("--check")
            .arg(out.join("pages/home/index.js"))
            .output()
            .expect("run node --check");
        assert!(check.status.success(), "{target} Home JS invalid");
    }
}

#[test]
fn web_emitter_draws_active_route_sections() {
    let ir = two_route_ir("emit-mp-web");
    let out = out_dir("emit-mp-web-out");
    let r = emit(&ir, "web", &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(out.join("app.js")).unwrap();
    // One draw function per route, driven by a route stack seeded at initialRoute.
    assert!(app_js.contains("function draw_Home()"), "{app_js}");
    assert!(app_js.contains("function draw_Detail()"), "{app_js}");
    assert!(
        app_js.contains("const routeStack = [\"Home\"];"),
        "{app_js}"
    );
    assert!(app_js.contains("function push(route)"), "push runtime");
    assert!(app_js.contains("function pop()"), "pop runtime");
    assert!(app_js.contains("push(\"Detail\")"), "push action");
    // The multi-page bundle carries no single top-level tree render.
    let check = Command::new("node")
        .arg("--check")
        .arg(out.join("app.js"))
        .output()
        .expect("run node --check");
    assert!(check.status.success(), "web JS invalid");
}

#[test]
fn native_and_wasm_carry_the_route_table() {
    let ir = two_route_ir("emit-mp-native");
    // native: the embedded IR carries routes + initialRoute, and both route
    // taps dispatch a reserved @nav patch (push -> Detail, pop -> @pop).
    let native = out_dir("emit-mp-native-out");
    let r = emit(&ir, "native", &native);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let main_rs = fs::read_to_string(native.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("\\\"initialRoute\\\":\\\"Home\\\""),
        "embedded initialRoute"
    );
    assert!(main_rs.contains("\\\"routes\\\""), "embedded routes");
    assert!(
        main_rs.contains("value: \"Detail\".to_string()"),
        "push patch"
    );
    assert!(main_rs.contains("value: \"@pop\".to_string()"), "pop patch");

    // wasm: the host exposes the ROUTES table and INITIAL_ROUTE and handles pop.
    let wasm = out_dir("emit-mp-wasm-out");
    let r = emit(&ir, "wasm", &wasm);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let host = fs::read_to_string(wasm.join("host.js")).unwrap();
    assert!(host.contains("const ROUTES = "), "ROUTES table");
    assert!(
        host.contains("const INITIAL_ROUTE = \"Home\";"),
        "initial route"
    );
    assert!(
        host.contains("action.op === \"pop\""),
        "pop handling in host"
    );
}

#[test]
fn single_page_ir_still_draws_one_tree() {
    // A 0.1.0 single-page IR keeps its exact single-page output: no routes, one
    // top-level tree, and no route runtime leaks into the generated files.
    let dir = out_dir("emit-single-unchanged");
    let ir = dir.join("single.opuir.json");
    let doc = json!({
        "ir": "0.1.0", "app": "Solo", "state": [],
        "tree": {"id": 1, "kind": "Column", "props": {"gap": 4}, "events": {}, "children": [
            {"id": 2, "kind": "Text", "props": {"value": "hi"}, "events": {}, "children": []}
        ]},
        "theme": {"mode": "light", "accent": "#097FEB", "radius": 12}
    });
    fs::write(&ir, serde_json::to_string(&doc).unwrap()).unwrap();
    let web = out_dir("emit-single-unchanged-web");
    let r = emit(&ir, "web", &web);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        !app_js.contains("routeStack"),
        "no route runtime on single page"
    );
    assert!(!app_js.contains("function draw_"), "no per-route draw");
    let mini = out_dir("emit-single-unchanged-mini");
    let r = emit(&ir, "wechat", &mini);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let js = fs::read_to_string(mini.join("pages/index/index.js")).unwrap();
    assert!(
        !js.contains("routeStack"),
        "no route runtime on single-page mini"
    );
}

#[test]
fn optimizer_keeps_multi_page_ir_intact() {
    let dir = out_dir("routes-optimize");
    let src = dir.join("routes.opu");
    fs::write(
        &src,
        r#"app MultiPage {
  state used = 0
  state dead = 0
  route Home {
    Column {
      Text("Home {used}")
      Button("Open") { push: Detail }
    }
  }
  route Detail {
    Column {
      Text("Detail")
      Button("Back") { pop }
    }
  }
}
"#,
    )
    .unwrap();
    let compiled = dir.join("out.opuir.json");
    let r = compile_raw(&src, &compiled);
    assert!(r.status.success(), "compile failed for optimize input");
    let out = dir.join("optimized.opuir.json");
    let r = optimize(&compiled, &out);
    assert!(r.status.success(), "optimize failed on multi-page IR");
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(ir["initialRoute"], "Home");
    assert!(ir.get("tree").is_none());
    assert!(ir["routes"]["Home"]["tree"].is_object());
    assert!(ir["routes"]["Detail"]["tree"].is_object());
    // Dead-state elimination runs across every route tree: `used` survives
    // (Home reads it) and `dead` is dropped.
    let names: Vec<&str> = ir["state"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["used"]);
    // Node ids stay globally unique across the optimized route trees.
    fn walk(node: &Json, ids: &mut Vec<u64>) {
        ids.push(node["id"].as_u64().unwrap());
        for child in node["children"].as_array().unwrap() {
            walk(child, ids);
        }
    }
    let mut ids = Vec::new();
    walk(&ir["routes"]["Home"]["tree"], &mut ids);
    walk(&ir["routes"]["Detail"]["tree"], &mut ids);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "ids stay unique after optimize");
}

#[test]
fn optimizer_rejects_inconsistent_multi_page_ir() {
    let dir = out_dir("routes-optimize-bad");
    let ir = dir.join("both.opuir.json");
    fs::write(
        &ir,
        r#"{
  "ir": "0.1.0", "app": "Bad", "state": [],
  "tree": {"id": 0, "kind": "Text", "props": {"value": "x"}, "children": [], "events": {}},
  "theme": {},
  "initialRoute": "Home",
  "routes": {"Home": {"tree": {"id": 1, "kind": "Text", "props": {"value": "y"}, "children": [], "events": {}}, "title": "Home"}}
}
"#,
    )
    .unwrap();
    let out = dir.join("out.json");
    let r = optimize(&ir, &out);
    assert!(!r.status.success(), "tree plus routes must be rejected");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("must not also have a top-level 'tree'"),
        "got: {stderr}"
    );

    let ir2 = dir.join("noroute.opuir.json");
    fs::write(
        &ir2,
        r#"{
  "ir": "0.1.0", "app": "Bad", "state": [],
  "theme": {},
  "initialRoute": "Missing",
  "routes": {"Home": {"tree": {"id": 0, "kind": "Text", "props": {"value": "y"}, "children": [], "events": {}}, "title": "Home"}}
}
"#,
    )
    .unwrap();
    let r = optimize(&ir2, &out);
    assert!(!r.status.success(), "unknown initialRoute must be rejected");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("'initialRoute' 'Missing' is not a declared route"),
        "got: {stderr}"
    );
}

// ---------------------------------------------------- theme fontMetrics / tokens ----

#[test]
fn theme_font_metrics_and_tokens_lower_to_ir() {
    let dir = out_dir("theme-metrics");
    let src = dir.join("metrics.opu");
    fs::write(
        &src,
        "app Metrics {\n  theme {\n    fontMetrics: proportional\n    tokens: { success: \"#1FB14C\", warning: \"#FD960E\", error: \"#F62B35\", info: \"#0A71FC\" }\n  }\n  Container { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["theme"]["fontMetrics"], json!("proportional"));
    assert_eq!(ir["theme"]["tokens"]["success"], json!("#1FB14C"));
    assert_eq!(ir["theme"]["tokens"]["warning"], json!("#FD960E"));
    assert_eq!(ir["theme"]["tokens"]["error"], json!("#F62B35"));
    assert_eq!(ir["theme"]["tokens"]["info"], json!("#0A71FC"));

    // The bare `tokens { ... }` form (no colon) parses identically.
    let bare = dir.join("bare.opu");
    fs::write(
        &bare,
        "app Bare {\n  theme { fontMetrics: fixed tokens { success: \"#1FB14C\" } }\n  Container { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&bare);
    assert_eq!(ir["theme"]["fontMetrics"], json!("fixed"));
    assert_eq!(ir["theme"]["tokens"]["success"], json!("#1FB14C"));
}

#[test]
fn theme_tokens_merge_with_surface_stage_behavior() {
    let dir = out_dir("theme-merge");
    let src = dir.join("merge.opu");
    fs::write(
        &src,
        "app Merge {\n  theme { surface: \"#F5F6F8\" stage: \"#0B1220\" tokens: { primary: \"#097FEB\" } }\n  Container { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["theme"]["tokens"]["surface"], json!("#F5F6F8"));
    assert_eq!(ir["theme"]["tokens"]["stage"], json!("#0B1220"));
    assert_eq!(ir["theme"]["tokens"]["primary"], json!("#097FEB"));
    // Absent fontMetrics writes no key, keeping golden IR unchanged.
    assert_eq!(ir["theme"].get("fontMetrics"), None);
}

#[test]
fn theme_font_metrics_rejects_bogus_values() {
    let dir = out_dir("theme-metrics-bad");
    let src = dir.join("bad.opu");
    fs::write(
        &src,
        "app Bad { theme { fontMetrics: elastic } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&src, &dir.join("bad.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("unknown fontMetrics 'elastic'"), "{stderr}");
    assert!(
        stderr.contains("proportional") && stderr.contains("fixed"),
        "{stderr}"
    );
}

#[test]
fn theme_tokens_reject_unknown_names_and_bad_colors() {
    let dir = out_dir("theme-tokens-bad");
    let unknown = dir.join("unknown.opu");
    fs::write(
        &unknown,
        "app A { theme { tokens { danger: \"#FF0000\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&unknown, &dir.join("u.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("unknown theme token 'danger'"), "{stderr}");
    assert!(stderr.contains("success, warning, error, info"), "{stderr}");

    let bad_color = dir.join("badcolor.opu");
    fs::write(
        &bad_color,
        "app A { theme { tokens { success: \"green\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&bad_color, &dir.join("c.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("invalid color 'green' for 'success'"),
        "{stderr}"
    );
}

#[test]
fn input_text_buffer_props_lower_and_unknown_still_rejected() {
    let dir = out_dir("input-text-buffer");
    let src = dir.join("input.opu");
    fs::write(
        &src,
        "app Edit {\n  Input(placeholder: \"Type\", textBuffer: true, secure: true, maxLength: 32, value: \"hi\")\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let node = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == json!("Input"))
        .expect("an Input node");
    assert_eq!(node["props"]["textBuffer"], json!(true));
    assert_eq!(node["props"]["secure"], json!(true));
    assert_eq!(node["props"]["value"], json!("hi"));
    assert_eq!(node["props"]["maxLength"], json!(32));

    let bad = dir.join("bad.opu");
    fs::write(&bad, "app A { Input(placeholder: \"x\", blah: true) }\n").unwrap();
    let r = compile(&bad, &dir.join("b.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("unknown prop 'blah' on Input"), "{stderr}");
    assert!(stderr.contains("textBuffer"), "{stderr}");
}

/// Read the primary artifact each of the ten emitters writes, so the test can
/// assert the new theme keys and Input text-buffer props survive per target.
fn primary_artifact(dir: &Path, target: &str) -> PathBuf {
    match target {
        "native" => dir.join("src/main.rs"),
        "web" => dir.join("ir.json"),
        "wasm" => dir.join("host.js"),
        "wechat" => dir.join("pages/index/index.wxml"),
        "alipay" => dir.join("pages/index/index.axml"),
        "qq" => dir.join("pages/index/index.qml"),
        "douyin" => dir.join("pages/index/index.ttml"),
        "baidu" => dir.join("pages/index/index.swan"),
        "kuaishou" => dir.join("pages/index/index.ksml"),
        "harmony" => dir.join("src/main/ets/pages/Index.ets"),
        _ => panic!("unknown target {target}"),
    }
}

#[test]
fn all_ten_emitters_preserve_theme_keys_and_input_props() {
    let dir = out_dir("emit-theme-input");
    let src = dir.join("app.opu");
    fs::write(
        &src,
        "app Carry {\n  theme {\n    fontMetrics: proportional\n    tokens: { success: \"#1FB14C\", error: \"#F62B35\" }\n  }\n  Input(placeholder: \"Type\", textBuffer: true, secure: true)\n}\n",
    )
    .unwrap();
    let ir = dir.join("app.opuir.json");
    let r = compile(&src, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    for target in [
        "native", "web", "wechat", "alipay", "qq", "douyin", "baidu", "kuaishou", "wasm", "harmony",
    ] {
        let out = out_dir(&format!("emit-theme-input-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target} emit failed: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let artifact = fs::read_to_string(primary_artifact(&out, target)).unwrap();
        // The ten IR emitters must carry the theme keys: embedded IR (native,
        // web) or the mini-program's app.js. wasm is a bonus host-only target.
        if target != "wasm" {
            let theme_carrier = match target {
                "native" | "web" | "harmony" => artifact.clone(),
                _ => fs::read_to_string(out.join("app.js")).unwrap(),
            };
            assert!(
                theme_carrier.contains("proportional"),
                "{target}: theme.fontMetrics missing:\n{theme_carrier}"
            );
            assert!(
                theme_carrier.contains("#1FB14C") && theme_carrier.contains("#F62B35"),
                "{target}: theme.tokens missing:\n{theme_carrier}"
            );
        }
        // textBuffer is a hyphenated data attribute in markup, camelCase in the
        // embedded IR/JS; accept either spelling of the same prop.
        assert!(
            artifact.contains("textBuffer") || artifact.contains("text-buffer"),
            "{target}: Input textBuffer missing:\n{artifact}"
        );
        // Generated JS must be syntactically valid in every target that emits JS.
        let js = match target {
            "native" | "harmony" => None, // Rust and ArkTS are checked elsewhere.
            "web" => Some(out.join("app.js")),
            "wasm" => Some(out.join("host.js")),
            _ => Some(out.join("pages/index/index.js")),
        };
        if let Some(js) = js {
            let check = Command::new("node")
                .arg("--check")
                .arg(&js)
                .output()
                .expect("run node --check");
            assert!(check.status.success(), "{target}: invalid JS {js:?}");
        }
    }
}

#[test]
fn only_referenced_package_icons_are_embedded_in_the_ir() {
    // A project that declares an icon package but uses no package icon must not
    // embed the whole catalog: compile an example that resolves the package and
    // confirm the emitted IR carries no `icons` key (and stays small).
    // Standalone CI checkouts have no sibling example repo; the icon-package
    // contract is verified where the examples are present.
    let todo = example("todo.opu");
    let gallery = example("icons.opu");
    if !todo.is_file() || !gallery.is_file() {
        return;
    }
    let ir = out_dir("icons-unused").join("todo.opuir.json");
    let r = compile(&todo, &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let doc: serde_json::Value = serde_json::from_slice(&fs::read(&ir).unwrap()).unwrap();
    assert!(
        doc.get("icons").is_none(),
        "an icon-free program must not embed the catalog"
    );
    assert!(
        fs::read(&ir).unwrap().len() < 20_000,
        "the IR must not be bloated by unused icons"
    );

    // A program that shows the icon gallery embeds only the icons it references,
    // never the full vendored catalog.
    let gal = out_dir("icons-used").join("icons.opuir.json");
    let r = compile(&gallery, &gal);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let doc: serde_json::Value = serde_json::from_slice(&fs::read(&gal).unwrap()).unwrap();
    let icons = doc
        .get("icons")
        .and_then(|i| i.as_array())
        .expect("icons present when used");
    assert!(
        !icons.is_empty() && icons.len() < 200,
        "only referenced icons ({}) are embedded",
        icons.len()
    );
}

#[test]
fn icon_styles_and_rtl_are_baked_into_package_paths() {
    // A package icon's requested style and RTL variant are resolved at compile
    // time: each use gets its own IR entry under a collision-free variant name,
    // and a missing style degrades to outline with a readable warning.
    let dir = out_dir("icon-variant-bake");
    let pkg = dir.join("registry/icons/1.0.0");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        dir.join("opurom.json"),
        r#"{"name":"p","version":"0.1.0","dependencies":{"icons":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("opurom.lock"),
        r#"{"packages":[{"name":"icons","path":"registry/icons/1.0.0"}]}"#,
    )
    .unwrap();
    fs::write(
        pkg.join("opurom-pkg.json"),
        r#"{"name":"icons","version":"1.0.0","kind":"icons","entry":"icons.json"}"#,
    )
    .unwrap();
    fs::write(
        pkg.join("icons.json"),
        r#"{"icons":[
          {"name":"star","viewBox":"0 0 24 24","paths":[{"d":"M3 12h18"}],"styles":{"outline":[{"d":"M3 12h18"}],"filled":[{"d":"M4 12h16"}]}},
          {"name":"arrow-left","viewBox":"0 0 24 24","paths":[{"d":"M9 5l7 7-7 7"}],"styles":{"outline":[{"d":"M9 5l7 7-7 7"}]}}
        ]}"#,
    )
    .unwrap();

    let src = dir.join("main.opu");
    fs::write(
        &src,
        "app I {\n  Column {\n    Icon(name: \"star\", iconStyle: filled)\n    Icon(name: \"star\")\n    Icon(name: \"star\", flipRtl: true)\n    Icon(name: \"arrow-left\", iconStyle: filled)\n    Icon(name: \"arrow-left\", iconStyle: rounded)\n    Icon(name: \"arrow-left\", flipRtl: true)\n  }\n}\n",
    )
    .unwrap();
    let out = dir.join("main.opuir.json");
    let run = compile(&src, &out);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("warning: icon 'arrow-left' has no filled artwork; outlined is used"),
        "{stderr}"
    );
    assert!(
        stderr.contains("warning: icon 'arrow-left' has no rounded artwork; outlined is used"),
        "{stderr}"
    );
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let table = ir
        .get("icons")
        .and_then(Json::as_array)
        .expect("icons table");
    let paths = |key: &str| -> Vec<String> {
        table.iter().find(|e| e["name"] == key).unwrap()["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(paths("star~filled"), vec!["M4 12h16"]);
    assert_eq!(paths("star"), vec!["M3 12h18"]);
    assert_eq!(paths("star~rtl"), vec!["M21 12h-18"]);
    assert_eq!(paths("arrow-left~filled"), vec!["M9 5l7 7-7 7"]);
    assert_eq!(paths("arrow-left~rounded"), vec!["M9 5l7 7-7 7"]);
    assert_eq!(paths("arrow-left~rtl"), vec!["M15 5l-7 7 7 7"]);
    let used: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Icon")
        .map(|n| n["props"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        used,
        vec![
            "star~filled",
            "star",
            "star~rtl",
            "arrow-left~filled",
            "arrow-left~rounded",
            "arrow-left~rtl",
        ]
    );
    let out2 = dir.join("main2.opuir.json");
    assert!(compile(&src, &out2).status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&out2).unwrap());

    // Web resolves the baked variant by name instead of falling back to a
    // text label, which is what a name-keyed emitter would do otherwise.
    let web = dir.join("web");
    let emitted = emit(&out, "web", &web);
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        js.contains("M4 12h16"),
        "web must draw the baked path: {js}"
    );
    assert!(
        !js.contains("fillText(\"star~filled\""),
        "variant name must not fall back to text: {js}"
    );
}

#[test]
fn corner_radii_are_named_and_additive() {
    let dir = out_dir("corner-radii");
    let src = fixture("golden/corner_radii.opu");
    let out = dir.join("corner_radii.opuir.json");
    let r = compile(&src, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        fs::read(&out).unwrap(),
        fs::read(fixture("golden/corner_radii.opuir.json")).unwrap()
    );
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let props =
        |id: u64| -> &Json { &nodes_of(&ir).into_iter().find(|n| n["id"] == id).unwrap()["props"] };
    assert_eq!(props(1)["cornerRadii"]["topLeft"], 16);
    assert_eq!(props(1)["cornerRadii"]["topRight"], json!([16, 8]));
    assert_eq!(props(1)["cornerRadii"]["bottomRight"], 0);
    assert_eq!(props(1)["cornerRadii"]["bottomLeft"], 12);
    assert_eq!(props(2)["cornerRadii"], json!({"topRight": 28}));
    assert!(props(3).get("cornerRadii").is_none(), "absent stays absent");

    for (src, golden) in [
        (contract("counter.opu"), contract("counter.opuir.json")),
        (
            fixture("golden/gallery.opu"),
            fixture("golden/gallery.opuir.json"),
        ),
        (
            fixture("golden/profile.opu"),
            fixture("golden/profile.opuir.json"),
        ),
        (
            fixture("golden/showcase.opu"),
            fixture("golden/showcase.opuir.json"),
        ),
        (
            fixture("golden/todo.opu"),
            fixture("golden/todo.opuir.json"),
        ),
    ] {
        let plain = dir.join(format!(
            "{}-plain.json",
            src.file_stem().unwrap().to_string_lossy()
        ));
        assert!(compile(&src, &plain).status.success());
        assert_eq!(fs::read(&plain).unwrap(), fs::read(golden).unwrap());
    }
}

#[test]
fn corner_radii_emit_css_in_w3c_order() {
    // W3C `border-radius` order is top-left, top-right, bottom-right,
    // bottom-left; an elliptical pair is horizontal / vertical radii.
    let dir = out_dir("corner-radii-css");
    let src = fixture("golden/corner_radii.opu");
    let ir = dir.join("corner_radii.opuir.json");
    assert!(compile(&src, &ir).status.success());
    let out = dir.join("wechat");
    assert!(emit(&ir, "wechat", &out).status.success());
    let css = fs::read_to_string(out.join("pages/index/index.wxss")).unwrap();

    // Node 1: TL=16, TR=[16,8], BR=0, BL=12. A swapped order or a shared
    // flat value would not produce this exact string.
    assert!(
        css.contains("border-radius:16px 16px 0px 12px / 16px 8px 0px 12px"),
        "four named corners must map to W3C four-value + elliptical slash: {css}"
    );
    // Node 2 declares only topRight; the other corners keep the app radius,
    // which is what the renderer draws for them. The expected value is read
    // back from the compiled IR so the test never assumes the default.
    let theme_radius = serde_json::from_slice::<Json>(&fs::read(&ir).unwrap()).unwrap()["theme"]
        ["radius"]
        .to_string();
    assert!(
        css.contains(&format!(
            "border-radius:{theme_radius}px 28px {theme_radius}px {theme_radius}px"
        )),
        "partial corners fall back to the theme radius: {css}"
    );
    // No JSON object text may leak into CSS.
    assert!(
        !css.contains("topLeft"),
        "named keys must not reach CSS: {css}"
    );

    // A program without cornerRadii emits byte-identical CSS to the scalar path.
    let plain = dir.join("plain.opu");
    fs::write(
        &plain,
        "app R { Column { Container(radius: 8, padding: 0) { Text(\"x\") } } }\n",
    )
    .unwrap();
    let plain_ir = dir.join("plain.opuir.json");
    assert!(compile(&plain, &plain_ir).status.success());
    let plain_out = dir.join("plain-out");
    assert!(emit(&plain_ir, "wechat", &plain_out).status.success());
    let plain_css = fs::read_to_string(plain_out.join("pages/index/index.wxss")).unwrap();
    assert!(
        plain_css.contains(".n1 { padding:0px;border-radius:8px; }"),
        "scalar-only output must stay byte-identical: {plain_css}"
    );
    assert!(
        !plain_css.contains(" / "),
        "scalar output must not use the elliptical slash: {plain_css}"
    );

    // A node-level radius overrides the theme radius for omitted corners, and
    // the named corners still win over both.
    let mixed = dir.join("mixed.opu");
    fs::write(
        &mixed,
        "app M { Column { Container(radius: 6, cornerRadii: { topLeft: 4 }) { Text(\"x\") } } }\n",
    )
    .unwrap();
    let mixed_ir = dir.join("mixed.opuir.json");
    assert!(compile(&mixed, &mixed_ir).status.success());
    let mixed_out = dir.join("mixed-out");
    assert!(emit(&mixed_ir, "wechat", &mixed_out).status.success());
    let mixed_css = fs::read_to_string(mixed_out.join("pages/index/index.wxss")).unwrap();
    assert!(
        mixed_css.contains("border-radius:4px 6px 6px 6px"),
        "named corners win and omitted ones keep the node radius: {mixed_css}"
    );

    // A hand-written IR without a theme radius cannot fill an omitted corner,
    // so the emitter reports it instead of inventing a radius of its own.
    let bare = dir.join("bare.opuir.json");
    fs::write(
        &bare,
        r#"{"app":"X","ir":"0.1.0","state":[],"tree":{"id":0,"kind":"Column","props":{},"events":{},"children":[{"id":1,"kind":"Container","props":{"cornerRadii":{"topLeft":4}},"events":{},"children":[]}]}}"#,
    )
    .unwrap();
    let r = emit(&bare, "wechat", &dir.join("bare-out"));
    assert!(
        !r.status.success(),
        "a cornerRadii node without a radius must fail"
    );
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("a node with cornerRadii needs a radius for its omitted corners"),
        "{stderr}"
    );
}

#[test]
fn corner_radii_reject_arrays_and_bad_corners() {
    let dir = out_dir("corner-radii-errors");
    for (body, needle) in [
        (
            "app X { Container(cornerRadii: [16, 8, 4, 2]) }",
            "cornerRadii rejects an array of corners",
        ),
        (
            "app X { Container(cornerRadii: { topLeft: -1 }) }",
            "a corner radius must be non-negative",
        ),
        (
            "app X { Container(cornerRadii: { foo: 4 }) }",
            "unknown corner 'foo'",
        ),
        (
            "app X { Container(cornerRadii: { topLeft: 16, topLeft: 2 }) }",
            "duplicate corner 'topLeft'",
        ),
        (
            "app X { Container(cornerRadii: 4) }",
            "prop 'cornerRadii' expects a named object",
        ),
    ] {
        let src = dir.join("bad.opu");
        fs::write(&src, body).unwrap();
        let out = dir.join("bad.opuir.json");
        let r = compile(&src, &out);
        assert!(!r.status.success(), "{body} compiled");
        let stderr = String::from_utf8_lossy(&r.stderr);
        assert!(stderr.contains(needle), "{stderr}");
        assert_diag(&stderr, &src);
    }
}

#[test]
fn role_keywords_are_validated_at_compile_time() {
    let dir = out_dir("role-validation");
    let src = dir.join("good.opu");
    fs::write(&src, "app R { Column { Text(\"hi\", role: heading) } }\n").unwrap();
    let ir = compile_ok(&src);
    let roles: Vec<&str> = nodes_of(&ir)
        .into_iter()
        .filter_map(|n| n["props"]["role"].as_str())
        .collect();
    assert_eq!(roles, vec!["heading"]);
    for (body, needle) in [
        (
            "app R { Column { Text(\"hi\", role: alert) } }",
            "unknown role 'alert'",
        ),
        (
            "app R { Column { Text(\"hi\", role: 1) } }",
            "prop 'role' expects a keyword",
        ),
    ] {
        let bad = dir.join("bad.opu");
        fs::write(&bad, body).unwrap();
        let out = dir.join("bad.opuir.json");
        let r = compile(&bad, &out);
        assert!(!r.status.success(), "{body} compiled");
        let stderr = String::from_utf8_lossy(&r.stderr);
        assert!(stderr.contains(needle), "{stderr}");
        assert_diag(&stderr, &bad);
    }
}

#[test]
fn imported_module_may_declare_and_expand_a_component() {
    let dir = out_dir("mod-component");
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(
        dir.join("components/button.opu"),
        "component ActionButton(label: string) {\n  Container(p: 12) { Text(\"{label}\", size: 16) }\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.opu"),
        "import \"components/button.opu\";\napp Demo {\n  Column(gap: 8) {\n    ActionButton(\"Save\")\n    ActionButton(\"Cancel\")\n  }\n}\n",
    )
    .unwrap();
    let out = dir.join("out.opuir.json");
    let r = compile(&dir.join("main.opu"), &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let ir: Json = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(ir["app"], "Demo");
    // Two calls expand into two containers, each with its own substituted label.
    let containers = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Container")
        .count();
    assert_eq!(containers, 2, "each use site expands its component body");
    let labels: Vec<String> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Text")
        .filter_map(|n| n["props"]["value"].as_str().map(str::to_string))
        .collect();
    assert_eq!(labels, vec!["Save".to_string(), "Cancel".to_string()]);
}

#[test]
fn imported_module_with_bare_ui_node_is_rejected() {
    let dir = out_dir("mod-bare-node");
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(dir.join("components/bad.opu"), "Text(\"bare node\")\n").unwrap();
    fs::write(
        dir.join("main.opu"),
        "import \"components/bad.opu\";\napp X {\n  Column() { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let out = dir.join("out.opuir.json");
    let r = compile(&dir.join("main.opu"), &out);
    assert!(!r.status.success(), "bare module node must be rejected");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("may declare only state, model, api, theme and imports"),
        "unexpected error: {stderr}"
    );
}

// ---- theme token import (M6) ----

#[test]
fn token_import_flattens_nested_and_dotted_keys() {
    let dir = out_dir("token-import-flatten");
    fs::create_dir_all(dir.join("theme")).unwrap();
    fs::write(
        dir.join("theme/nested.json"),
        r##"{
  "md": { "sys": { "color": { "primary": "#6750A4" } } },
  "md.sys.typescale.display.large": { "size": 96, "weight": 400 }
}"##,
    )
    .unwrap();
    fs::write(
        dir.join("main.opu"),
        "app M {\n  theme { tokens { import \"theme/nested.json\" } }\n  Container { Text(\"hi\") }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&dir.join("main.opu"));
    let tok = &ir["theme"]["tokens"];
    assert_eq!(tok["md-sys-color-primary"], json!("#6750A4"));
    assert_eq!(tok["md-sys-typescale-display-large-size"], json!("96"));
    assert_eq!(tok["md-sys-typescale-display-large-weight"], json!("400"));
    // Keys are emitted in sorted order.
    let keys: Vec<&str> = tok
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "token keys must be sorted: {keys:?}");
}

#[test]
fn token_import_accepts_nested_objects_and_arrays() {
    let dir = out_dir("token-import-structured");
    fs::write(
        dir.join("tokens.json"),
        r##"{"md": {"elev-0": [], "elev-1": {"umbra": {"offset": [0, 2], "blur": 1}}}}"##,
    )
    .unwrap();
    let src = dir.join("main.opu");
    fs::write(
        &src,
        "app A { theme { tokens { import \"tokens.json\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let tok = &ir["theme"]["tokens"];
    assert_eq!(tok["md-elev-0"], json!("[]"));
    assert_eq!(tok["md-elev-1-umbra-offset"], json!("[0,2]"));
    assert_eq!(tok["md-elev-1-umbra-blur"], json!("1"));
}

#[test]
fn token_import_keeps_golden_ir_unchanged_without_declaration() {
    let golden = compile_ok(&contract("counter.opu"));
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    assert_eq!(
        golden, expected,
        "counter golden IR must stay byte-identical"
    );
}

#[test]
fn token_import_rejects_non_md_namespace_and_bad_leaf() {
    let dir = out_dir("token-import-bad");
    fs::write(dir.join("theme.json"), r##"{"success": "#FF0000"}"##).unwrap();
    let src = dir.join("main.opu");
    fs::write(
        &src,
        "app A { theme { tokens { import \"theme.json\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&src, &dir.join("out.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("must use the 'md-' namespace"), "{stderr}");

    fs::write(dir.join("leaf.json"), r##"{"md": {"flag": true}}"##).unwrap();
    let src2 = dir.join("leaf.opu");
    fs::write(
        &src2,
        "app A { theme { tokens { import \"leaf.json\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&src2, &dir.join("out2.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("must hold a string or number"), "{stderr}");
}

#[test]
fn token_import_rejects_duplicate_against_inline_and_across_files() {
    let dir = out_dir("token-import-dup");
    fs::write(dir.join("a.json"), r##"{"md-sys-color": "#6750A4"}"##).unwrap();
    let inline = dir.join("inline.opu");
    fs::write(
        &inline,
        "app A { theme { tokens { md-sys-color: \"#FF0000\", import \"a.json\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&inline, &dir.join("i.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("duplicate token 'md-sys-color'"),
        "{stderr}"
    );

    fs::write(dir.join("b.json"), r##"{"md-sys-color": "#000000"}"##).unwrap();
    let two = dir.join("two.opu");
    fs::write(
        &two,
        "app A { theme { tokens { import \"a.json\", import \"b.json\" } } Container { Text(\"hi\") } }\n",
    )
    .unwrap();
    let r = compile(&two, &dir.join("t.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("duplicate token 'md-sys-color'"),
        "{stderr}"
    );
}

// ---- native press phase (A3/M4 stdin wiring) ----

fn build_press_binary() -> (PathBuf, u32) {
    let ir = out_dir("native-press-ir").join("counter.opuir.json");
    let r = compile(&contract("counter.opu"), &ir);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let dir = out_dir("native-press");
    let r = emit(&ir, "native", &dir);
    assert!(
        r.status.success(),
        "native emit failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let (built, stderr) = build_native(&dir);
    assert!(built, "cargo build --release failed:\n{stderr}");
    assert!(
        !stderr.contains("warning:"),
        "generated Rust must be warning-free:\n{stderr}"
    );
    let ir_json: Json =
        serde_json::from_str(&fs::read_to_string(&ir).expect("read IR")).expect("IR is valid JSON");
    let press_id = first_positive_add_tap_id(ir_json.get("tree").expect("IR tree"))
        .expect("counter has a positive add tap");
    (dir, press_id)
}

fn run_native_frame(exe: &Path, frame: &Path, stdin: &str) -> (String, Vec<u8>) {
    let mut child = Command::new(exe)
        .arg(frame.to_str().expect("utf8 frame path"))
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
    assert!(
        out.status.success(),
        "native frame render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        fs::read(frame).expect("read rendered frame"),
    )
}

#[test]
fn native_press_on_changes_the_frame() {
    let (dir, press_id) = build_press_binary();
    let exe = dir.join("target/release/opurom_native");
    let (base_stdout, base_bytes) = run_native_frame(&exe, &dir.join("base.ppm"), "quit\n");
    assert!(
        base_stdout.contains("frame: 800x600 wrote"),
        "{base_stdout}"
    );
    let (press_stdout, press_bytes) = run_native_frame(
        &exe,
        &dir.join("press-on.ppm"),
        &format!("press {press_id} on\nquit\n"),
    );
    assert!(
        press_stdout.contains("frame: 800x600 wrote"),
        "{press_stdout}"
    );
    assert_ne!(
        base_bytes, press_bytes,
        "press on must change the rendered frame"
    );
}

#[test]
fn native_without_press_frames_are_byte_identical() {
    let (dir, press_id) = build_press_binary();
    let exe = dir.join("target/release/opurom_native");
    let (_, a) = run_native_frame(&exe, &dir.join("no-a.ppm"), "quit\n");
    let (_, b) = run_native_frame(&exe, &dir.join("no-b.ppm"), "quit\n");
    assert_eq!(a, b, "two no-command runs must be byte-identical");
    let (_, off) = run_native_frame(
        &exe,
        &dir.join("press-off.ppm"),
        &format!("press {press_id} off\nquit\n"),
    );
    assert_eq!(
        a, off,
        "press off must be byte-identical to no press command"
    );
}

// ---- Button textColor (G7, background/foreground separation) ----

#[test]
fn text_color_writes_ir_independent_of_fill() {
    let dir = out_dir("text-color");
    let src = dir.join("tc.opu");
    fs::write(
        &src,
        "app TC {\n  Button(\"Go\", color: \"#112233\", textColor: \"#EEFFFF\")\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["tree"]["props"]["color"], json!("#112233"));
    assert_eq!(ir["tree"]["props"]["textColor"], json!("#EEFFFF"));
}

#[test]
fn text_color_is_generic_across_kinds() {
    let dir = out_dir("text-color-generic");
    let src = dir.join("g.opu");
    fs::write(&src, "app G {\n  Text(\"hi\", textColor: \"#FEDCBA\")\n}\n").unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["tree"]["props"]["textColor"], json!("#FEDCBA"));
}

#[test]
fn design_token_color_emits_semantic_role() {
    let dir = out_dir("design-token-color");
    let src = dir.join("role.opu");
    fs::write(
        &src,
        "app D { Button(\"Go\", color: \"{@md-sys-color-primary}\") }\n",
    )
    .unwrap();
    let roles = [
        "primary",
        "on-primary",
        "surface-container-highest",
        "inverse-primary",
        "background",
    ];
    for role in roles {
        fs::write(
            &src,
            format!("app D {{ Button(\"Go\", color: \"{{@md-sys-color-{role}}}\") }}\n"),
        )
        .unwrap();
        let ir = compile_ok(&src);
        assert_eq!(
            ir["tree"]["props"]["color"],
            json!(format!("@md-sys-color-{role}"))
        );
    }

    let bad = dir.join("bad.opu");
    fs::write(
        &bad,
        "app D { Button(\"Go\", color: \"{@md-sys-color-primaryx}\") }\n",
    )
    .unwrap();
    let r = compile(&bad, &dir.join("bad.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("unsupported design token role 'md-sys-color-primaryx'"),
        "{stderr}"
    );

    // An imported colour token resolves to its literal value: the role name
    // is looked up in the imported token table, not left as a semantic
    // reference the renderer cannot resolve.
    let tokens = dir.join("tokens.json");
    fs::write(&tokens, r##"{"md-sys-color-secondary": "#123456"}"##).unwrap();
    let imported = dir.join("imported.opu");
    fs::write(
        &imported,
        "app I { theme { tokens { import \"tokens.json\" } } Button(\"Go\", color: \"{@md-sys-color-secondary}\") { } }\n",
    )
    .unwrap();
    let ir = compile_ok(&imported);
    assert_eq!(ir["tree"]["props"]["color"], json!("#123456"));
}

#[test]
fn full_m3_token_bundle_imports_as_compile_time_constants() {
    let dir = out_dir("full-m3-token-import");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m3_color_tokens.json");
    let tokens: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture).unwrap()).unwrap();
    let doc = tokens.clone();
    let flat = tokens.clone();
    fs::write(
        dir.join("tokens.json"),
        serde_json::to_string(&flat).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("main.opu"),
        "app M {\n  theme { tokens { import \"tokens.json\" } }\n  Text(\"hi\")\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&dir.join("main.opu"));
    let tok = &ir["theme"]["tokens"];
    let light = &doc;
    assert_eq!(light.as_object().unwrap().len(), 49);
    for (key, value) in light.as_object().unwrap() {
        assert_eq!(tok[key], value.clone(), "{key}");
    }
}

#[test]
fn m3_motion_tokens_compile_to_named_curves() {
    let dir = out_dir("m3-motion-tokens");
    let src = dir.join("motion.opu");
    for name in ["m3-short1", "m3-medium1"] {
        fs::write(
            &src,
            format!("app M {{ Text(\"hi\", animate: \"{name}\") }}\n"),
        )
        .unwrap();
        let ir = compile_ok(&src);
        assert_eq!(ir["tree"]["props"]["animate"], json!(name));
    }
}

#[test]
fn text_color_rejects_non_color_and_non_string() {
    let dir = out_dir("text-color-bad");
    let bad = dir.join("bad.opu");
    fs::write(&bad, "app B { Button(\"Go\", textColor: \"red\") }\n").unwrap();
    let r = compile(&bad, &dir.join("bad.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("invalid color 'red' for 'textColor'"),
        "{stderr}"
    );

    let typ = dir.join("typ.opu");
    fs::write(&typ, "app B { Button(\"Go\", textColor: 255) }\n").unwrap();
    let r = compile(&typ, &dir.join("typ.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("expects a color string"), "{stderr}");
}

#[test]
fn text_color_absent_keeps_golden_ir_unchanged() {
    let golden = compile_ok(&contract("counter.opu"));
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    assert_eq!(
        golden, expected,
        "counter golden IR must stay byte-identical"
    );
}

// ---- text baseline fields (sign-off: baselineKind, no grid gate) ----

#[test]
fn baseline_fields_emit_to_ir() {
    let dir = out_dir("baseline-emit");
    let src = dir.join("b.opu");
    fs::write(
        &src,
        r##"app B {
  theme {
    fontFamily: "Roboto"
    verticalMetrics {
      "Roboto": { ascent: 928, descent: 244, leading: 0 }
      "Noto Sans": { ascent: 880, descent: 120, leading: -20 }
    }
  }
  Text("hi", fontPx: 24, lineHeight: 1.5, baselineKind: "ideographic")
}
"##,
    )
    .unwrap();
    let ir = compile_ok(&src);
    let vm = &ir["theme"]["verticalMetrics"];
    assert_eq!(vm["Roboto"]["ascent"], json!(928));
    assert_eq!(vm["Roboto"]["descent"], json!(244));
    assert_eq!(vm["Noto Sans"]["leading"], json!(-20));
    // Family keys are emitted in sorted order.
    let keys: Vec<&str> = vm.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["Noto Sans", "Roboto"]);
    let props = &ir["tree"]["props"];
    assert_eq!(props["lineHeight"], json!(1.5));
    assert_eq!(props["baselineKind"], json!("ideographic"));

    let none = dir.join("none.opu");
    fs::write(&none, "app Z { Text(\"x\", lineHeight: 0) }\n").unwrap();
    let ir = compile_ok(&none);
    assert_eq!(ir["tree"]["props"]["lineHeight"], json!(0));
}

#[test]
fn baseline_fields_absent_keep_golden_ir_unchanged() {
    let golden = compile_ok(&contract("counter.opu"));
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    assert_eq!(
        golden, expected,
        "counter golden IR must stay byte-identical"
    );
}

#[test]
fn baseline_fields_reject_bad_values() {
    let dir = out_dir("baseline-bad");
    let lh = dir.join("lh.opu");
    fs::write(&lh, "app A { Text(\"x\", lineHeight: 4.0) }\n").unwrap();
    let r = compile(&lh, &dir.join("lh.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("lineHeight must be 0 or between 0.5 and 3.0"),
        "{stderr}"
    );

    let bk = dir.join("bk.opu");
    fs::write(&bk, "app A { Text(\"x\", baselineKind: \"hanging\") }\n").unwrap();
    let r = compile(&bk, &dir.join("bk.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("expects 'alphabetic' or 'ideographic'"),
        "{stderr}"
    );

    let vm = dir.join("vm.opu");
    fs::write(
        &vm,
        "app A { theme { verticalMetrics { \"F\": { ascent: 3000, descent: 200, leading: 0 } } } Text(\"x\") }\n",
    )
    .unwrap();
    let r = compile(&vm, &dir.join("vm.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("verticalMetrics value out of range"),
        "{stderr}"
    );

    let wrong = dir.join("wrong.opu");
    fs::write(
        &wrong,
        "app A { Column(lineHeight: 1.2) { Text(\"x\") } }\n",
    )
    .unwrap();
    let r = compile(&wrong, &dir.join("wrong.json"));
    assert!(!r.status.success());
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("unknown prop 'lineHeight' on Column"),
        "{stderr}"
    );
}

// ---- per-node elevation (M3 six-level whitelist) ----

#[test]
fn elevation_emits_all_six_legal_levels() {
    let dir = out_dir("elevation-levels");
    let src = dir.join("e.opu");
    fs::write(
        &src,
        "app E {\n  Column(elevation: 0) {\n    Text(\"a\", elevation: 1)\n    Text(\"b\", elevation: 2)\n    Text(\"c\", elevation: 3)\n    Text(\"d\", elevation: 4)\n    Text(\"e\", elevation: 6)\n  }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    let children = ir["tree"]["children"].as_array().unwrap();
    let got: Vec<i64> = children
        .iter()
        .map(|n| n["props"]["elevation"].as_i64().unwrap())
        .collect();
    assert_eq!(got, vec![1, 2, 3, 4, 6]);
    assert_eq!(ir["tree"]["props"]["elevation"], json!(0));
}

#[test]
fn elevation_absent_keeps_golden_ir_unchanged() {
    let golden = compile_ok(&contract("counter.opu"));
    let expected: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    assert_eq!(
        golden, expected,
        "counter golden IR must stay byte-identical"
    );
}

#[test]
fn elevation_rejects_values_outside_whitelist() {
    let dir = out_dir("elevation-bad");
    for (label, value, needle) in [
        (
            "five",
            "5",
            "elevation must be one of levels 0,1,2,3,4,6, found 5",
        ),
        (
            "seven",
            "7",
            "elevation must be one of levels 0,1,2,3,4,6, found 7",
        ),
        (
            "eight",
            "8",
            "elevation must be one of levels 0,1,2,3,4,6, found 8",
        ),
        (
            "negative",
            "-1",
            "elevation must be one of levels 0,1,2,3,4,6, found -1",
        ),
        ("string", "\"4\"", "prop 'elevation' expects an integer"),
    ] {
        let src = dir.join(format!("{label}.opu"));
        fs::write(
            &src,
            format!("app B {{ Text(\"x\", elevation: {value}) }}\n"),
        )
        .unwrap();
        let r = compile(&src, &dir.join(format!("{label}.json")));
        assert!(!r.status.success(), "{label} should be rejected");
        let stderr = String::from_utf8_lossy(&r.stderr);
        assert!(stderr.contains(needle), "{label}: {stderr}");
        assert!(stderr.contains("levels 0,1,2,3,4,6"), "{label}: {stderr}");
    }
}

// ---- gradientDirection (renderer already consumes this prop) ----

#[test]
fn gradient_direction_lowers_and_drives_the_sweep_axis() {
    let dir = out_dir("gradient-direction");
    let src = dir.join("g.opu");
    fs::write(
        &src,
        "app G { Column { Container(gradient: \"#097FEB,#101828\", gradientDirection: \"horizontal\") } }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(
        ir["tree"]["children"][0]["props"]["gradientDirection"],
        json!("horizontal")
    );

    // The mini stylesheet and the canvas emitter both sweep along the axis.
    let ir_path = dir.join("g.opuir.json");
    assert!(compile(&src, &ir_path).status.success());
    let wx = dir.join("wx");
    assert!(emit(&ir_path, "wechat", &wx).status.success());
    let css = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(
        css.contains("linear-gradient(to right, #097FEB, #101828)"),
        "horizontal gradient must sweep left to right: {css}"
    );
    let web = dir.join("web");
    assert!(emit(&ir_path, "web", &web).status.success());
    let js = fs::read_to_string(web.join("app.js")).unwrap();
    assert!(
        js.contains("createLinearGradient(16, 16, 16 + 1, 16)"),
        "canvas gradient must run along the horizontal axis: {js}"
    );
}

#[test]
fn gradient_direction_defaults_to_vertical_and_rejects_other_values() {
    let dir = out_dir("gradient-direction-bad");
    let src = dir.join("v.opu");
    fs::write(
        &src,
        "app G { Column { Container(gradient: \"#097FEB,#101828\") } }\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert!(
        ir["tree"]["children"][0]["props"]
            .get("gradientDirection")
            .is_none(),
        "an absent direction must not write a key"
    );
    let ir_path = dir.join("v.opuir.json");
    assert!(compile(&src, &ir_path).status.success());
    let wx = dir.join("wx");
    assert!(emit(&ir_path, "wechat", &wx).status.success());
    let css = fs::read_to_string(wx.join("pages/index/index.wxss")).unwrap();
    assert!(
        css.contains("linear-gradient(to bottom, #097FEB, #101828)"),
        "the default gradient must stay vertical: {css}"
    );

    for (label, value, needle) in [
        (
            "unknown",
            "\"diagonal\"",
            "unknown gradientDirection 'diagonal'",
        ),
        ("number", "2", "prop 'gradientDirection' expects a keyword"),
    ] {
        let bad = dir.join(format!("{label}.opu"));
        fs::write(
            &bad,
            format!("app B {{ Container(gradient: \"#097FEB,#101828\", gradientDirection: {value}) }}\n"),
        )
        .unwrap();
        let r = compile(&bad, &dir.join(format!("{label}.json")));
        assert!(!r.status.success(), "{label} must be rejected");
        let stderr = String::from_utf8_lossy(&r.stderr);
        assert!(stderr.contains(needle), "{label}: {stderr}");
        assert!(
            stderr.contains("vertical or horizontal"),
            "{label}: {stderr}"
        );
    }
}

// ---- default-path folding (P0 compile-time folding and DCE) ----

#[test]
fn default_compile_folds_constants_text_and_unreachable_nodes() {
    let dir = out_dir("default-fold");
    let src = dir.join("f.opu");
    fs::write(
        &src,
        "app Fold {\n  state off = false\n  computed total = 2 * 3 + 4\n  Column {\n    Text(\"{total}\")\n    if (off) { Text(\"gone\") }\n    Text(\"keep\")\n  }\n}\n",
    )
    .unwrap();
    let raw = dir.join("raw.opuir.json");
    let default = dir.join("default.opuir.json");
    assert!(compile_raw(&src, &raw).status.success());
    assert!(compile(&src, &default).status.success());
    let a: Json = serde_json::from_slice(&fs::read(&raw).unwrap()).unwrap();
    let b: Json = serde_json::from_slice(&fs::read(&default).unwrap()).unwrap();

    // numeric constant expression folds to a literal
    assert_eq!(a["computed"][0]["expr"], "2 * 3 + 4");
    assert_eq!(b["computed"][0]["expr"], "10");
    // string interpolation of a constant computed folds inline
    assert_eq!(b["tree"]["children"][0]["props"]["value"], "10");
    // unreachable branch removed; surviving children renumber densely
    assert_eq!(a["tree"]["children"].as_array().unwrap().len(), 3);
    assert_eq!(b["tree"]["children"].as_array().unwrap().len(), 2);
    let ids: Vec<u64> = nodes_of(&b)
        .into_iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1, 2]);
    for node in nodes_of(&b) {
        assert!(node.get("when").is_none(), "constant when must be resolved");
    }
}

#[test]
fn default_folding_keeps_golden_counter_byte_identical() {
    let dir = out_dir("default-fold-golden");
    let out = dir.join("counter.opuir.json");
    assert!(compile(&contract("counter.opu"), &out).status.success());
    assert_eq!(
        fs::read(&out).unwrap(),
        fs::read(contract("counter.opuir.json")).unwrap(),
        "default folds must not drift the golden IR"
    );
    assert!(!out.with_extension("idmap.json").exists());
}

/// The Batch 2 expression operators keep the canonical IR token form: one
/// space between tokens, operators as standalone tokens and a string literal
/// as a single quoted token. Every emitter tokenizes the same text.
#[test]
fn expression_operators_lower_to_canonical_token_text() {
    let dir = out_dir("expr-ops");
    let cases: Vec<(&str, &str)> = vec![
        ("cmp-lt", "a < b"),
        ("cmp-eq", "a == b"),
        ("cmp-ne", "a != b"),
        ("cmp-le", "a <= b"),
        ("cmp-ge", "a >= b"),
        ("cmp-gt", "a > b"),
        ("logic-and", "x && y"),
        ("logic-or", "x || y"),
        ("logic-not", "!flag"),
        ("str-eq", "\"a\" == s"),
    ];
    for (label, expr) in cases {
        let src = dir.join(format!("{label}.opu"));
        fs::write(
            &src,
            format!(
                "app Probe {{\n  state a = 1\n  state b = 2\n  state s = \"\"\n  state x = true\n  state y = false\n  state flag = true\n  computed c = {expr}\n  Column {{\n    Text(\"{{c}}\")\n  }}\n}}\n"
            ),
        )
        .unwrap();
        let ir = compile_ok(&src);
        assert_eq!(ir["computed"][0]["expr"], expr, "{label}: token form");
        assert_eq!(ir["computed"][0]["type"], "bool", "{label}: type");
    }
}

/// Comparison binds looser than arithmetic and tighter than the boolean
/// connectives, so the emitted token text keeps the source grouping.
#[test]
fn expression_operator_precedence_keeps_source_grouping() {
    let dir = out_dir("expr-ops-precedence");
    let src = dir.join("precedence.opu");
    fs::write(
        &src,
        "app Probe {\n  state a = 1\n  state b = 2\n  state x = true\n  state y = false\n  \
         computed c = a < b + 1\n  computed d = x && y || a > b\n  Column {\n    Text(\"{c}\")\n  }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&src);
    assert_eq!(ir["computed"][0]["expr"], "a < b + 1");
    assert_eq!(ir["computed"][1]["expr"], "x && y || a > b");
    assert_eq!(ir["computed"][0]["type"], "bool");
    assert_eq!(ir["computed"][1]["type"], "bool");
}

/// Mixed operand types, string ordering and non-bool logical operands are
/// rejected at the source position instead of being coerced.
#[test]
fn expression_operator_type_errors_are_reported() {
    let dir = out_dir("expr-ops-errors");
    let cases: Vec<(&str, &str, &str)> = vec![
        (
            "mixed.opu",
            "app P { state a = 1 state s = \"\" computed c = a == s Column { Text(\"{c}\") } }",
            "requires operands of the same type",
        ),
        (
            "str-order.opu",
            "app P { state s = \"\" state t = \"\" computed c = s < t Column { Text(\"{c}\") } }",
            "requires int operands",
        ),
        (
            "logic-int.opu",
            "app P { state a = 1 computed c = a && a Column { Text(\"{c}\") } }",
            "requires bool operands",
        ),
        (
            "not-int.opu",
            "app P { state a = 1 computed c = !a Column { Text(\"{c}\") } }",
            "requires a bool operand",
        ),
    ];
    for (name, body, message) in cases {
        let src = dir.join(name);
        fs::write(&src, format!("{body}\n")).unwrap();
        let out = dir.join(format!("{name}.out.json"));
        let r = compile(&src, &out);
        assert!(!r.status.success(), "{name} must fail");
        let stderr = String::from_utf8_lossy(&r.stderr).to_string();
        assert!(
            stderr.contains(message),
            "{name}: unexpected diagnostic: {stderr}"
        );
        assert_diag(&stderr, &src);
    }
}

/// The shared evaluator generates the same comparison, logic and string
/// equality expression on every emitter target, from the canonical IR token
/// text. Bool-typed computed values survive IR load-time type validation.
#[test]
fn computed_expression_operators_emit_across_targets() {
    let dir = out_dir("expr-emit");
    let src = dir.join("probe.opu");
    fs::write(
        &src,
        "app Probe {\n  state a = 1\n  state b = 2\n  state flag = true\n  state s = \"x\"\n  \
         computed lt = a < b\n  computed and = flag && flag\n  computed eq = s == \"x\"\n  \
         Column { Text(\"{lt}\") Text(\"{and}\") Text(\"{eq}\") }\n}\n",
    )
    .unwrap();
    let ir = dir.join("probe.opuir.json");
    assert!(compile(&src, &ir).status.success());

    // IR keeps canonical token text and bool result type; emit validates it.
    let parsed: Json = serde_json::from_slice(&fs::read(&ir).unwrap()).expect("parse compiled IR");
    assert_eq!(parsed["computed"][0]["expr"], "a < b");
    assert_eq!(parsed["computed"][0]["type"], "bool");
    assert_eq!(parsed["computed"][1]["expr"], "flag && flag");
    assert_eq!(parsed["computed"][2]["expr"], "s == \"x\"");

    let cases: Vec<(&str, &str, &str)> = vec![
        ("web", "app.js", "state.lt = (state.a < state.b);"),
        (
            "native",
            "src/main.rs",
            "_state.lt = (_state.a < _state.b);",
        ),
        (
            "wechat",
            "pages/index/index.js",
            "\"lt\": (this.data.a < this.data.b)",
        ),
        (
            "harmony",
            "src/main/ets/pages/Index.ets",
            "this.s4 = (this.s0 < this.s1);",
        ),
    ];
    for (target, artifact, marker) in cases {
        let out = dir.join(format!("out-{target}"));
        let r = emit(&ir, target, &out);
        assert!(
            r.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let file = fs::read_to_string(out.join(artifact))
            .unwrap_or_else(|_| panic!("{target}: missing {artifact}"));
        assert!(
            file.contains(marker),
            "{target}: generated code missing '{marker}'"
        );
        assert!(file.contains("&&"), "{target}: boolean connective missing");
        assert!(
            file.contains("== \"x\""),
            "{target}: string equality missing"
        );
    }
}

/// An IR computed entry whose declared type disagrees with the inferred type
/// of its new-operator expression is rejected at load time, on every emitter.
#[test]
fn computed_expression_operator_type_mismatch_is_rejected() {
    let dir = out_dir("expr-ir-mismatch");
    let base: Json =
        serde_json::from_slice(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap();
    let mut bad = base.clone();
    let mut states = bad["state"].as_array().unwrap().clone();
    states.push(json!({"name": "flag", "type": "int", "init": 0}));
    bad["state"] = json!(states);
    bad["computed"] = json!([{"name": "flag", "type": "int", "expr": "1 < 2"}]);
    let ir = dir.join("mismatch.opuir.json");
    fs::write(&ir, serde_json::to_vec(&bad).unwrap()).unwrap();
    for target in ["web", "native", "wechat", "wasm"] {
        let out = dir.join(format!("out-{target}"));
        let r = emit(&ir, target, &out);
        assert!(!r.status.success(), "{target} must reject type mismatch");
        let stderr = String::from_utf8_lossy(&r.stderr).to_string();
        assert!(
            stderr.contains("has type 'int' but its expression is bool"),
            "{target}: unexpected diagnostic: {stderr}"
        );
    }
}

/// A component use-site block used to be dropped with no diagnostic. It is
/// now adopted into every handler the component declares, appended after the
/// component's own actions in source order. A use site without a block is
/// byte-identical to the same component expanded alone.
#[test]
fn component_use_site_block_is_adopted_into_handlers() {
    let dir = out_dir("component-use-block");

    let adopted = dir.join("adopted.opu");
    fs::write(
        &adopted,
        "component TapButton(label: string) {\n  Button(\"{label}\") { }\n}\n\
         app T {\n  state count = 0\n  TapButton(\"Go\") { count += 1 }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&adopted);
    let buttons = nodes_of(&ir);
    let btn = buttons
        .iter()
        .find(|n| n["kind"] == "Button")
        .copied()
        .expect("expanded Button");
    assert_eq!(
        btn["events"]["tap"],
        json!({"op": "add", "target": "count", "value": 1}),
        "use-site statement must reach the handler"
    );

    // Two handlers both receive the block; the component's own actions come
    // first, the caller's statements are appended after in source order.
    let multi = dir.join("multi.opu");
    fs::write(
        &multi,
        "component Pair(label: string) {\n  Button(\"{label}\") { count += 10 }\n  Button(\"Second\") { }\n}\n\
         app T {\n  state count = 0\n  Pair(\"First\") { count += 1 }\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&multi);
    let taps: Vec<Json> = nodes_of(&ir)
        .into_iter()
        .filter(|n| n["kind"] == "Button")
        .map(|n| n["events"]["tap"].clone())
        .collect();
    assert_eq!(taps.len(), 2, "both handlers expanded");
    assert_eq!(
        taps[0],
        json!({"actions": [
            {"op": "add", "target": "count", "value": 10},
            {"op": "add", "target": "count", "value": 1}
        ]}),
        "caller statements appended after the component's own, in order"
    );
    assert_eq!(taps[1], json!({"op": "add", "target": "count", "value": 1}));

    // No block: the expanded handler stays empty, so the default path is
    // unchanged and the counter golden still compiles deep-equal.
    let plain = dir.join("plain.opu");
    fs::write(
        &plain,
        "component TapButton(label: string) {\n  Button(\"{label}\") { }\n}\n\
         app T {\n  state count = 0\n  TapButton(\"Go\")\n}\n",
    )
    .unwrap();
    let ir = compile_ok(&plain);
    let btn = nodes_of(&ir)
        .into_iter()
        .find(|n| n["kind"] == "Button")
        .expect("expanded Button");
    assert_eq!(btn["events"], json!({}), "no block means no handler");

    let out = out_dir("component-use-block-golden").join("counter.opuir.json");
    assert!(compile(&contract("counter.opu"), &out).status.success());
    assert_eq!(
        serde_json::from_slice::<Json>(&fs::read(&out).unwrap()).unwrap(),
        serde_json::from_slice::<Json>(&fs::read(contract("counter.opuir.json")).unwrap()).unwrap(),
        "counter golden unchanged by use-site block adoption"
    );
}
