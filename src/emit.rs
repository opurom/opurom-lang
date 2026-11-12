use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};
use crate::lower::{interp_parts, offset_pos, Part};
use crate::parser::Pos;

pub const TARGETS: &[(&str, &str)] = &[
    ("native", "Cargo.toml, src/main.rs"),
    ("web", "index.html, app.js, ir.json"),
    ("wechat", "app.json, index.wxml, index.wxss, index.js"),
    ("alipay", "app.json, index.axml, index.acss, index.js"),
    ("qq", "app.json, index.qml, index.qss, index.js"),
    ("douyin", "app.json, index.ttml, index.ttss, index.js"),
    ("baidu", "app.json, index.swan, index.css, index.js"),
    ("kuaishou", "app.json, index.ksml, index.css, index.js"),
    (
        "wasm",
        "Cargo.toml, src/lib.rs, index.html, host.js, README.md",
    ),
];

#[derive(Clone)]
pub struct EState {
    pub name: String,
    pub ty: String,
    pub init: Json,
}

#[derive(Clone)]
pub struct ENode {
    pub id: u64,
    pub kind: String,
    pub props: JsonMap<String, Json>,
    pub events: JsonMap<String, Json>,
    pub children: Vec<ENode>,
    /// Optional bool state that hides this node and its subtree.
    pub when: Option<String>,
}

#[derive(Clone, Copy)]
struct Box2 {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct Platform {
    key: &'static str,
    markup_ext: &'static str,
    style_ext: &'static str,
    for_attr: &'static str,
    cond_attr: &'static str,
    key_attr: &'static str,
    tap_attr: &'static str,
    input_attr: &'static str,
    state_set: &'static str,
    title_key: &'static str,
    open: &'static str,
    close: &'static str,
}

const PLATFORMS: &[Platform] = &[
    Platform {
        key: "wechat",
        cond_attr: "wx:if",
        markup_ext: "wxml",
        style_ext: "wxss",
        for_attr: "wx:for",
        key_attr: "wx:key",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "alipay",
        cond_attr: "a:if",
        markup_ext: "axml",
        style_ext: "acss",
        for_attr: "a:for",
        key_attr: "a:key",
        tap_attr: "onTap",
        input_attr: "onInput",
        state_set: "setData",
        title_key: "defaultTitle",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "qq",
        cond_attr: "qq:if",
        markup_ext: "qml",
        style_ext: "qss",
        for_attr: "qq:for",
        key_attr: "qq:key",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "douyin",
        cond_attr: "tt:if",
        markup_ext: "ttml",
        style_ext: "ttss",
        for_attr: "tt:for",
        key_attr: "tt:key",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "baidu",
        cond_attr: "s-if",
        markup_ext: "swan",
        style_ext: "css",
        for_attr: "s-for",
        key_attr: "s-key",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "kuaishou",
        cond_attr: "ks:if",
        markup_ext: "ksml",
        style_ext: "css",
        for_attr: "ks:for",
        key_attr: "ks:key",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
];

pub fn bad_ir(ir_path: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::file(
        ir_path,
        message,
        "compile the .opu source with 'opuromc compile' to produce valid IR v0.1",
    )
}

fn read_node(v: &Json, ir_path: &str) -> DResult<ENode> {
    let o = v
        .as_object()
        .ok_or_else(|| bad_ir(ir_path, "IR tree node must be an object"))?;
    let id = o
        .get("id")
        .and_then(Json::as_u64)
        .ok_or_else(|| bad_ir(ir_path, "IR tree node is missing a numeric 'id'"))?;
    let kind = o
        .get("kind")
        .and_then(Json::as_str)
        .ok_or_else(|| bad_ir(ir_path, "IR tree node is missing a string 'kind'"))?
        .to_string();
    let props = o
        .get("props")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let events = o
        .get("events")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let child_values = o
        .get("children")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let mut children = Vec::new();
    for c in &child_values {
        children.push(read_node(c, ir_path)?);
    }
    let when = o.get("when").and_then(Json::as_str).map(str::to_string);
    Ok(ENode {
        id,
        kind,
        props,
        events,
        children,
        when,
    })
}

fn read_state(arr: &[Json], ir_path: &str) -> DResult<Vec<EState>> {
    let mut out = Vec::new();
    for v in arr {
        let o = v
            .as_object()
            .ok_or_else(|| bad_ir(ir_path, "IR state entry must be an object"))?;
        let name = o
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "IR state entry is missing 'name'"))?
            .to_string();
        let ty = o
            .get("type")
            .and_then(Json::as_str)
            .unwrap_or("string")
            .to_string();
        let init = o.get("init").cloned().unwrap_or(Json::Null);
        out.push(EState { name, ty, init });
    }
    Ok(out)
}

pub fn collect<'a>(n: &'a ENode, out: &mut Vec<&'a ENode>) {
    out.push(n);
    for c in &n.children {
        collect(c, out);
    }
}

fn prop_num(props: &JsonMap<String, Json>, key: &str, default: f64) -> f64 {
    props.get(key).and_then(Json::as_f64).unwrap_or(default)
}

