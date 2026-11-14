use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};
use crate::icons;
use crate::lower::{interp_parts, offset_pos, Part};
use crate::parser::Pos;

pub const TARGETS: &[(&str, &str)] = &[
    ("native", "Cargo.toml, src/main.rs"),
    ("web", "index.html, app.js, ir.json"),
    (
        "wechat",
        "app.json, app.js, pages/index/index.wxml, pages/index/index.wxss, pages/index/index.js",
    ),
    (
        "alipay",
        "app.json, app.js, pages/index/index.axml, pages/index/index.acss, pages/index/index.js",
    ),
    (
        "qq",
        "app.json, app.js, pages/index/index.qml, pages/index/index.qss, pages/index/index.js",
    ),
    (
        "douyin",
        "app.json, app.js, pages/index/index.ttml, pages/index/index.ttss, pages/index/index.js",
    ),
    (
        "baidu",
        "app.json, app.js, pages/index/index.swan, pages/index/index.css, pages/index/index.js",
    ),
    (
        "kuaishou",
        "app.json, app.js, pages/index/index.ksml, pages/index/index.css, pages/index/index.js",
    ),
    (
        "wasm",
        "Cargo.toml, src/lib.rs, index.html, host.js, README.md",
    ),
];

/// Responsive DOM build target. It is deliberately kept out of [`TARGETS`] so
/// `opuromc targets` still reports exactly the nine v0.1 IR emitters; it is
/// dispatched explicitly in [`emit`].
pub const WEB_DOM_TARGET: &str = "web-dom";

/// Path of the sibling responsive DOM emitter, resolved next to this crate.
fn web_dom_emitter_path() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("opurom-lang manifest dir has a parent workspace");
    root.join("opurom-web").join("bin").join("opurom-web.mjs")
}

#[derive(Clone)]
pub struct EState {
    pub name: String,
    pub ty: String,
    pub init: Json,
}

#[derive(Clone)]
pub struct Computed {
    pub name: String,
    pub ty: String,
    pub expr: String,
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
    /// Optional bool state whose false value shows this node and its subtree.
    pub unless: Option<String>,
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
    cond_attr: &'static str,
    tap_attr: &'static str,
    input_attr: &'static str,
    state_set: &'static str,
    request_api: &'static str,
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
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        request_api: "wx.request",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "alipay",
        cond_attr: "a:if",
        markup_ext: "axml",
        style_ext: "acss",
        tap_attr: "onTap",
        input_attr: "onInput",
        state_set: "setData",
        request_api: "my.request",
        title_key: "defaultTitle",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "qq",
        cond_attr: "qq:if",
        markup_ext: "qml",
        style_ext: "qss",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        request_api: "qq.request",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "douyin",
        cond_attr: "tt:if",
        markup_ext: "ttml",
        style_ext: "ttss",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        request_api: "tt.request",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "baidu",
        cond_attr: "s-if",
        markup_ext: "swan",
        style_ext: "css",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        request_api: "swan.request",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
    Platform {
        key: "kuaishou",
        cond_attr: "ks:if",
        markup_ext: "ksml",
        style_ext: "css",
        tap_attr: "bindtap",
        input_attr: "bindinput",
        state_set: "setData",
        request_api: "ks.request",
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
    let unless = o.get("unless").and_then(Json::as_str).map(str::to_string);
    Ok(ENode {
        id,
        kind,
        props,
        events,
        children,
        when,
        unless,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprTy {
    Int,
    Bool,
    Str,
}

impl ExprTy {
    fn name(self) -> &'static str {
        match self {
            ExprTy::Int => "int",
            ExprTy::Bool => "bool",
            ExprTy::Str => "string",
        }
    }
}

struct ExprParser<'a, F> {
    toks: Vec<&'a str>,
    i: usize,
    states: &'a [EState],
    computed: &'a [Computed],
    ir_path: &'a str,
    rust: bool,
    var: &'a mut F,
}

impl<'a, F> ExprParser<'a, F>
where
    F: FnMut(&EState) -> DResult<String>,
{
    fn peek(&self) -> Option<&'a str> {
        self.toks.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.i += 1;
        Some(token)
    }

    fn invalid(&self, found: &str) -> Diagnostic {
        bad_ir(
            self.ir_path,
            format!(
                "computed expression supports only +, -, *, integer literals and declared state names, found '{found}'"
            ),
        )
    }

    fn state(&mut self, name: &str) -> DResult<(ExprTy, String)> {
        if self.computed.iter().any(|c| c.name == name) {
            return Err(bad_ir(
                self.ir_path,
                format!("computed expression cannot read computed value '{name}'"),
            ));
        }
        let st = self.states.iter().find(|s| s.name == name).ok_or_else(|| {
            bad_ir(
                self.ir_path,
                format!("unknown state '{name}' in computed expression"),
            )
        })?;
        let ty = match st.ty.as_str() {
            "int" => ExprTy::Int,
            "bool" => ExprTy::Bool,
            "string" => ExprTy::Str,
            "list" => {
                return Err(bad_ir(
                    self.ir_path,
                    format!("list state '{name}' is compile-time only"),
                ));
            }
            other => {
                return Err(bad_ir(
                    self.ir_path,
                    format!("IR state '{name}' has unsupported type '{other}'"),
                ));
            }
        };
        let code = (self.var)(st)?;
        Ok((ty, code))
    }

    fn factor(&mut self) -> DResult<(ExprTy, String)> {
        let token = self
            .bump()
            .ok_or_else(|| self.invalid("end of expression"))?;
        if let Ok(n) = token.parse::<i64>() {
            let code = if self.rust {
                format!("{n}i64")
            } else {
                n.to_string()
            };
            return Ok((ExprTy::Int, code));
        }
        if !simple_ident(token) {
            return Err(self.invalid(token));
        }
        self.state(token)
    }

    fn combine(
        &self,
        op: &str,
        lhs: (ExprTy, String),
        rhs: (ExprTy, String),
    ) -> DResult<(ExprTy, String)> {
        if lhs.0 != ExprTy::Int || rhs.0 != ExprTy::Int {
            return Err(bad_ir(
                self.ir_path,
                format!("computed expression '{op}' requires int operands"),
            ));
        }
        let code = if self.rust {
            let method = match op {
                "+" => "saturating_add",
                "-" => "saturating_sub",
                _ => "saturating_mul",
            };
            format!("({}).{}({})", lhs.1, method, rhs.1)
        } else {
            format!("({} {} {})", lhs.1, op, rhs.1)
        };
        Ok((ExprTy::Int, code))
    }

    fn term(&mut self) -> DResult<(ExprTy, String)> {
        let mut lhs = self.factor()?;
        while self.peek() == Some("*") {
            self.i += 1;
            let rhs = self.factor()?;
            lhs = self.combine("*", lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn sum(&mut self) -> DResult<(ExprTy, String)> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some("+") => "+",
                Some("-") => "-",
                _ => break,
            };
            self.i += 1;
            let rhs = self.term()?;
            lhs = self.combine(op, lhs, rhs)?;
        }
        if self.i != self.toks.len() {
            return Err(self.invalid(self.peek().unwrap_or("unexpected token")));
        }
        Ok(lhs)
    }
}

pub fn computed_expr<F>(
    expr: &str,
    states: &[EState],
    computed: &[Computed],
    ir_path: &str,
    rust: bool,
    mut var: F,
) -> DResult<(&'static str, String)>
where
    F: FnMut(&EState) -> DResult<String>,
{
    let toks: Vec<&str> = expr.split_whitespace().collect();
    if toks.is_empty() {
        return Err(bad_ir(ir_path, "IR computed expression is empty"));
    }
    let mut parser = ExprParser {
        toks,
        i: 0,
        states,
        computed,
        ir_path,
        rust,
        var: &mut var,
    };
    let (ty, code) = parser.sum()?;
    Ok((ty.name(), code))
}

fn read_computed(ir: &Json, states: &[EState], ir_path: &str) -> DResult<Vec<Computed>> {
    let Some(raw) = ir.get("computed") else {
        return Ok(Vec::new());
    };
    let arr = raw
        .as_array()
        .ok_or_else(|| bad_ir(ir_path, "IR 'computed' must be an array"))?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in arr {
        let o = entry
            .as_object()
            .ok_or_else(|| bad_ir(ir_path, "IR computed entry must be an object"))?;
        for key in o.keys() {
            if !matches!(key.as_str(), "name" | "type" | "expr") {
                return Err(bad_ir(
                    ir_path,
                    format!("unknown IR computed field '{key}'"),
                ));
            }
        }
        let name = o
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "IR computed entry is missing a string 'name'"))?
            .to_string();
        if !simple_ident(&name) {
            return Err(bad_ir(
                ir_path,
                format!("IR computed name '{name}' is not a valid state field"),
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(bad_ir(
                ir_path,
                format!("duplicate IR computed value '{name}'"),
            ));
        }
        let ty = o
            .get("type")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "IR computed entry is missing a string 'type'"))?;
        if !matches!(ty, "int" | "bool" | "string") {
            return Err(bad_ir(
                ir_path,
                format!("unsupported IR computed type '{ty}'"),
            ));
        }
        let expr = o
            .get("expr")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "IR computed entry is missing a string 'expr'"))?;
        let st = states.iter().find(|s| s.name == name).ok_or_else(|| {
            bad_ir(
                ir_path,
                format!("computed value '{name}' is not declared in state"),
            )
        })?;
        if st.ty.as_str() != ty {
            return Err(bad_ir(
                ir_path,
                format!(
                    "computed value '{name}' has type '{ty}' but its state has type '{}'",
                    st.ty
                ),
            ));
        }
        out.push(Computed {
            name,
            ty: ty.to_string(),
            expr: expr.to_string(),
        });
    }
    for c in &out {
        let (inferred, _) = computed_expr(&c.expr, states, &out, ir_path, false, |st| {
            Ok(st.name.clone())
        })?;
        if inferred != c.ty.as_str() {
            return Err(bad_ir(
                ir_path,
                format!(
                    "IR computed value '{}' has type '{}' but its expression is {inferred}",
                    c.name, c.ty
                ),
            ));
        }
    }
    Ok(out)
}