fn prop_str(props: &JsonMap<String, Json>, key: &str, default: &str) -> String {
    match props.get(key) {
        Some(Json::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => default.to_string(),
    }
}

fn js_lit(v: &Json) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
}

fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

fn html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markup_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn fmt_num(v: f64) -> String {
    format!("{v:.0}")
}

pub fn write(out: &Path, name: &str, content: &str, ir_path: &str) -> DResult<()> {
    let p = out.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Diagnostic::file(
                ir_path,
                format!("cannot create {}: {e}", parent.display()),
                "check the output directory path and permissions",
            )
        })?;
    }
    fs::write(&p, content).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot write {}: {e}", p.display()),
            "check the output directory path and permissions",
        )
    })
}

fn action_str<'a>(action: &'a Json, key: &str, ir_path: &str) -> DResult<&'a str> {
    action
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| bad_ir(ir_path, format!("event action is missing string '{key}'")))
}

/// An event is either one action object (v0.1) or `{"actions":[...]}` (R2).
pub fn event_actions(event: &Json) -> Vec<&Json> {
    match event.get("actions").and_then(Json::as_array) {
        Some(list) => list.iter().collect(),
        None => vec![event],
    }
}

fn interp_error(s: &str, off: usize, msg: String, ir_path: &str) -> Diagnostic {
    let p = offset_pos(Pos { line: 1, col: 1 }, s, off);
    Diagnostic::new(
        ir_path,
        p.line,
        p.col,
        msg,
        "fix the interpolation in the .opu source and recompile",
    )
}

pub fn emit(target: &str, ir_text: &str, ir_path: &str, out: &Path) -> DResult<()> {
    let ir: Json = serde_json::from_str(ir_text).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("invalid IR JSON: {e}"),
            "compile a .opu file with 'opuromc compile' to produce valid IR",
        )
    })?;
    match ir.get("ir").and_then(Json::as_str) {
        Some("0.1") => {}
        Some(v) => {
            return Err(Diagnostic::file(
                ir_path,
                format!("unsupported IR version '{v}', expected '0.1'"),
                "recompile the source with this version of opuromc",
            ))
        }
        None => {
            return Err(Diagnostic::file(
                ir_path,
                "missing 'ir' version field",
                "IR v0.1 must contain \"ir\": \"0.1\"",
            ))
        }
    }
    let known = TARGETS.iter().any(|(n, _)| *n == target);
    if !known {
        return Err(Diagnostic::file(
            ir_path,
            format!("unknown target '{target}'"),
            format!(
                "choose one of: {}",
                TARGETS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let app = ir
        .get("app")
        .and_then(Json::as_str)
        .unwrap_or("App")
        .to_string();
    let state_values = ir
        .get("state")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let tree_value = ir
        .get("tree")
        .ok_or_else(|| bad_ir(ir_path, "IR is missing the 'tree' field"))?;
    let states = read_state(&state_values, ir_path)?;
    let tree = read_node(tree_value, ir_path)?;
    fs::create_dir_all(out).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot create output directory {}: {e}", out.display()),
            "check the -o path and permissions",
        )
    })?;
    match target {
        "native" => emit_native(&app, &states, &tree, out, ir_path),
        "web" => emit_web(&ir, &app, &states, &tree, out, ir_path),
        "wasm" => crate::wasm::emit_wasm(&app, &states, &tree, out, ir_path),
        _ => emit_mini(target, &app, &states, &tree, out, ir_path),
    }
}

// ---------------------------------------------------------------- native ----

fn native_manifest() -> String {
    concat!(
        "[package]\n",
        "name = \"opurom_native\"\n",
        "version = \"0.1.0\"\n",
        "edition = \"2021\"\n\n",
        "[workspace]\n\n",
        "[dependencies]\n",
    )
    .to_string()
}

fn rust_ty(ty: &str) -> &'static str {
    match ty {
        "int" => "i64",
        "bool" => "bool",
        _ => "String",
    }
}

fn rust_init(st: &EState) -> String {
    match st.ty.as_str() {
        "int" => format!("{}i64", st.init.as_i64().unwrap_or(0)),
        "bool" => st.init.as_bool().unwrap_or(false).to_string(),
        _ => format!("{}.to_string()", rust_str(st.init.as_str().unwrap_or(""))),
    }
}

fn state_ty<'a>(states: &'a [EState], name: &str) -> &'a str {
    states
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.ty.as_str())
        .unwrap_or("string")
}

fn patch_value(states: &[EState], target: &str) -> String {
    if state_ty(states, target) == "string" {
        format!("_state.{target}.clone()")
    } else {
        format!("_state.{target}.to_string()")
    }
}