fn ensure_mutable(
    name: &str,
    states: &[EState],
    computed: &[Computed],
    ir_path: &str,
) -> DResult<()> {
    if computed.iter().any(|c| c.name == name) {
        return Err(bad_ir(
            ir_path,
            format!("computed value '{name}' is read-only"),
        ));
    }
    let st = states
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| bad_ir(ir_path, format!("state '{name}' is not declared")))?;
    if st.ty == "list" {
        return Err(bad_ir(
            ir_path,
            format!("list state '{name}' is compile-time only"),
        ));
    }
    Ok(())
}

fn ensure_condition(name: &str, states: &[EState], ir_path: &str) -> DResult<()> {
    let st = states.iter().find(|s| s.name == name).ok_or_else(|| {
        bad_ir(
            ir_path,
            format!("conditional state '{name}' is not declared"),
        )
    })?;
    if st.ty != "bool" {
        return Err(bad_ir(
            ir_path,
            format!(
                "IR conditional requires a bool state, found '{}' state '{name}'",
                st.ty
            ),
        ));
    }
    Ok(())
}

fn validate_actions(
    states: &[EState],
    computed: &[Computed],
    tree: &ENode,
    ir_path: &str,
) -> DResult<()> {
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    for node in nodes {
        if let Some(Json::String(name)) = node.props.get("bind") {
            ensure_mutable(name, states, computed, ir_path)?;
        }
        if let Some(name) = &node.when {
            ensure_condition(name, states, ir_path)?;
        }
        if let Some(name) = &node.unless {
            ensure_condition(name, states, ir_path)?;
        }
        for event in node.events.values() {
            for effect in event_effects(event, ir_path)? {
                if let Effect::Fetch { into, .. } = effect {
                    ensure_mutable(&into, states, computed, ir_path)?;
                }
            }
            for action in event_actions(event) {
                if let Some(target) = action.get("target").and_then(Json::as_str) {
                    ensure_mutable(target, states, computed, ir_path)?;
                }
            }
        }
    }
    Ok(())
}

fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

fn simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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