fn native_action(action: &Json, states: &[EState], ir_path: &str) -> DResult<(String, String)> {
    let op = action_str(action, "op", ir_path)?;
    let target = action_str(action, "target", ir_path)?;
    let patch = format!(
        "Patch {{ name: {}, value: {} }}",
        rust_str(target),
        patch_value(states, target)
    );
    let mutation = match op {
        "add" => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("_state.{from}"),
                None => {
                    let n = action
                        .get("value")
                        .and_then(Json::as_i64)
                        .ok_or_else(|| bad_ir(ir_path, "'add' action needs an integer 'value'"))?;
                    format!("{n}i64")
                }
            };
            format!("_state.{target} = _state.{target}.saturating_add({rhs});")
        }
        "toggle" => format!("_state.{target} = !_state.{target};"),
        "set" => match action.get("valueFrom").and_then(Json::as_str) {
            Some(from) => match state_ty(states, target) {
                "int" => format!("_state.{target} = _state.{from};"),
                "bool" => format!("_state.{target} = _state.{from};"),
                _ => format!("_state.{target} = _state.{from}.clone();"),
            },
            None => match action.get("value") {
                Some(v) => {
                    let rhs = match state_ty(states, target) {
                        "int" => format!("{}i64", v.as_i64().unwrap_or(0)),
                        "bool" => v.as_bool().unwrap_or(false).to_string(),
                        _ => format!("{}.to_string()", rust_str(v.as_str().unwrap_or(""))),
                    };
                    format!("_state.{target} = {rhs};")
                }
                None => match state_ty(states, target) {
                    "int" => format!(
                        "_state.{target} = _input.unwrap_or(\"\").parse::<i64>().unwrap_or(0);"
                    ),
                    "bool" => format!("_state.{target} = _input.unwrap_or(\"\") == \"true\";"),
                    _ => format!("_state.{target} = _input.unwrap_or(\"\").to_string();"),
                },
            },
        },
        _ => return Err(bad_ir(ir_path, format!("unknown event op '{op}'"))),
    };
    Ok((mutation, patch))
}

fn native_source(app: &str, states: &[EState], tree: &ENode, ir_path: &str) -> DResult<String> {
    let mut s = String::new();
    s.push_str(&format!(
        "// Generated by opuromc for app {app}; lowered from IR v0.1. Do not edit.\n"
    ));
    s.push_str("use std::io::{self, BufRead, Write};\n\n");
    s.push_str("struct State {\n");
    for st in states {
        s.push_str(&format!("    {}: {},\n", st.name, rust_ty(&st.ty)));
    }
    s.push_str("}\n\nstruct Patch {\n    name: &'static str,\n    value: String,\n}\n\n");
    s.push_str(
        "fn apply(_state: &mut State, event_node_id: u32, _input: Option<&str>) -> Vec<Patch> {\n",
    );
    s.push_str("    match (event_node_id, _input.is_some()) {\n");
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    for n in &nodes {
        for (event, is_input) in [("tap", false), ("input", true)] {
            if let Some(action) = n.events.get(event) {
                let mut body = String::new();
                let mut patches = Vec::new();
                for one in event_actions(action) {
                    let (mutation, patch) = native_action(one, states, ir_path)?;
                    body.push_str(&format!("            {mutation}\n"));
                    patches.push(patch);
                }
                let patch_expr = if patches.is_empty() {
                    "Vec::new()".to_string()
                } else {
                    format!("vec![{}]", patches.join(", "))
                };
                s.push_str(&format!(
                    "        ({}, {}) => {{\n{body}            {patch_expr}\n        }}\n",
                    n.id, is_input
                ));
            }
        }
    }
    s.push_str("        _ => Vec::new(),\n    }\n}\n\n");
    if states.is_empty() {
        s.push_str("fn print_state(_state: &State) {\n    println!(\"state (empty)\");\n}\n\n");
    } else {
        let fmt = states
            .iter()
            .map(|st| format!("{}={{}}", st.name))
            .collect::<Vec<_>>()
            .join(" ");
        let args = states
            .iter()
            .map(|st| format!("state.{}", st.name))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!(
            "fn print_state(state: &State) {{\n    println!(\"state {fmt}\", {args});\n}}\n\n"
        ));
    }
    s.push_str("fn main() {\n    let mut state = State {\n");
    for st in states {
        s.push_str(&format!("        {}: {},\n", st.name, rust_init(st)));
    }
    s.push_str("    };\n");
    s.push_str(concat!(
        "    let stdin = io::stdin();\n",
        "    for line in stdin.lock().lines() {\n",
        "        let line = match line { Ok(l) => l, Err(_) => break };\n",
        "        let parts: Vec<&str> = line.split_whitespace().collect();\n",
        "        if parts.is_empty() { continue; }\n",
        "        match parts[0] {\n",
        "            \"quit\" => break,\n",
        "            \"state\" => print_state(&state),\n",
        "            \"tap\" | \"input\" => {\n",
        "                if parts.len() < 2 {\n",
        "                    println!(\"error: usage: {} <id> [text]\", parts[0]);\n",
        "                    continue;\n",
        "                }\n",
        "                let id = match parts[1].parse::<u32>() {\n",
        "                    Ok(id) => id,\n",
        "                    Err(_) => { println!(\"error: invalid node id {:?}\", parts[1]); continue; }\n",
        "                };\n",
        "                let text = parts[2..].join(\" \");\n",
        "                let patches = if parts[0] == \"input\" {\n",
        "                    apply(&mut state, id, Some(text.as_str()))\n",
        "                } else {\n",
        "                    apply(&mut state, id, None)\n",
        "                };\n",
        "                print_state(&state);\n",
        "                for p in patches { println!(\"patch {}={}\", p.name, p.value); }\n",
        "            }\n",
        "            other => println!(\"error: unknown command {:?}\", other),\n",
        "        }\n",
        "        let _ = io::stdout().flush();\n",
        "    }\n",
        "}\n",
    ));
    Ok(s)
}

fn emit_native(
    app: &str,
    states: &[EState],
    tree: &ENode,
    out: &Path,
    ir_path: &str,
) -> DResult<()> {
    write(out, "Cargo.toml", &native_manifest(), ir_path)?;
    let src = native_source(app, states, tree, ir_path)?;
    write(out, "src/main.rs", &src, ir_path)
}

// -------------------------------------------------------------------- web ----

fn intrinsic(n: &ENode) -> (f64, f64) {
    match n.kind.as_str() {
        "Text" => {
            let size = prop_num(&n.props, "size", 16.0);
            let len = prop_str(&n.props, "value", "").chars().count() as f64;
            (len * size * 0.6 + 4.0, size * 1.5)
        }
        "Button" => {
            let len = prop_str(&n.props, "label", "").chars().count() as f64;
            (len * 10.0 + 28.0, 32.0)
        }
        "Input" => (200.0, 32.0),
        "Image" => (
            prop_num(&n.props, "width", 100.0),
            prop_num(&n.props, "height", 100.0),
        ),
        "Spacer" => (
            prop_num(&n.props, "width", 0.0),
            prop_num(&n.props, "height", 0.0),
        ),
        _ => (0.0, 0.0),
    }
}

fn layout(n: &ENode, x: f64, y: f64, boxes: &mut HashMap<u64, Box2>) -> Box2 {
    let pad = prop_num(&n.props, "padding", 0.0);
    let gap = prop_num(&n.props, "gap", 4.0);
    let (iw, ih) = intrinsic(n);
    let vertical = n.kind == "Column";
    let horizontal = n.kind == "Row" || n.kind == "Container";
    let b = if !n.children.is_empty() && (vertical || horizontal) {
        let mut cx = x + pad;
        let mut cy = y + pad;
        let mut max_w = 0.0f64;
        let mut max_h = 0.0f64;
        for child in &n.children {
            let cb = layout(child, cx, cy, boxes);
            max_w = max_w.max(cb.w);
            max_h = max_h.max(cb.h);
            if vertical {
                cy += cb.h + gap;
            } else {
                cx += cb.w + gap;
            }
        }
        let w = if iw > 0.0 {
            iw
        } else if vertical {
            max_w + 2.0 * pad
        } else {
            cx - x - gap + pad
        };
        let h = if ih > 0.0 {
            ih
        } else if vertical {
            cy - y - gap + pad
        } else {
            max_h + 2.0 * pad
        };
        Box2 {
            x,
            y,
            w: w.max(1.0),
            h: h.max(1.0),
        }
    } else {
        Box2 {
            x,
            y,
            w: iw.max(1.0),
            h: ih.max(1.0),
        }
    };
    boxes.insert(n.id, b);
    b
}

fn js_interp(s: &str, ir_path: &str) -> DResult<String> {
    let parts = interp_parts(s).map_err(|(off, msg)| interp_error(s, off, msg, ir_path))?;
    let mut exprs = Vec::new();
    for part in parts {
        match part {
            Part::Lit(t) => exprs.push(js_lit(&Json::String(t))),
            Part::Var(name, _) => exprs.push(format!("String(state.{name})")),
        }
    }
    Ok(if exprs.is_empty() {
        "\"\"".to_string()
    } else {
        exprs.join(" + ")
    })
}