/// Encode a string as standard base64 (RFC 4648, `+`/`/` and `=` padding), so a
/// generated data URI needs no runtime and no extra crate.
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((n >> 18) & 63) as usize] as char);
        out.push(CHARS[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            CHARS[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
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

/// A compile-time-lowered effect that emitters turn into target-native async code.
#[derive(Clone, Debug)]
pub enum Effect {
    Delay { ms: u64 },
    Fetch { path: String, into: String },
}

pub fn event_effects(event: &Json, ir_path: &str) -> DResult<Vec<Effect>> {
    if let Some(value) = event.get("async") {
        if !value.is_boolean() {
            return Err(bad_ir(ir_path, "IR event 'async' must be a boolean"));
        }
    }
    let Some(raw) = event.get("effects") else {
        return Ok(Vec::new());
    };
    if event.get("async").and_then(Json::as_bool) != Some(true) {
        return Err(bad_ir(
            ir_path,
            "IR event 'effects' requires \"async\": true",
        ));
    }
    let list = raw
        .as_array()
        .ok_or_else(|| bad_ir(ir_path, "IR event 'effects' must be an array"))?;
    let mut out = Vec::with_capacity(list.len());
    for one in list {
        let kind = one
            .get("kind")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "IR effect is missing string 'kind'"))?;
        match kind {
            "delay" => {
                let ms = one
                    .get("ms")
                    .and_then(Json::as_u64)
                    .ok_or_else(|| bad_ir(ir_path, "IR delay effect needs integer 'ms'"))?;
                if ms > 60000 {
                    return Err(bad_ir(
                        ir_path,
                        format!("IR delay effect 'ms' must be 0..60000, found {ms}"),
                    ));
                }
                out.push(Effect::Delay { ms });
            }
            "fetch" => {
                let path = one
                    .get("path")
                    .and_then(Json::as_str)
                    .ok_or_else(|| bad_ir(ir_path, "IR fetch effect needs string 'path'"))?
                    .to_string();
                let into = one
                    .get("into")
                    .and_then(Json::as_str)
                    .ok_or_else(|| bad_ir(ir_path, "IR fetch effect needs string 'into'"))?
                    .to_string();
                out.push(Effect::Fetch { path, into });
            }
            other => {
                return Err(bad_ir(ir_path, format!("unknown IR effect kind '{other}'")));
            }
        }
    }
    Ok(out)
}

fn tree_has_effects(states: &[EState], tree: &ENode, ir_path: &str) -> DResult<bool> {
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut found = false;
    for node in nodes {
        for (name, event) in &node.events {
            let effects = event_effects(event, ir_path)?;
            let marked = event.get("async").is_some() || event.get("effects").is_some();
            if marked && name.as_str() != "tap" {
                return Err(bad_ir(
                    ir_path,
                    format!("IR effects are only supported on 'tap' events, found '{name}'"),
                ));
            }
            for effect in &effects {
                found = true;
                if let Effect::Fetch { into, .. } = effect {
                    let valid = simple_ident(into)
                        && states
                            .iter()
                            .any(|state| state.name == *into && state.ty == "string");
                    if !valid {
                        return Err(bad_ir(
                            ir_path,
                            format!("IR fetch target '{into}' is not a declared string state"),
                        ));
                    }
                }
            }
        }
    }
    Ok(found)
}

/// An event is either one action object (v0.1), `{"actions":[...]}` (interactive UI), or
/// effects only (async language layer).
pub fn event_actions(event: &Json) -> Vec<&Json> {
    match event.get("actions").and_then(Json::as_array) {
        Some(list) => list.iter().collect(),
        None if event.get("op").is_some() => vec![event],
        None => Vec::new(),
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
    let known = TARGETS.iter().any(|(n, _)| *n == target) || target == WEB_DOM_TARGET;
    if !known {
        let names: Vec<&str> = TARGETS
            .iter()
            .map(|(n, _)| *n)
            .chain(std::iter::once(WEB_DOM_TARGET))
            .collect();
        return Err(Diagnostic::file(
            ir_path,
            format!("unknown target '{target}'"),
            format!("choose one of: {}", names.join(", ")),
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
    let computed = read_computed(&ir, &states, ir_path)?;
    validate_actions(&states, &computed, &tree, ir_path)?;
    let has_effects = tree_has_effects(&states, &tree, ir_path)?;
    fs::create_dir_all(out).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot create output directory {}: {e}", out.display()),
            "check the -o path and permissions",
        )
    })?;
    match target {
        "native" => emit_native(
            &app,
            &states,
            &computed,
            &tree,
            &ir,
            out,
            ir_path,
            has_effects,
        ),
        "web" => emit_web(&ir, &app, &states, &computed, &tree, out, ir_path),
        "web-dom" => emit_web_dom(ir_path, out),
        "wasm" => {
            crate::wasm::emit_wasm(&app, &states, &computed, &tree, out, ir_path, has_effects)
        }
        _ => emit_mini(target, &app, &states, &computed, &tree, out, ir_path),
    }
}

// ---------------------------------------------------------------- native ----

/// The workspace that holds the sibling render crate the generated native app
/// renders through. The compiler's manifest directory is the `opurom-lang`
/// crate, so its parent is the workspace root. The generated app depends only
/// on opurom-render, which writes PPM with no external crates, so an offline
/// build resolves without a registry.
fn native_dep_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("opurom-lang manifest dir has a parent workspace")
        .to_path_buf()
}

fn native_manifest(dep_root: &Path) -> String {
    let render = dep_root.join("opurom-render");
    format!(
        "[package]\nname = \"opurom_native\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\nopurom-render = {{ path = \"{}\" }}\n",
        render.display()
    )
}

fn rust_ty(ty: &str) -> &'static str {
    match ty {
        "int" => "i64",
        "bool" => "bool",
        "list" => "Vec<String>",
        _ => "String",
    }
}

fn rust_init(st: &EState) -> String {
    match st.ty.as_str() {
        "int" => format!("{}i64", st.init.as_i64().unwrap_or(0)),
        "bool" => st.init.as_bool().unwrap_or(false).to_string(),
        "list" => {
            let values = st
                .init
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|v| format!("{}.to_string()", rust_str(v.as_str().unwrap_or(""))))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("vec![{values}]")
        }
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
    match state_ty(states, target) {
        "string" => format!("_state.{target}.clone()"),
        "list" => format!("format!(\"{{:?}}\", _state.{target})"),
        _ => format!("_state.{target}.to_string()"),
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
        op @ ("add" | "sub") => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("_state.{from}"),
                None => {
                    let n = action.get("value").and_then(Json::as_i64).ok_or_else(|| {
                        bad_ir(ir_path, format!("'{op}' action needs an integer 'value'"))
                    })?;
                    format!("{n}i64")
                }
            };
            let method = if op == "sub" {
                "saturating_sub"
            } else {
                "saturating_add"
            };
            format!("_state.{target} = _state.{target}.{method}({rhs});")
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

/// Generated std-only effect helpers. Each event calls the sequencer in source order.
fn native_effect_helpers() -> &'static str {
    r#"// Async effects: measured sleeps and plain HTTP GET; no third-party runtime.
#[allow(dead_code)]
enum Effect<'a> {
    Delay(u64),
    Fetch { path: &'static str, into: &'a mut String },
}

#[allow(dead_code)]
fn measured_delay(ms: u64) {
    let start = Instant::now();
    let target = Duration::from_millis(ms);
    loop {
        let elapsed = start.elapsed();
        if elapsed >= target { break; }
        std::thread::sleep(target - elapsed);
    }
}

#[allow(dead_code)]
fn decode_chunked(body: &str) -> Result<String, String> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        let end = bytes[i..]
            .windows(2)
            .position(|pair| pair == b"\r\n")
            .ok_or_else(|| "truncated chunked HTTP body".to_string())?;
        let size_line = std::str::from_utf8(&bytes[i..i + end])
            .map_err(|_| "invalid chunk size".to_string())?;
        let size_text = size_line.split(';').next().unwrap_or(size_line).trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| "invalid chunk size".to_string())?;
        i += end + 2;
        if size == 0 { break; }
        if i + size + 2 > bytes.len() {
            return Err("truncated chunked HTTP body".to_string());
        }
        out.extend_from_slice(&bytes[i..i + size]);
        i += size;
        if bytes.get(i..i + 2) != Some(b"\r\n".as_slice()) {
            return Err("invalid chunk terminator".to_string());
        }
        i += 2;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[allow(dead_code)]
fn http_get(base: &str, path: &str) -> Result<String, String> {
    let url = base.trim_end_matches('/');
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "OPR_BASE_URL must start with http://".to_string())?;
    let (authority, prefix) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err("OPR_BASE_URL is missing a host".to_string());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .map_err(|_| "invalid port in OPR_BASE_URL".to_string())?,
        ),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err("OPR_BASE_URL is missing a host".to_string());
    }
    let prefix = prefix.trim_end_matches('/');
    let request_path = if path.starts_with('/') {
        format!("{prefix}{path}")
    } else {
        format!("{prefix}/{path}")
    };
    let request_path = if request_path.is_empty() {
        "/".to_string()
    } else {
        request_path
    };
    let mut stream = TcpStream::connect((host, port)).map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        request_path, authority
    )
    .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response".to_string())?;
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        decode_chunked(body)
    } else {
        Ok(body.to_string())
    }
}

#[allow(dead_code)]
fn fetch_text(path: &str) -> String {
    // OPR_BASE_URL unset: use a deterministic local result instead of touching the network.
    match std::env::var("OPR_BASE_URL") {
        Ok(base) if !base.trim().is_empty() => http_get(base.trim(), path)
            .unwrap_or_else(|error| format!("error: {error}")),
        _ => format!("local: {path}"),
    }
}