fn draw_node(
    n: &ENode,
    boxes: &HashMap<u64, Box2>,
    out: &mut String,
    ir_path: &str,
) -> DResult<()> {
    if let Some(cond) = &n.when {
        out.push_str(&format!("  if (state.{cond}) {{\n"));
    }
    let zero = Box2 {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    let b = boxes.get(&n.id).copied().unwrap_or(zero);
    let x = fmt_num(b.x);
    let y = fmt_num(b.y);
    let w = fmt_num(b.w);
    let h = fmt_num(b.h);
    match n.kind.as_str() {
        "Text" => {
            let size = fmt_num(prop_num(&n.props, "size", 16.0));
            let color = js_lit(&Json::String(prop_str(&n.props, "color", "#111111")));
            let expr = js_interp(&prop_str(&n.props, "value", ""), ir_path)?;
            out.push_str(&format!(
                "  ctx.font = \"{size}px sans-serif\";\n  ctx.fillStyle = {color};\n  ctx.fillText({expr}, {x}, {y});\n"
            ));
        }
        "Button" => {
            let label = js_lit(&Json::String(prop_str(&n.props, "label", "")));
            out.push_str(&format!(
                "  ctx.fillStyle = \"#4F46E5\";\n  ctx.fillRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#ffffff\";\n  ctx.font = \"16px sans-serif\";\n  ctx.fillText({label}, {x} + 10, {y} + 8);\n"
            ));
        }
        "Input" => {
            let placeholder = js_lit(&Json::String(prop_str(&n.props, "placeholder", "")));
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#cbd5e1\";\n  ctx.strokeRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#64748b\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({placeholder}, {x} + 6, {y} + 7);\n"
            ));
        }
        "Image" => {
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#94a3b8\";\n  ctx.strokeRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#64748b\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText(\"[image]\", {x} + 6, {y} + 6);\n"
            ));
        }
        "Container" | "Card" | "Dialog" | "List" | "Grid" | "Scroll" => {
            let color = js_lit(&Json::String(prop_str(&n.props, "color", "#f1f5f9")));
            out.push_str(&format!(
                "  ctx.fillStyle = {color};\n  ctx.fillRect({x}, {y}, {w}, {h});\n"
            ));
        }
        "Badge" | "Avatar" | "Icon" => {
            let label = fallback_label(n);
            out.push_str(&format!(
                "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n"
            ));
        }
        "Divider" | "Spinner" => {
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#cbd5e1\";\n  ctx.beginPath();\n  ctx.moveTo({x}, {y} + {h} / 2);\n  ctx.lineTo({x} + {w}, {y} + {h} / 2);\n  ctx.stroke();\n"
            ));
        }
        "Progress" => {
            let value = fmt_num(prop_num(&n.props, "value", 0.0).clamp(0.0, 100.0));
            out.push_str(&format!(
                "  ctx.fillStyle = \"#e2e8f0\";\n  ctx.fillRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#4F46E5\";\n  ctx.fillRect({x}, {y}, {w} * {value} / 100, {h});\n"
            ));
        }
        "Switch" | "Checkbox" => {
            let bind = prop_str(&n.props, "bind", "");
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#94a3b8\";\n  ctx.strokeRect({x}, {y}, {h}, {h});\n  if (state.{bind}) {{ ctx.fillStyle = \"#4F46E5\"; ctx.fillRect({x} + 3, {y} + 3, {h} - 6, {h} - 6); }}\n"
            ));
        }
        "Slider" => {
            let bind = prop_str(&n.props, "bind", "");
            let min = fmt_num(prop_num(&n.props, "min", 0.0));
            let max = fmt_num(prop_num(&n.props, "max", 100.0));
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#cbd5e1\";\n  ctx.beginPath();\n  ctx.moveTo({x}, {y} + {h} / 2);\n  ctx.lineTo({x} + {w}, {y} + {h} / 2);\n  ctx.stroke();\n  ctx.fillStyle = \"#4F46E5\";\n  ctx.beginPath();\n  ctx.arc({x} + {w} * (state.{bind} - ({min})) / (({max}) - ({min})), {y} + {h} / 2, 6, 0, 6.283185307179586);\n  ctx.fill();\n"
            ));
        }
        "Radio" => {
            let bind = prop_str(&n.props, "bind", "");
            let label = js_lit(
                &n.props
                    .get("value")
                    .cloned()
                    .unwrap_or(Json::String(String::new())),
            );
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#94a3b8\";\n  ctx.beginPath();\n  ctx.arc({x} + {h} / 2, {y} + {h} / 2, {h} / 2 - 2, 0, 6.283185307179586);\n  ctx.stroke();\n  if (state.{bind} === {label}) {{ ctx.fillStyle = \"#4F46E5\"; ctx.fill(); }}\n"
            ));
        }
        "Tabs" => {
            let labels = prop_str(&n.props, "labels", "");
            let bind = prop_str(&n.props, "bind", "");
            let label = js_lit(&Json::String(labels));
            out.push_str(&format!(
                "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n  if (state.{bind}) {{ ctx.fillStyle = \"#4F46E5\"; ctx.fillRect({x}, {y} + {h} - 3, {w}, 3); }}\n"
            ));
        }
        // Documented fallback: an unknown kind renders as a Container or Text.
        _ => {
            let label = fallback_label(n);
            if !label.is_empty() {
                out.push_str(&format!(
                    "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n"
                ));
            }
        }
    }
    for child in &n.children {
        draw_node(child, boxes, out, ir_path)?;
    }
    if n.when.is_some() {
        out.push_str("  }\n");
    }
    Ok(())
}

/// A node kind a consumer does not know is rendered as its label/value/title/name.
fn fallback_label(n: &ENode) -> String {
    for key in ["label", "value", "title", "name"] {
        if let Some(Json::String(s)) = n.props.get(key) {
            return js_lit(&Json::String(s.clone()));
        }
    }
    "\"\"".to_string()
}

/// The text a fallback rendering shows for a node kind.
fn fallback_text(n: &ENode) -> String {
    for key in ["label", "value", "title", "name"] {
        if let Some(Json::String(s)) = n.props.get(key) {
            return s.clone();
        }
    }
    String::new()
}