#[allow(dead_code)]
fn run_effects<'a>(effects: impl IntoIterator<Item = Effect<'a>>) {
    for effect in effects {
        match effect {
            Effect::Delay(ms) => measured_delay(ms),
            Effect::Fetch { path, into } => *into = fetch_text(path),
        }
    }
}

"#
}

fn native_effects(effects: &[Effect], ir_path: &str) -> DResult<String> {
    let mut out = String::new();
    for effect in effects {
        match effect {
            Effect::Delay { ms } => {
                out.push_str(&format!(
                    "            run_effects([Effect::Delay({ms})]);\n"
                ));
            }
            Effect::Fetch { path, into } => {
                if !simple_ident(into) {
                    return Err(bad_ir(
                        ir_path,
                        format!("IR fetch target '{into}' is not a valid state field"),
                    ));
                }
                out.push_str(&format!(
                    "            run_effects([Effect::Fetch {{ path: {}, into: &mut _state.{into} }}]);\n",
                    rust_str(path)
                ));
            }
        }
    }
    Ok(out)
}

fn native_computed(states: &[EState], computed: &[Computed], ir_path: &str) -> DResult<String> {
    if computed.is_empty() {
        return Ok(String::new());
    }
    let mut s = String::from(
        "#[allow(dead_code)]\nfn apply_computed(_state: &mut State) -> Vec<Patch> {\n",
    );
    for c in computed {
        let (_, expr) = computed_expr(&c.expr, states, computed, ir_path, true, |st| {
            Ok(format!("_state.{}", st.name))
        })?;
        let expr = if c.ty == "string" {
            format!("{expr}.clone()")
        } else {
            expr
        };
        s.push_str(&format!("    _state.{} = {expr};\n", c.name));
    }
    s.push_str("    vec![\n");
    for c in computed {
        s.push_str(&format!(
            "        Patch {{ name: {}, value: {} }},\n",
            rust_str(&c.name),
            patch_value(states, &c.name)
        ));
    }
    s.push_str("    ]\n}\n\n");
    Ok(s)
}