fn js_action(action: &Json, ir_path: &str) -> DResult<String> {
    let op = action_str(action, "op", ir_path)?;
    let target = action_str(action, "target", ir_path)?;
    Ok(match op {
        "add" => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("state.{from}"),
                None => {
                    let n = action
                        .get("value")
                        .and_then(Json::as_i64)
                        .ok_or_else(|| bad_ir(ir_path, "'add' action needs an integer 'value'"))?;
                    n.to_string()
                }
            };
            format!("state.{target} += {rhs};")
        }
        "toggle" => format!("state.{target} = !state.{target};"),
        "set" => match action.get("valueFrom").and_then(Json::as_str) {
            Some(from) => format!("state.{target} = state.{from};"),
            None => {
                let v = action
                    .get("value")
                    .cloned()
                    .unwrap_or(Json::String(String::new()));
                format!("state.{target} = {};", js_lit(&v))
            }
        },
        _ => return Err(bad_ir(ir_path, format!("unknown event op '{op}'"))),
    })
}

/// One statement per action; a chain is a sequence of statements.
fn js_action_chain(event: &Json, ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for action in event_actions(event) {
        code.push_str(&js_action(action, ir_path)?);
        code.push('\n');
    }
    Ok(code)
}

fn emit_web(
    ir: &Json,
    app: &str,
    states: &[EState],
    tree: &ENode,
    out: &Path,
    ir_path: &str,
) -> DResult<()> {
    let pretty = serde_json::to_string_pretty(ir)
        .map_err(|e| bad_ir(ir_path, format!("cannot serialize IR: {e}")))?;
    write(out, "ir.json", &(pretty + "\n"), ir_path)?;

    let mut state_map = JsonMap::new();
    for st in states {
        state_map.insert(st.name.clone(), st.init.clone());
    }
    let state_js = js_lit(&Json::Object(state_map));
    let mut boxes = HashMap::new();
    layout(tree, 16.0, 16.0, &mut boxes);
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);

    let mut draw = String::new();
    draw_node(tree, &boxes, &mut draw, ir_path)?;

    let mut tap_arms = String::new();
    let mut input_arms = String::new();
    let mut clicks = String::new();
    let mut input_html = String::new();
    let mut input_listeners = String::new();
    for n in &nodes {
        if let Some(action) = n.events.get("tap") {
            let code = js_action_chain(action, ir_path)?;
            tap_arms.push_str(&format!(
                "  if (id === {}) {{ {}  render(); return; }}\n",
                n.id, code
            ));
            if n.kind == "Button" {
                let b = boxes.get(&n.id).copied().unwrap_or(Box2 {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                });
                clicks.push_str(&format!(
                    "  if (x >= {} && x <= {} && y >= {} && y <= {}) {{ applyTap({}); }}\n",
                    fmt_num(b.x),
                    fmt_num(b.x + b.w),
                    fmt_num(b.y),
                    fmt_num(b.y + b.h),
                    n.id
                ));
            }
        }
        if let Some(action) = n.events.get("input") {
            let target = action_str(action, "target", ir_path)?;
            input_arms.push_str(&format!(
                "  if (id === {}) {{ state.{} = String(text); render(); return; }}\n",
                n.id, target
            ));
        }
        if n.kind == "Input" {
            let placeholder = html_attr(&prop_str(&n.props, "placeholder", ""));
            input_html.push_str(&format!(
                "    <input id=\"n{}\" placeholder=\"{}\">\n",
                n.id, placeholder
            ));
            input_listeners.push_str(&format!(
                "const el{} = document.getElementById(\"n{}\");\nif (el{}) {{ el{}.addEventListener(\"input\", function (e) {{ applyInput({}, e.target.value); }}); }}\n",
                n.id, n.id, n.id, n.id, n.id
            ));
        }
    }

    let mut app_js = String::new();
    app_js.push_str(&format!(
        "// Generated by opuromc for app {app}; straight-line code, no IR interpreter.\n"
    ));
    app_js.push_str("\"use strict\";\n");
    app_js.push_str("const canvas = document.getElementById(\"app\");\n");
    app_js.push_str("const ctx = canvas.getContext(\"2d\");\n");
    app_js.push_str(&format!("const state = {state_js};\n\n"));
    app_js.push_str("function render() {\n");
    app_js.push_str("  ctx.clearRect(0, 0, canvas.width, canvas.height);\n");
    app_js.push_str("  ctx.textBaseline = \"top\";\n");
    app_js.push_str(&draw);
    app_js.push_str("}\n\n");
    app_js.push_str("function applyTap(id) {\n");
    app_js.push_str(&tap_arms);
    app_js.push_str("}\n\n");
    app_js.push_str("function applyInput(id, text) {\n");
    app_js.push_str(&input_arms);
    app_js.push_str("}\n\n");
    if !clicks.is_empty() {
        app_js.push_str("canvas.addEventListener(\"click\", function (e) {\n");
        app_js.push_str("  const rect = canvas.getBoundingClientRect();\n");
        app_js.push_str("  const x = e.clientX - rect.left;\n");
        app_js.push_str("  const y = e.clientY - rect.top;\n");
        app_js.push_str(&clicks);
        app_js.push_str("});\n\n");
    }
    app_js.push_str(&input_listeners);
    app_js.push_str("render();\n");
    write(out, "app.js", &app_js, ir_path)?;

    let title = html_attr(app);
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>body {{ font-family: sans-serif; margin: 0; padding: 16px; }} canvas {{ background: #fff; border: 1px solid #ddd; }} #inputs {{ margin-top: 12px; }}</style>\n</head>\n<body>\n<canvas id=\"app\" width=\"800\" height=\"600\"></canvas>\n<div id=\"inputs\">\n{input_html}</div>\n<script src=\"app.js\"></script>\n</body>\n</html>\n"
    );
    write(out, "index.html", &html, ir_path)
}

// ----------------------------------------------------------- mini programs ----

fn style_of(props: &JsonMap<String, Json>, background: bool) -> String {
    let mut parts = Vec::new();
    for k in ["gap", "padding", "width", "height", "radius", "size"] {
        if let Some(v) = props.get(k) {
            let s = match v {
                Json::String(s) => s.clone(),
                _ => v.to_string(),
            };
            let css = match k {
                "radius" => format!("border-radius:{s}px"),
                "size" => format!("font-size:{s}px"),
                _ => format!("{k}:{s}px"),
            };
            parts.push(css);
        }
    }
    if let Some(Json::String(c)) = props.get("color") {
        let key = if background {
            "background-color"
        } else {
            "color"
        };
        parts.push(format!("{key}:{c}"));
    }
    parts.join(";")
}

fn markup_interp(s: &str, p: &Platform, ir_path: &str) -> DResult<String> {
    let parts = interp_parts(s).map_err(|(off, msg)| interp_error(s, off, msg, ir_path))?;
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Lit(t) => out.push_str(&markup_text(&t)),
            Part::Var(name, _) => {
                out.push_str(p.open);
                out.push_str(&name);
                out.push_str(p.close);
            }
        }
    }
    Ok(out)
}

fn bind_expr(p: &Platform, name: &str) -> String {
    format!("{}{name}{}", p.open, p.close)
}

fn markup_node(
    n: &ENode,
    p: &Platform,
    depth: usize,
    out: &mut String,
    ir_path: &str,
) -> DResult<()> {
    let pad = "  ".repeat(depth);
    let id = n.id;
    let style = style_of(&n.props, n.kind == "Container");
    let style_attr = if style.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", html_attr(&style))
    };
    let cond_attr = match &n.when {
        Some(name) => format!(" {}=\"{{{{{name}}}}}\"", p.cond_attr),
        None => String::new(),
    };
    match n.kind.as_str() {
        "Text" => {
            let value = markup_interp(&prop_str(&n.props, "value", ""), p, ir_path)?;
            out.push_str(&format!(
                "{pad}<text class=\"n{id}\"{style_attr}{cond_attr}>{value}</text>\n"
            ));
        }
        "Button" => {
            let label = markup_text(&prop_str(&n.props, "label", ""));
            let tap = if n.events.contains_key("tap") {
                format!(" {}=\"onTap{id}\"", p.tap_attr)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{pad}<button class=\"n{id}\"{style_attr}{cond_attr}{tap}>{label}</button>\n"
            ));
        }
        "Input" => {
            let placeholder = html_attr(&prop_str(&n.props, "placeholder", ""));
            let bind = n.props.get("bind").and_then(Json::as_str);
            let value = match bind {
                Some(name) => format!(" value=\"{}\"", bind_expr(p, name)),
                None => String::new(),
            };
            let input = if n.events.contains_key("input") {
                format!(" {}=\"onInput{id}\"", p.input_attr)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{pad}<input class=\"n{id}\"{style_attr}{cond_attr} placeholder=\"{placeholder}\"{value}{input} />\n"
            ));
        }
        "Image" => {
            let src = html_attr(&prop_str(&n.props, "src", ""));
            out.push_str(&format!(
                "{pad}<image class=\"n{id}\"{style_attr}{cond_attr} src=\"{src}\" />\n"
            ));
        }
        "Spacer" => {
            out.push_str(&format!(
                "{pad}<view class=\"spacer n{id}\"{style_attr}{cond_attr}></view>\n"
            ));
        }
        "Badge" | "Avatar" | "Icon" | "Progress" | "Switch" | "Slider" | "Checkbox" | "Radio"
        | "Tabs" => {
            let label = markup_text(&fallback_text(n));
            let tap = if n.events.contains_key("tap") {
                format!(" {}=\"onTap{id}\"", p.tap_attr)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{pad}<view class=\"n{id}\"{style_attr}{cond_attr}{tap}><text>{label}</text></view>\n"
            ));
        }
        "Divider" | "Spinner" => {
            out.push_str(&format!(
                "{pad}<view class=\"n{id}\"{style_attr}{cond_attr}></view>\n"
            ));
        }
        kind => {
            // Documented fallback: an unknown kind renders as a Container.
            let cls = if kind == "Row" { "row" } else { "col" };
            out.push_str(&format!(
                "{pad}<view class=\"{cls} n{id}\"{style_attr}{cond_attr}>\n"
            ));
            for child in &n.children {
                markup_node(child, p, depth + 1, out, ir_path)?;
            }
            out.push_str(&format!("{pad}</view>\n"));
        }
    }
    Ok(())
}