fn native_source(
    app: &str,
    states: &[EState],
    computed: &[Computed],
    tree: &ENode,
    ir: &Json,
    ir_path: &str,
    has_effects: bool,
) -> DResult<String> {
    let ir_text = serde_json::to_string(ir)
        .map_err(|e| bad_ir(ir_path, format!("cannot serialize IR: {e}")))?;
    let ir_lit = format!("{ir_text:?}");
    let mut s = String::new();
    s.push_str(&format!(
        "// Generated by opuromc for app {app}; lowered from IR v0.1. Do not edit.\n"
    ));
    if has_effects {
        s.push_str("use std::io::{self, BufRead, Read, Write};\n");
        s.push_str("use std::net::TcpStream;\n");
        s.push_str("use std::time::{Duration, Instant};\n\n");
        s.push_str(native_effect_helpers());
    } else {
        s.push_str("use std::io::{self, BufRead, Write};\n\n");
    }
    s.push_str("struct State {\n");
    for st in states {
        s.push_str(&format!("    {}: {},\n", st.name, rust_ty(&st.ty)));
    }
    s.push_str("}\n\nstruct Patch {\n    name: &'static str,\n    value: String,\n}\n\n");
    s.push_str(&native_computed(states, computed, ir_path)?);
    s.push_str(
        "fn apply(_state: &mut State, event_node_id: u32, _input: Option<&str>) -> Vec<Patch> {\n",
    );
    s.push_str("    match (event_node_id, _input.is_some()) {\n");
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    for n in &nodes {
        for (event, is_input) in [("tap", false), ("input", true)] {
            if let Some(action) = n.events.get(event) {
                let effect_code = native_effects(&event_effects(action, ir_path)?, ir_path)?;
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
                if computed.is_empty() {
                    s.push_str(&format!(
                        "        ({}, {}) => {{\n{effect_code}{body}            {patch_expr}\n        }}\n",
                        n.id, is_input
                    ));
                } else {
                    s.push_str(&format!(
                        "        ({}, {}) => {{\n{effect_code}{body}            let mut patches = {patch_expr};\n            patches.extend(apply_computed(_state));\n            patches\n        }}\n",
                        n.id, is_input
                    ));
                }
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
            .map(|st| {
                if st.ty == "list" {
                    format!("format!(\"{{:?}}\", state.{})", st.name)
                } else {
                    format!("state.{}", st.name)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!(
            "fn print_state(state: &State) {{\n    println!(\"state {fmt}\", {args});\n}}\n\n"
        ));
    }
    // The rendered frame: re-renders the embedded IR with the live state and
    // writes the PPM frame. The viewport defaults to 800x600 and is overridable
    // through OPUROM_FRAME_WIDTH/HEIGHT; the frame path is the first argument,
    // defaulting to the system temp directory so running the state machine never
    // writes into the caller's directory. The renderer's StateView holds the
    // scalar state kinds only, so list state fields are left out of the frame.
    let mut state_sets = String::new();
    let mut renders_state = false;
    for st in states {
        if st.ty.as_str() == "list" {
            continue;
        }
        renders_state = true;
        let value = match st.ty.as_str() {
            "int" => format!("opurom_render::StateValue::Int(state.{})", st.name),
            "bool" => format!("opurom_render::StateValue::Bool(state.{})", st.name),
            _ => format!("opurom_render::StateValue::Str(state.{}.clone())", st.name),
        };
        state_sets.push_str(&format!("    view.set(\"{}\", {});\n", st.name, value));
    }
    // A state whose fields are all lists cannot reach the renderer's StateView;
    // read `state` so the parameter still counts as used and the build stays
    // warning-free under -D warnings. The `mut` on `view` is only needed when
    // there is at least one scalar field to set.
    let view_mut = if renders_state { "mut " } else { "" };
    if states.is_empty() || !renders_state {
        state_sets.push_str("    let _ = state;\n");
    }
    s.push_str(
        &r##"// Rendered frame: re-renders the embedded IR with the live state, then
// writes the PPM frame. Default viewport 800x600, overridable through
// OPUROM_FRAME_WIDTH and OPUROM_FRAME_HEIGHT. The frame path is the first
// argument, defaulting to the system temp directory so the state machine never
// writes into the caller's directory.
const IR_TEXT: &str = {IR_LIT};
const FRAME_WIDTH: u32 = 800;
const FRAME_HEIGHT: u32 = 600;

fn frame_width() -> u32 {
    std::env::var("OPUROM_FRAME_WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(FRAME_WIDTH)
}

fn frame_height() -> u32 {
    std::env::var("OPUROM_FRAME_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(FRAME_HEIGHT)
}

fn frame_default_path() -> String {
    std::env::temp_dir()
        .join("opurom-native-frame.ppm")
        .to_string_lossy()
        .to_string()
}

fn write_ppm(frame: &opurom_render::Frame, path: &str) -> std::io::Result<()> {
    let mut ppm = Vec::with_capacity(frame.width as usize * frame.height as usize * 3 + 32);
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", frame.width, frame.height).as_bytes());
    for px in frame.pixels.chunks_exact(4) {
        ppm.push(px[0]);
        ppm.push(px[1]);
        ppm.push(px[2]);
    }
    std::fs::write(path, ppm)
}

fn render_frame(state: &State) {
    let ir = match opurom_render::parse_ir(IR_TEXT) {
        Ok(ir) => ir,
        Err(e) => {
            println!("error: embedded IR is invalid: {e}");
            return;
        }
    };
    let {VIEW_MUT}view = opurom_render::StateView::from_ir(&ir);
{STATE_SETS}    let mut renderer = match opurom_render::Renderer::try_new(frame_width(), frame_height()) {
        Ok(renderer) => renderer,
        Err(e) => {
            println!("error: invalid viewport: {e}");
            return;
        }
    };
    let frame = renderer.render(&ir, &view);
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(frame_default_path);
    if let Err(e) = write_ppm(&frame, &out) {
        println!("error: cannot write frame: {e}");
        return;
    }
    println!("frame: {}x{} wrote {}", frame.width, frame.height, out);
    // A11y sidecar: appending a fixed suffix keeps the frame path and PPM bytes
    // untouched. The byte length is reported instead of a parsed node count, so
    // the line is deterministic and needs no JSON parser in the generated app.
    let a11y_out = format!("{out}.a11y.json");
    let a11y = opurom_render::a11y_json(&ir, &view, frame.width, frame.height);
    match std::fs::write(&a11y_out, &a11y) {
        Ok(()) => println!("a11y: {} bytes wrote {}", a11y.len(), a11y_out),
        Err(e) => println!("error: cannot write a11y tree: {e}"),
    }
}

"##
        .replace("{IR_LIT}", &ir_lit)
        .replace("{VIEW_MUT}", view_mut)
        .replace("{STATE_SETS}", &state_sets),
    );
    s.push_str("fn main() {\n    let mut state = State {\n");
    for st in states {
        s.push_str(&format!("        {}: {},\n", st.name, rust_init(st)));
    }
    s.push_str("    };\n");
    s.push_str(concat!(
        "    render_frame(&state);\n",
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
        "                render_frame(&state);\n",
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
    computed: &[Computed],
    tree: &ENode,
    ir: &Json,
    out: &Path,
    ir_path: &str,
    has_effects: bool,
) -> DResult<()> {
    write(
        out,
        "Cargo.toml",
        &native_manifest(&native_dep_root()),
        ir_path,
    )?;
    let src = native_source(app, states, computed, tree, ir, ir_path, has_effects)?;
    write(out, "src/main.rs", &src, ir_path)
}

// -------------------------------------------------------------------- web ----

fn intrinsic(n: &ENode) -> (f64, f64) {
    // Explicit width/height props override the kind default, matching
    // opurom-render's natural_w/natural_h rule.
    let pw = prop_num(&n.props, "width", 0.0);
    let ph = prop_num(&n.props, "height", 0.0);
    let (w, h) = match n.kind.as_str() {
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
        // Unknown kinds draw their fallback label; size the box to it so
        // stacked exhibits keep their line height instead of collapsing to 1px.
        // Kinds with children size from the children in layout() instead.
        _ if n.children.is_empty() => {
            let label = fallback_text(n);
            if label.is_empty() {
                (0.0, 0.0)
            } else {
                (label.chars().count() as f64 * 14.0 * 0.6 + 12.0, 24.0)
            }
        }
        _ => (0.0, 0.0),
    };
    (if pw > 0.0 { pw } else { w }, if ph > 0.0 { ph } else { h })
}

fn layout(n: &ENode, x: f64, y: f64, boxes: &mut HashMap<u64, Box2>) -> Box2 {
    let pad = prop_num(&n.props, "padding", 0.0);
    let gap = prop_num(&n.props, "gap", 4.0);
    let (iw, ih) = intrinsic(n);
    // Row stacks horizontally and every other kind stacks vertically,
    // matching opurom-render's direction() rule. Any kind with children is
    // laid out, so roots like Scroll or Grid place their subtree instead of
    // drawing it at the origin.
    let horizontal = n.kind == "Row";
    let vertical = !horizontal;
    let b = if !n.children.is_empty() {
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
            Part::Index { off, .. } => {
                return Err(interp_error(
                    s,
                    off,
                    "list index survives lowering".to_string(),
                    ir_path,
                ))
            }
        }
    }
    Ok(if exprs.is_empty() {
        "\"\"".to_string()
    } else {
        exprs.join(" + ")
    })
}

/// The Canvas 2D fill/effect statements for a node's bg/gradient/shadow/glass/border
/// props, mirroring the native renderer and the web-dom emitter. `gradient` is a
/// vertical two-stop fill (top -> bottom) and wins over `bg`; `shadow` (0..8) is a
/// soft drop shadow on the fill; `glass` is a translucent overlay; `border` strokes
/// the rect. Emits nothing when the node has no effect prop.
fn canvas_effect_fill(n: &ENode, x: &str, y: &str, w: &str, h: &str, out: &mut String) {
    let bg = n.props.get("bg").and_then(Json::as_str);
    let grad = n
        .props
        .get("gradient")
        .and_then(Json::as_str)
        .and_then(|s| s.split_once(','));
    let shadow = n.props.get("shadow").and_then(Json::as_i64).unwrap_or(0);
    let glass = n
        .props
        .get("glass")
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let border = n.props.get("border").and_then(Json::as_str);
    if grad.is_none() && bg.is_none() && shadow <= 0 && !glass && border.is_none() {
        return;
    }
    if shadow > 0 {
        out.push_str(&format!(
            "  ctx.save();\n  ctx.shadowColor = \"rgba(0, 0, 0, 0.15)\";\n  ctx.shadowBlur = {shadow};\n  ctx.shadowOffsetY = {shadow};\n"
        ));
    }
    if let Some((a, b)) = grad {
        out.push_str(&format!(
            "  var g = ctx.createLinearGradient({x}, {y}, {x}, {y} + {h});\n  g.addColorStop(0, {});\n  g.addColorStop(1, {});\n  ctx.fillStyle = g;\n  ctx.fillRect({x}, {y}, {w}, {h});\n",
            js_lit(&Json::String(a.trim().to_string())),
            js_lit(&Json::String(b.trim().to_string()))
        ));
    } else if let Some(c) = bg {
        out.push_str(&format!(
            "  ctx.fillStyle = {};\n  ctx.fillRect({x}, {y}, {w}, {h});\n",
            js_lit(&Json::String(c.to_string()))
        ));
    }
    if shadow > 0 {
        out.push_str("  ctx.restore();\n");
    }
    if glass {
        out.push_str(&format!(
            "  ctx.fillStyle = \"rgba(255, 255, 255, 0.7)\";\n  ctx.fillRect({x}, {y}, {w}, {h});\n"
        ));
    }
    if let Some(c) = border {
        out.push_str(&format!(
            "  ctx.strokeStyle = {};\n  ctx.lineWidth = 1;\n  ctx.strokeRect({x}, {y}, {w}, {h});\n",
            js_lit(&Json::String(c.to_string()))
        ));
    }
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
    if let Some(cond) = &n.unless {
        out.push_str(&format!("  if (!state.{cond}) {{\n"));
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
    canvas_effect_fill(n, &x, &y, &w, &h, out);
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
                "  ctx.fillStyle = \"#097FEB\";\n  ctx.fillRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#ffffff\";\n  ctx.font = \"16px sans-serif\";\n  ctx.fillText({label}, {x} + 10, {y} + 8);\n"
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
        "Badge" | "Avatar" => {
            let label = fallback_label(n);
            out.push_str(&format!(
                "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n"
            ));
        }
        "Icon" => {
            let color = js_lit(&Json::String(prop_str(&n.props, "color", "#334155")));
            let name = prop_str(&n.props, "name", "");
            match icons::icon_svg_path(&name) {
                // Draw the canonical 24x24 path centred on the node rect and
                // scaled to its smaller side, in the node's colour.
                Some(path) => {
                    let path_lit = js_lit(&Json::String(path.to_string()));
                    let scale = fmt_num((b.w.min(b.h) / 24.0).max(0.0));
                    let cx = fmt_num(b.x + b.w / 2.0);
                    let cy = fmt_num(b.y + b.h / 2.0);
                    out.push_str(&format!(
                        "  ctx.save();\n  ctx.strokeStyle = {color};\n  ctx.lineWidth = 2;\n  ctx.lineCap = \"round\";\n  ctx.lineJoin = \"round\";\n  ctx.translate({cx}, {cy});\n  ctx.scale({scale}, {scale});\n  ctx.stroke(new Path2D({path_lit}));\n  ctx.restore();\n"
                    ));
                }
                // An unknown name keeps the previous text fallback.
                None => {
                    let label = fallback_label(n);
                    out.push_str(&format!(
                        "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n"
                    ));
                }
            }
        }
        "Divider" | "Spinner" => {
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#cbd5e1\";\n  ctx.beginPath();\n  ctx.moveTo({x}, {y} + {h} / 2);\n  ctx.lineTo({x} + {w}, {y} + {h} / 2);\n  ctx.stroke();\n"
            ));
        }
        "Progress" => {
            let value = fmt_num(prop_num(&n.props, "value", 0.0).clamp(0.0, 100.0));
            out.push_str(&format!(
                "  ctx.fillStyle = \"#e2e8f0\";\n  ctx.fillRect({x}, {y}, {w}, {h});\n  ctx.fillStyle = \"#097FEB\";\n  ctx.fillRect({x}, {y}, {w} * {value} / 100, {h});\n"
            ));
        }
        "Switch" | "Checkbox" => {
            let bind = prop_str(&n.props, "bind", "");
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#94a3b8\";\n  ctx.strokeRect({x}, {y}, {h}, {h});\n  if (state.{bind}) {{ ctx.fillStyle = \"#097FEB\"; ctx.fillRect({x} + 3, {y} + 3, {h} - 6, {h} - 6); }}\n"
            ));
        }
        "Slider" => {
            let bind = prop_str(&n.props, "bind", "");
            let min = fmt_num(prop_num(&n.props, "min", 0.0));
            let max = fmt_num(prop_num(&n.props, "max", 100.0));
            out.push_str(&format!(
                "  ctx.strokeStyle = \"#cbd5e1\";\n  ctx.beginPath();\n  ctx.moveTo({x}, {y} + {h} / 2);\n  ctx.lineTo({x} + {w}, {y} + {h} / 2);\n  ctx.stroke();\n  ctx.fillStyle = \"#097FEB\";\n  ctx.beginPath();\n  ctx.arc({x} + {w} * (state.{bind} - ({min})) / (({max}) - ({min})), {y} + {h} / 2, 6, 0, 6.283185307179586);\n  ctx.fill();\n"
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
                "  ctx.strokeStyle = \"#94a3b8\";\n  ctx.beginPath();\n  ctx.arc({x} + {h} / 2, {y} + {h} / 2, {h} / 2 - 2, 0, 6.283185307179586);\n  ctx.stroke();\n  if (state.{bind} === {label}) {{ ctx.fillStyle = \"#097FEB\"; ctx.fill(); }}\n"
            ));
        }
        "Tabs" => {
            let labels = prop_str(&n.props, "labels", "");
            let bind = prop_str(&n.props, "bind", "");
            let label = js_lit(&Json::String(labels));
            out.push_str(&format!(
                "  ctx.fillStyle = \"#334155\";\n  ctx.font = \"14px sans-serif\";\n  ctx.fillText({label}, {x} + 6, {y} + 6);\n  if (state.{bind}) {{ ctx.fillStyle = \"#097FEB\"; ctx.fillRect({x}, {y} + {h} - 3, {w}, 3); }}\n"
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
    if n.unless.is_some() {
        out.push_str("  }\n");
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
        op @ ("add" | "sub") => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("state.{from}"),
                None => {
                    let n = action.get("value").and_then(Json::as_i64).ok_or_else(|| {
                        bad_ir(ir_path, format!("'{op}' action needs an integer 'value'"))
                    })?;
                    n.to_string()
                }
            };
            let sign = if op == "sub" { "-=" } else { "+=" };
            format!("state.{target} {sign} {rhs};")
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

fn web_effects(effects: &[Effect], ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for (index, effect) in effects.iter().enumerate() {
        match effect {
            Effect::Delay { ms } => code.push_str(&format!(
                "await new Promise(function (resolve) {{ setTimeout(resolve, {ms}); }}); "
            )),
            Effect::Fetch { path, into } => {
                if !simple_ident(into) {
                    return Err(bad_ir(
                        ir_path,
                        format!("IR fetch target '{into}' is not a valid state field"),
                    ));
                }
                code.push_str(&format!(
                    "const response{index} = await fetch({}); state.{into} = await response{index}.text(); ",
                    js_lit(&Json::String(path.clone()))
                ));
            }
        }
    }
    Ok(code)
}

fn web_computed(states: &[EState], computed: &[Computed], ir_path: &str) -> DResult<String> {
    if computed.is_empty() {
        return Ok(String::new());
    }
    let mut s = String::from("function applyComputed() {\n");
    for c in computed {
        let (_, expr) = computed_expr(&c.expr, states, computed, ir_path, false, |st| {
            Ok(format!("state.{}", st.name))
        })?;
        s.push_str(&format!("  state.{} = {};\n", c.name, expr));
    }
    s.push_str("}\n\n");
    Ok(s)
}

/// Emits the responsive DOM page by delegating to the sibling `opurom-web`
/// emitter, which bakes the IR into static HTML/CSS at build time. The
/// compiler shells out rather than embedding the emitter: the DOM emitter is a
/// Node tool, so reimplementing it here would duplicate it.
fn emit_web_dom(ir_path: &str, out: &Path) -> DResult<()> {
    let emitter = web_dom_emitter_path();
    if !emitter.is_file() {
        return Err(Diagnostic::file(
            ir_path,
            format!("responsive DOM emitter not found: {}", emitter.display()),
            "keep the opurom-web sibling next to opurom-lang, or build with --target web",
        ));
    }
    let output = Command::new("node")
        .arg(&emitter)
        .arg("emit")
        .arg(ir_path)
        .arg(out)
        .output()
        .map_err(|e| {
            Diagnostic::file(
                ir_path,
                format!("cannot run the responsive DOM emitter: {e}"),
                "install Node.js on PATH and build again",
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Diagnostic::file(
            ir_path,
            format!("responsive DOM emitter failed: {}", stderr.trim()),
            "run the opurom-web emit command by hand to see the full detail",
        ));
    }
    Ok(())
}

fn emit_web(
    ir: &Json,
    app: &str,
    states: &[EState],
    computed: &[Computed],
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
    let computed_fn = web_computed(states, computed, ir_path)?;
    let computed_call = if computed.is_empty() {
        ""
    } else {
        "applyComputed(); "
    };
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
            let effect_code = web_effects(&event_effects(action, ir_path)?, ir_path)?;
            let code = js_action_chain(action, ir_path)?;
            tap_arms.push_str(&format!(
                "  if (id === {}) {{ {effect_code}{code}  {computed_call}render(); return; }}\n",
                n.id
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
                "  if (id === {}) {{ state.{} = String(text); {computed_call}render(); return; }}\n",
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
    app_js.push_str(&computed_fn);
    app_js.push_str("function render() {\n");
    app_js.push_str("  ctx.clearRect(0, 0, canvas.width, canvas.height);\n");
    app_js.push_str("  ctx.textBaseline = \"top\";\n");
    app_js.push_str(&draw);
    app_js.push_str("}\n\n");
    app_js.push_str("async function applyTap(id) {\n");
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
    if computed.is_empty() {
        app_js.push_str("render();\n");
    } else {
        app_js.push_str("applyComputed();\nrender();\n");
    }
    write(out, "app.js", &app_js, ir_path)?;

    let title = html_attr(app);
    // Entry animation: when any node carries an `animate` curve, the canvas
    // fades/scales in once on load, mirroring the native per-node entry.
    let web_anim = nodes.iter().any(|n| !anim_class(&n.props).is_empty());
    let anim_css = if web_anim {
        "@keyframes opurom-entry { from { transform: scale(0); opacity: 0; } to { transform: scale(1); opacity: 1; } } canvas.opu-anim { transform-origin: center; animation: opurom-entry 480ms ease-out both; }"
    } else {
        ""
    };
    let canvas_cls = if web_anim { " class=\"opu-anim\"" } else { "" };
    // Canvas 2D has no DOM semantics, so every node carrying an explicit role
    // is mirrored into a visually hidden, screen-reader-readable element. A
    // page without explicit roles emits an empty mirror container.
    let mut a11y_mirror = String::new();
    for n in &nodes {
        if let Some(role) = n.props.get("role").and_then(Json::as_str) {
            a11y_mirror.push_str(&format!(
                "<div role=\"{}\" aria-label=\"{}\"></div>",
                html_attr(role),
                html_attr(&fallback_text(n))
            ));
        }
    }
    let sr_css = ".sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}";
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>body {{ font-family: sans-serif; margin: 0; padding: 16px; }} canvas {{ background: #fff; border: 1px solid #ddd; }} #inputs {{ margin-top: 12px; }}{sr_css}{anim_css}</style>\n</head>\n<body>\n<canvas id=\"app\" width=\"800\" height=\"600\"{canvas_cls}></canvas>\n<div id=\"inputs\">\n{input_html}</div>\n<div id=\"a11y\" class=\"sr-only\">{a11y_mirror}</div>\n<script src=\"app.js\"></script>\n</body>\n</html>\n"
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
    // Visual effects, mirroring the native renderer and the web-dom emitter.
    // `bg` is a solid fill; `gradient` ("#A,#B") is a vertical two-stop fill and
    // wins over `bg`; `shadow` (0..8) is a soft box-shadow; `glass` is a
    // frosted translucent fill; `border` is a 1px outline.
    let bg = props.get("bg").and_then(Json::as_str);
    let grad = props
        .get("gradient")
        .and_then(Json::as_str)
        .and_then(|s| s.split_once(','));
    if let Some((a, b)) = grad {
        parts.push(format!(
            "background:linear-gradient(to bottom, {}, {})",
            a.trim(),
            b.trim()
        ));
    } else if let Some(c) = bg {
        parts.push(format!("background-color:{c}"));
    }
    if let Some(n) = props.get("shadow").and_then(Json::as_i64) {
        if n > 0 {
            parts.push(format!("box-shadow:0 {n}px {n}px rgba(0, 0, 0, 0.15)"));
        }
    }
    if props.get("glass").and_then(Json::as_bool).unwrap_or(false) {
        parts.push("backdrop-filter:blur(6px)".to_string());
        parts.push("background:rgba(255, 255, 255, 0.7)".to_string());
    }
    if let Some(Json::String(c)) = props.get("border") {
        parts.push(format!("border:1px solid {c}"));
    }
    parts.join(";")
}

/// The entry-animation CSS class for a node's `animate` prop, mirroring the
/// native renderer and the web-dom emitter: spring/ease/inertia animate in from
/// the centre, `none`/absent is static. The curve is mapped to a CSS timing
/// function (spring is a documented cubic-bezier approximation).
fn anim_class(props: &JsonMap<String, Json>) -> String {
    let Some(Json::String(curve)) = props.get("animate") else {
        return String::new();
    };
    match curve.as_str() {
        "spring" => " anim-entry anim-spring".to_string(),
        "ease" => " anim-entry anim-ease".to_string(),
        "inertia" => " anim-entry anim-inertia".to_string(),
        _ => String::new(),
    }
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
            Part::Index { off, .. } => {
                return Err(interp_error(
                    s,
                    off,
                    "list index survives lowering".to_string(),
                    ir_path,
                ))
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
    let anim = anim_class(&n.props);
    let style_attr = if style.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", html_attr(&style))
    };
    let condition = match (&n.when, &n.unless) {
        (Some(w), Some(u)) => format!("{w} && !{u}"),
        (Some(w), None) => w.clone(),
        (None, Some(u)) => format!("!{u}"),
        (None, None) => String::new(),
    };
    let cond_attr = if condition.is_empty() {
        String::new()
    } else {
        format!(" {}=\"{}{}{}\"", p.cond_attr, p.open, condition, p.close)
    };
    // Mini runtimes read `aria-role`; an accessible name comes from the first
    // label/value/title/name prop. Only an explicit role produces attributes,
    // so a node without one stays byte-identical to the previous output.
    let a11y_attr = match n.props.get("role").and_then(Json::as_str) {
        Some(role) => {
            let label = fallback_text(n);
            let label_attr = if label.is_empty() {
                String::new()
            } else {
                format!(" aria-label=\"{}\"", html_attr(&label))
            };
            format!(" aria-role=\"{}\"{label_attr}", html_attr(role))
        }
        None => String::new(),
    };
    match n.kind.as_str() {
        "Text" => {
            let value = markup_interp(&prop_str(&n.props, "value", ""), p, ir_path)?;
            out.push_str(&format!(
                "{pad}<text class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}>{value}</text>\n"
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
                "{pad}<button class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}{tap}>{label}</button>\n"
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
                "{pad}<input class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr} placeholder=\"{placeholder}\"{value}{input} />\n"
            ));
        }
        "Image" => {
            let src = html_attr(&prop_str(&n.props, "src", ""));
            out.push_str(&format!(
                "{pad}<image class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr} src=\"{src}\" />\n"
            ));
        }
        "Spacer" => {
            out.push_str(&format!(
                "{pad}<view class=\"spacer n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}></view>\n"
            ));
        }
        "Badge" | "Avatar" | "Progress" | "Switch" | "Slider" | "Checkbox" | "Radio" | "Tabs" => {
            let label = markup_text(&fallback_text(n));
            let tap = if n.events.contains_key("tap") {
                format!(" {}=\"onTap{id}\"", p.tap_attr)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{pad}<view class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}{tap}><text>{label}</text></view>\n"
            ));
        }
        "Icon" => {
            let name = prop_str(&n.props, "name", "");
            let tap = if n.events.contains_key("tap") {
                format!(" {}=\"onTap{id}\"", p.tap_attr)
            } else {
                String::new()
            };
            match icons::icon_svg_path(&name) {
                // A real icon element: an inline SVG data URI on an <image>, so
                // no platform runtime is needed and every target draws the same
                // canonical path the native renderer uses.
                Some(path) => {
                    let svg = format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"{path}\"/></svg>"
                    );
                    let b64 = base64_encode(&svg);
                    let mut style = style_of(&n.props, false);
                    if !style.contains("width:") {
                        style = if style.is_empty() {
                            "width:16px;height:16px".to_string()
                        } else {
                            format!("{style};width:16px;height:16px")
                        };
                    }
                    let style_attr = if style.is_empty() {
                        String::new()
                    } else {
                        format!(" style=\"{}\"", html_attr(&style))
                    };
                    out.push_str(&format!(
                        "{pad}<image class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}{tap} src=\"data:image/svg+xml;base64,{b64}\" />\n"
                    ));
                }
                // An unknown name keeps the previous text fallback.
                None => {
                    let label = markup_text(&fallback_text(n));
                    out.push_str(&format!(
                        "{pad}<view class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}{tap}><text>{label}</text></view>\n"
                    ));
                }
            }
        }
        "Divider" | "Spinner" => {
            let label = markup_text(&fallback_text(n));
            let inner = if label.is_empty() {
                String::new()
            } else {
                format!("<text>{label}</text>")
            };
            out.push_str(&format!(
                "{pad}<view class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}>{inner}</view>\n"
            ));
        }
        kind => {
            // Documented fallback: an unknown kind renders as a Container
            // showing its label, so gallery exhibits stay visible in pages.
            let cls = if kind == "Row" { "row" } else { "col" };
            out.push_str(&format!(
                "{pad}<view class=\"{cls} n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr}>\n"
            ));
            if n.children.is_empty() {
                let label = markup_text(&fallback_text(n));
                if !label.is_empty() {
                    out.push_str(&format!("{pad}  <text>{label}</text>\n"));
                }
            }
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
        op @ ("add" | "sub") => {
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("this.data.{from}"),
                None => {
                    let n = action.get("value").and_then(Json::as_i64).ok_or_else(|| {
                        bad_ir(ir_path, format!("'{op}' action needs an integer 'value'"))
                    })?;
                    n.to_string()
                }
            };
            let sign = if op == "sub" { '-' } else { '+' };
            format!("this.{set}({{\"{target}\": this.data.{target} {sign} ({rhs})}});")
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

fn mini_effects(effects: &[Effect], p: &Platform, ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::Fetch { .. }))
    {
        code.push_str("const page = this; ");
    }
    for effect in effects {
        match effect {
            Effect::Delay { ms } => code.push_str(&format!(
                "await new Promise(function (resolve) {{ setTimeout(resolve, {ms}); }}); "
            )),
            Effect::Fetch { path, into } => {
                if !simple_ident(into) {
                    return Err(bad_ir(
                        ir_path,
                        format!("IR fetch target '{into}' is not a valid state field"),
                    ));
                }
                let path = js_lit(&Json::String(path.clone()));
                let target = js_lit(&Json::String(into.clone()));
                code.push_str("await new Promise(function (resolve) { ");
                code.push_str(&format!(
                    "{}({{url: {path}, method: \"GET\", success: function (res) {{ ",
                    p.request_api
                ));
                code.push_str(&format!(
                    "page.{}({{{target}: typeof res.data === \"string\" ? res.data : JSON.stringify(res.data)}}); resolve(); ",
                    p.state_set
                ));
                code.push_str("}, fail: function () { ");
                code.push_str(&format!(
                    "page.{}({{{target}: \"error: request failed\"}}); resolve(); ",
                    p.state_set
                ));
                code.push_str("}}); }); ");
            }
        }
    }
    Ok(code)
}

fn mini_computed(
    p: &Platform,
    states: &[EState],
    computed: &[Computed],
    ir_path: &str,
) -> DResult<String> {
    if computed.is_empty() {
        return Ok(String::new());
    }
    let mut entries = Vec::new();
    for c in computed {
        let (_, expr) = computed_expr(&c.expr, states, computed, ir_path, false, |st| {
            Ok(format!("this.data.{}", st.name))
        })?;
        entries.push(format!(
            "{}: {}",
            js_lit(&Json::String(c.name.clone())),
            expr
        ));
    }
    Ok(format!(
        "  applyComputed: function () {{\n    this.{}({{{}}});\n  }},\n",
        p.state_set,
        entries.join(", ")
    ))
}

fn emit_mini(
    target: &str,
    app: &str,
    states: &[EState],
    computed: &[Computed],
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
    // app.js is a required root entry in the mini-program package structure;
    // pages are resolved from the package root without file extensions.
    let app_js = format!(
        "// Generated by opuromc for {}; no IR interpreter.\nApp({{}});\n",
        p.key
    );
    write(out, "app.js", &app_js, ir_path)?;

    let mut markup = String::new();
    markup.push_str(&format!(
        "<!-- Generated by opuromc for {}; no IR interpreter. -->\n",
        p.key
    ));
    markup_node(tree, p, 0, &mut markup, ir_path)?;
    write(
        out,
        &format!("pages/index/index.{}", p.markup_ext),
        &markup,
        ir_path,
    )?;

    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut style = String::from(
        ".col { display: flex; flex-direction: column; }\n.row { display: flex; flex-direction: row; }\n@keyframes opurom-entry { from { transform: scale(0); opacity: 0; } to { transform: scale(1); opacity: 1; } }\n.anim-entry { transform-origin: center; animation: opurom-entry 480ms ease-out both; }\n.anim-spring { animation-timing-function: cubic-bezier(0.34, 1.56, 0.64, 1); }\n.anim-ease { animation-timing-function: ease-out; }\n.anim-inertia { animation-timing-function: ease-in-out; }\n",
    );
    for n in &nodes {
        let s = style_of(&n.props, n.kind == "Container");
        if !s.is_empty() {
            style.push_str(&format!(".n{} {{ {}; }}\n", n.id, s));
        }
    }
    write(
        out,
        &format!("pages/index/index.{}", p.style_ext),
        &style,
        ir_path,
    )?;

    let mut data = JsonMap::new();
    for st in states {
        data.insert(st.name.clone(), st.init.clone());
    }
    let data_js = js_lit(&Json::Object(data));
    let mut methods = mini_computed(p, states, computed, ir_path)?;
    let computed_call = if computed.is_empty() {
        ""
    } else {
        " this.applyComputed();"
    };
    for n in &nodes {
        if let Some(action) = n.events.get("tap") {
            let effect_code = mini_effects(&event_effects(action, ir_path)?, p, ir_path)?;
            let code = mini_action_chain(action, p, ir_path)?;
            methods.push_str(&format!(
                "  onTap{}: async function () {{ {effect_code}{code}{computed_call} }},\n",
                n.id
            ));
        }
        if let Some(action) = n.events.get("input") {
            let target_name = action_str(action, "target", ir_path)?;
            methods.push_str(&format!(
                "  onInput{}: function (e) {{ this.{}({{\"{}\": e.detail.value}});{} }},\n",
                n.id, p.state_set, target_name, computed_call
            ));
        }
    }
    let index_js = format!(
        "// Generated by opuromc for {}; straight-line code, no IR interpreter.\nPage({{\n  data: {},\n{}}});\n",
        p.key, data_js, methods
    );
    write(out, "pages/index/index.js", &index_js, ir_path)
}