fn mini_action(action: &Json, p: &Platform, ir_path: &str) -> DResult<String> {
    let op = action_str(action, "op", ir_path)?;
    let target = action_str(action, "target", ir_path)?;
    let set = &p.state_set;
    Ok(match op {
        "add" => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("this.data.{from}"),
                None => {
                    let n = action
                        .get("value")
                        .and_then(Json::as_i64)
                        .ok_or_else(|| bad_ir(ir_path, "'add' action needs an integer 'value'"))?;
                    n.to_string()
                }
            };
            format!("this.{set}({{\"{target}\": this.data.{target} + ({rhs})}});")
        }
        "toggle" => format!("this.{set}({{\"{target}\": !this.data.{target}}});"),
        "set" => match action.get("valueFrom").and_then(Json::as_str) {
            Some(from) => format!("this.{set}({{\"{target}\": this.data.{from}}});"),
            None => {
                let v = action
                    .get("value")
                    .cloned()
                    .unwrap_or(Json::String(String::new()));
                format!("this.{set}({{\"{target}\": {}}});", js_lit(&v))
            }
        },
        _ => return Err(bad_ir(ir_path, format!("unknown event op '{op}'"))),
    })
}

/// A chain is a sequence of statements, executed in order.
fn mini_action_chain(event: &Json, p: &Platform, ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for action in event_actions(event) {
        code.push_str(&mini_action(action, p, ir_path)?);
        code.push(' ');
    }
    Ok(code)
}

fn emit_mini(
    target: &str,
    app: &str,
    states: &[EState],
    tree: &ENode,
    out: &Path,
    ir_path: &str,
) -> DResult<()> {
    let p = PLATFORMS
        .iter()
        .find(|p| p.key == target)
        .ok_or_else(|| bad_ir(ir_path, format!("no platform table entry for '{target}'")))?;

    let mut window = JsonMap::new();
    window.insert(p.title_key.to_string(), json!(app));
    let mut app_obj = JsonMap::new();
    app_obj.insert("pages".to_string(), json!(["pages/index/index"]));
    app_obj.insert("window".to_string(), Json::Object(window));
    let app_json = serde_json::to_string_pretty(&Json::Object(app_obj))
        .map_err(|e| bad_ir(ir_path, format!("cannot serialize app.json: {e}")))?;
    write(out, "app.json", &(app_json + "\n"), ir_path)?;

    let mut markup = String::new();
    markup.push_str(&format!(
        "<!-- Generated by opuromc for {}; no IR interpreter. -->\n",
        p.key
    ));
    markup_node(tree, p, 0, &mut markup, ir_path)?;
    let keys = bind_expr(p, "__stateKeys");
    let item = bind_expr(p, "item");
    markup.push_str(&format!(
        "<view class=\"state\" {}=\"{}\" {}=\"*this\"><text>{}</text></view>\n",
        p.for_attr, keys, p.key_attr, item
    ));
    write(out, &format!("index.{}", p.markup_ext), &markup, ir_path)?;

    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut style = String::from(
        ".col { display: flex; flex-direction: column; }\n.row { display: flex; flex-direction: row; }\n.state { margin-top: 12px; }\n",
    );
    for n in &nodes {
        let s = style_of(&n.props, n.kind == "Container");
        if !s.is_empty() {
            style.push_str(&format!(".n{} {{ {}; }}\n", n.id, s));
        }
    }
    write(out, &format!("index.{}", p.style_ext), &style, ir_path)?;

    let mut data = JsonMap::new();
    for st in states {
        data.insert(st.name.clone(), st.init.clone());
    }
    let keys_arr: Vec<Json> = states.iter().map(|s| json!(s.name)).collect();
    data.insert("__stateKeys".to_string(), Json::Array(keys_arr));
    let data_js = js_lit(&Json::Object(data));
    let mut methods = String::new();
    for n in &nodes {
        if let Some(action) = n.events.get("tap") {
            let code = mini_action_chain(action, p, ir_path)?;
            methods.push_str(&format!("  onTap{}: function () {{ {} }},\n", n.id, code));
        }
        if let Some(action) = n.events.get("input") {
            let target_name = action_str(action, "target", ir_path)?;
            methods.push_str(&format!(
                "  onInput{}: function (e) {{ this.{}({{\"{}\": e.detail.value}}); }},\n",
                n.id, p.state_set, target_name
            ));
        }
    }
    let index_js = format!(
        "// Generated by opuromc for {}; straight-line code, no IR interpreter.\nPage({{\n  data: {},\n{}}});\n",
        p.key, data_js, methods
    );
    write(out, "index.js", &index_js, ir_path)
}
