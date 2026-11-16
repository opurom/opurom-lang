use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};
use crate::icons;
use crate::lower::{interp_parts, offset_pos, Part, DESIGN_PROPS, EXTENDED_PRIMITIVE_KINDS};
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
    (
        "harmony",
        "oh-package.json5, build-profile.json5, hvigorfile.ts, AppScope/app.json5, AppScope/resources/base/element/string.json, src/main/module.json5, src/main/ets/entryability/EntryAbility.ets, src/main/ets/pages/Index.ets, src/main/ets/pages/Index.ir.json, src/main/resources/base/profile/main_pages.json",
    ),
];

/// Additive `Input` validator props preserved in generated metadata and markup.
const VALIDATOR_PROPS: &[&str] = &["required", "minLength", "maxLength", "pattern"];

/// `Input` text-buffer props forwarded verbatim. `textBuffer`/`secure` are
/// booleans and `value` is a string; each rides on the Input as a `data-*`
/// attribute so every target's primary artifact carries it.
const TEXT_BUFFER_PROPS: &[&str] = &["textBuffer", "secure", "value"];

/// Responsive DOM build target. It is deliberately kept out of [`TARGETS`] so
/// `opuromc targets` still reports exactly the ten 0.1.0 IR emitters; it is
/// dispatched explicitly in [`emit`].
pub const WEB_DOM_TARGET: &str = "web-dom";

/// Platform-adaptive system font stack. A CSS `font-family` list resolves to the
/// first family installed on the host, so one declaration renders PingFang on
/// macOS/iOS, Microsoft YaHei on Windows, and Noto Sans CJK / Roboto on Linux
/// and Android without any per-OS branch. Concrete family names (not the
/// `-apple-system` keyword) keep the list valid inside a canvas `ctx.font`.
pub const SYSTEM_FONT_STACK: &str =
    "'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Microsoft JhengHei', \
     'Segoe UI', Roboto, 'Noto Sans CJK SC', 'Source Han Sans SC', 'Noto Sans CJK JP', \
     'Helvetica Neue', Arial, sans-serif";

/// Effective font stack for an IR: a theme `fontFamily` leads the system stack
/// so a project can ship a bundled typeface while keeping every OS fallback.
pub fn theme_font_stack(ir: &Json) -> String {
    match ir
        .get("theme")
        .and_then(|t| t.get("fontFamily"))
        .and_then(|v| v.as_str())
    {
        Some(custom) if !custom.trim().is_empty() => format!("{custom}, {SYSTEM_FONT_STACK}"),
        _ => SYSTEM_FONT_STACK.to_string(),
    }
}

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
    /// Additive repeatable-subtree metadata: `{"item": ..., "list": ...}`.
    pub repeat: Option<Json>,
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
    touch_start_attr: &'static str,
    touch_move_attr: &'static str,
    touch_end_attr: &'static str,
    state_set: &'static str,
    request_api: &'static str,
    navigate_api: &'static str,
    navigate_back_api: &'static str,
    stop_refresh_api: &'static str,
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
        touch_start_attr: "bindtouchstart",
        touch_move_attr: "bindtouchmove",
        touch_end_attr: "bindtouchend",
        state_set: "setData",
        request_api: "wx.request",
        navigate_api: "wx.navigateTo",
        navigate_back_api: "wx.navigateBack",
        stop_refresh_api: "wx.stopPullDownRefresh",
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
        touch_start_attr: "onTouchStart",
        touch_move_attr: "onTouchMove",
        touch_end_attr: "onTouchEnd",
        state_set: "setData",
        request_api: "my.request",
        navigate_api: "my.navigateTo",
        navigate_back_api: "my.navigateBack",
        stop_refresh_api: "my.stopPullDownRefresh",
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
        touch_start_attr: "bindtouchstart",
        touch_move_attr: "bindtouchmove",
        touch_end_attr: "bindtouchend",
        state_set: "setData",
        request_api: "qq.request",
        navigate_api: "qq.navigateTo",
        navigate_back_api: "qq.navigateBack",
        stop_refresh_api: "qq.stopPullDownRefresh",
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
        touch_start_attr: "bindtouchstart",
        touch_move_attr: "bindtouchmove",
        touch_end_attr: "bindtouchend",
        state_set: "setData",
        request_api: "tt.request",
        navigate_api: "tt.navigateTo",
        navigate_back_api: "tt.navigateBack",
        stop_refresh_api: "tt.stopPullDownRefresh",
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
        touch_start_attr: "bindtouchstart",
        touch_move_attr: "bindtouchmove",
        touch_end_attr: "bindtouchend",
        state_set: "setData",
        request_api: "swan.request",
        navigate_api: "swan.navigateTo",
        navigate_back_api: "swan.navigateBack",
        stop_refresh_api: "swan.stopPullDownRefresh",
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
        touch_start_attr: "bindtouchstart",
        touch_move_attr: "bindtouchmove",
        touch_end_attr: "bindtouchend",
        state_set: "setData",
        request_api: "ks.request",
        navigate_api: "ks.navigateTo",
        navigate_back_api: "ks.navigateBack",
        stop_refresh_api: "ks.stopPullDownRefresh",
        title_key: "navigationBarTitleText",
        open: "{{",
        close: "}}",
    },
];

pub fn bad_ir(ir_path: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::file(
        ir_path,
        message,
        "compile the .opu source with 'opuromc compile' to produce valid IR 0.1.0",
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
    // the IR format declares a closed event-name set. Reject an undeclared name
    // here, at the shared entry for all ten emitters, instead of silently
    // dropping the binding: the web-dom validator applies the same rule.
    if let Some(unknown) = events.keys().find(|k| {
        !matches!(
            k.as_str(),
            "tap" | "input" | "doubleTap" | "longPress" | "drag"
        )
    }) {
        return Err(bad_ir(
            ir_path,
            format!("IR node has unknown event name '{unknown}' (expected tap|input|doubleTap|longPress|drag)"),
        ));
    }
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
    let repeat = match o.get("repeat") {
        Some(Json::Object(fields)) => {
            if fields
                .keys()
                .any(|k| !matches!(k.as_str(), "item" | "list"))
                || fields.get("item").and_then(Json::as_str).is_none()
                || fields.get("list").and_then(Json::as_str).is_none()
            {
                return Err(bad_ir(
                    ir_path,
                    "IR node 'repeat' needs string 'item' and 'list' fields",
                ));
            }
            o.get("repeat").cloned()
        }
        Some(_) => return Err(bad_ir(ir_path, "IR node 'repeat' must be an object")),
        None => None,
    };
    Ok(ENode {
        id,
        kind,
        props,
        events,
        children,
        when,
        unless,
        repeat,
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

/// One page of a multi-page IR: its route name, optional title, and content tree.
pub struct EPage {
    pub name: String,
    pub title: Option<String>,
    pub tree: ENode,
    /// PLATFORM_DELIVERY.md §7: page-level pull-refresh effects, reusing the
    /// fetch/delay shapes of an async handler.
    pub pull_refresh: Option<Vec<Effect>>,
    /// PLATFORM_DELIVERY.md §7: subpackage name this route belongs to.
    pub subpackage: Option<String>,
}

/// The additive multi-page shape: an initial route plus an ordered route table.
/// Single-page IR has neither field, so `read_routes` returns `None` and every
/// emitter keeps its byte-identical single-page output.
pub struct ERoutes {
    pub initial: String,
    pub pages: Vec<EPage>,
}

/// A route name is used verbatim as a mini-program page path segment and as a
/// `data-current-route` value, so it must be a plain identifier: no slashes,
/// dots or quotes can reach a generated path or attribute.
fn valid_route_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Read the additive `initialRoute` + `routes` record. Returns `None` for a
/// single-page IR that has only a top-level `tree`.
fn read_routes(ir: &Json, ir_path: &str) -> DResult<Option<ERoutes>> {
    let Some(routes) = ir.get("routes") else {
        return Ok(None);
    };
    let map = routes
        .as_object()
        .ok_or_else(|| bad_ir(ir_path, "IR 'routes' must be an object"))?;
    if map.is_empty() {
        return Err(bad_ir(ir_path, "IR 'routes' must not be empty"));
    }
    let mut pages = Vec::with_capacity(map.len());
    for (name, route) in map {
        if !valid_route_name(name) {
            return Err(bad_ir(
                ir_path,
                format!("route name '{name}' must be an identifier (letters, digits, '_' or '-')"),
            ));
        }
        let obj = route
            .as_object()
            .ok_or_else(|| bad_ir(ir_path, format!("route '{name}' must be an object")))?;
        let tree_value = obj
            .get("tree")
            .ok_or_else(|| bad_ir(ir_path, format!("route '{name}' is missing 'tree'")))?;
        let tree = read_node(tree_value, ir_path)?;
        let title = obj.get("title").and_then(Json::as_str).map(str::to_string);
        // pullRefresh mirrors event_effects: fetch/delay only, `into` a declared
        // string|list state is checked by the caller against the state table.
        let pull_refresh = match obj.get("pullRefresh") {
            None => None,
            Some(pr) => {
                let inner = pr.as_object().ok_or_else(|| {
                    bad_ir(
                        ir_path,
                        format!("route '{name}' pullRefresh must be an object"),
                    )
                })?;
                let raw = inner.get("effects").ok_or_else(|| {
                    bad_ir(
                        ir_path,
                        format!("route '{name}' pullRefresh is missing 'effects'"),
                    )
                })?;
                let effects = parse_effect_list(raw, ir_path)?;
                if effects.is_empty() {
                    return Err(bad_ir(
                        ir_path,
                        format!("route '{name}' pullRefresh must declare at least one effect"),
                    ));
                }
                Some(effects)
            }
        };
        let subpackage = match obj.get("subpackage") {
            None => None,
            Some(v) => {
                let s = v.as_str().ok_or_else(|| {
                    bad_ir(
                        ir_path,
                        format!("route '{name}' subpackage must be a string"),
                    )
                })?;
                if !valid_route_name(s) {
                    return Err(bad_ir(
                        ir_path,
                        format!("route '{name}' subpackage '{s}' must be an identifier"),
                    ));
                }
                Some(s.to_string())
            }
        };
        pages.push(EPage {
            name: name.clone(),
            title,
            tree,
            pull_refresh,
            subpackage,
        });
    }
    // serde_json maps keep insertion order only with the preserve_order feature;
    // the emitted `routes` object is authored, so honour `initialRoute` when it
    // is a known route, else fall back to the first route in document order.
    let initial = match ir.get("initialRoute").and_then(Json::as_str) {
        Some(name) if map.contains_key(name) => name.to_string(),
        _ => pages[0].name.clone(),
    };
    Ok(Some(ERoutes { initial, pages }))
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
                // A push is a navigation signal and validate is a built-in form
                // signal; neither target is a mutable state.
                if matches!(
                    action.get("op").and_then(Json::as_str),
                    Some("push" | "validate")
                ) {
                    continue;
                }
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

fn validation_attrs(props: &JsonMap<String, Json>) -> String {
    let mut out = String::new();
    for key in VALIDATOR_PROPS.iter().chain(TEXT_BUFFER_PROPS.iter()) {
        if let Some(value) = props.get(*key) {
            let rendered = match value {
                Json::String(s) => s.clone(),
                other => other.to_string(),
            };
            let attr = match *key {
                "minLength" => "min-length",
                "maxLength" => "max-length",
                "textBuffer" => "text-buffer",
                other => other,
            };
            out.push_str(&format!(" data-{}=\"{}\"", attr, html_attr(&rendered)));
        }
    }
    out
}

/// Whether a prop key is one of the interaction/variant/tone design props.
fn is_design_prop(key: &str) -> bool {
    DESIGN_PROPS.iter().any(|(k, _)| *k == key)
}

/// Preserve the design props on every node as `data-*` attributes, so a
/// mini-program stylesheet or script can drive the same states the renderer
/// draws. Nodes without any of them emit nothing (byte-identical output).
fn design_attrs(props: &JsonMap<String, Json>) -> String {
    let mut out = String::new();
    for (key, _) in DESIGN_PROPS {
        if let Some(Json::String(value)) = props.get(*key) {
            out.push_str(&format!(" data-{}=\"{}\"", key, html_attr(value)));
        }
    }
    out
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
    parse_effect_list(raw, ir_path)
}

/// Parse an `effects` array sharing the fetch/delay shapes. Also used by a
/// route's `pullRefresh` (§7), which reuses these shapes without the async flag.
fn parse_effect_list(raw: &Json, ir_path: &str) -> DResult<Vec<Effect>> {
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

/// An event is either one action object (0.1.0), `{"actions":[...]}` (interactive UI), or
/// effects only (async language layer).
pub fn event_actions(event: &Json) -> Vec<&Json> {
    match event.get("actions").and_then(Json::as_array) {
        Some(list) => list.iter().collect(),
        None if event.get("op").is_some() => vec![event],
        None => Vec::new(),
    }
}

/// Preserve repeat, validator, validate-action and design metadata in targets
/// that do not have native form or builder runtimes yet.
pub fn additive_meta(tree: &ENode) -> Option<String> {
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut repeat = Vec::new();
    let mut validation = Vec::new();
    let mut validate = Vec::new();
    let mut design = Vec::new();
    for n in &nodes {
        if let Some(meta) = &n.repeat {
            repeat.push(json!({"id": n.id, "repeat": meta.clone()}));
        }
        let mut props = JsonMap::new();
        for key in VALIDATOR_PROPS {
            if let Some(value) = n.props.get(*key) {
                props.insert((*key).to_string(), value.clone());
            }
        }
        if !props.is_empty() {
            validation.push(json!({"id": n.id, "props": Json::Object(props)}));
        }
        let mut dprops = JsonMap::new();
        for (key, _) in DESIGN_PROPS {
            if let Some(value) = n.props.get(*key) {
                dprops.insert((*key).to_string(), value.clone());
            }
        }
        if !dprops.is_empty() {
            design.push(json!({"id": n.id, "props": Json::Object(dprops)}));
        }
        for (event, action) in &n.events {
            for one in event_actions(action) {
                if one.get("op").and_then(Json::as_str) == Some("validate") {
                    validate.push(json!({
                        "id": n.id,
                        "event": event.clone(),
                        "action": one.clone(),
                    }));
                }
            }
        }
    }
    if repeat.is_empty() && validation.is_empty() && validate.is_empty() && design.is_empty() {
        return None;
    }
    // Only add the `design` field when present, so IR without the new props
    // keeps byte-identical metadata.
    let meta = if design.is_empty() {
        json!({
            "repeat": repeat,
            "validation": validation,
            "validate": validate,
        })
    } else {
        json!({
            "repeat": repeat,
            "validation": validation,
            "validate": validate,
            "design": design,
        })
    };
    serde_json::to_string(&meta).ok()
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
        Some("0.1.0") => {}
        Some(v) => {
            return Err(Diagnostic::file(
                ir_path,
                format!("unsupported IR version '{v}', expected '0.1.0'"),
                "recompile the source with this version of opuromc",
            ))
        }
        None => {
            return Err(Diagnostic::file(
                ir_path,
                "missing 'ir' version field",
                "IR 0.1.0 must contain \"ir\": \"0.1.0\"",
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
    let states = read_state(&state_values, ir_path)?;
    let computed = read_computed(&ir, &states, ir_path)?;
    // The additive `routes` record means multi-page; otherwise a single top-level
    // `tree` drives the byte-identical single-page emitters.
    let routes = read_routes(&ir, ir_path)?;
    if let Some(routes) = routes {
        for page in &routes.pages {
            validate_actions(&states, &computed, &page.tree, ir_path)?;
        }
        let has_effects = routes
            .pages
            .iter()
            .map(|p| tree_has_effects(&states, &p.tree, ir_path))
            .collect::<DResult<Vec<_>>>()?
            .into_iter()
            .any(|v| v);
        fs::create_dir_all(out).map_err(|e| {
            Diagnostic::file(
                ir_path,
                format!("cannot create output directory {}: {e}", out.display()),
                "check the -o path and permissions",
            )
        })?;
        return match target {
            "native" => emit_native_multipage(
                &app,
                &states,
                &computed,
                &routes,
                &ir,
                out,
                ir_path,
                has_effects,
            ),
            "web" => emit_web_multipage(&ir, &app, &states, &computed, &routes, out, ir_path),
            "web-dom" => emit_web_dom(ir_path, out),
            "wasm" => crate::wasm::emit_wasm_multipage(
                &app,
                &states,
                &computed,
                &routes,
                out,
                ir_path,
                has_effects,
                &theme_font_stack(&ir),
            ),
            "harmony" => emit_harmony(
                &app,
                &states,
                &computed,
                &harmony_pages(&routes),
                &routes.initial,
                true,
                &ir,
                out,
                ir_path,
            ),
            _ => emit_mini_multipage(target, &app, &states, &computed, &routes, &ir, out, ir_path),
        };
    }
    let tree_value = ir
        .get("tree")
        .ok_or_else(|| bad_ir(ir_path, "IR is missing the 'tree' field"))?;
    let tree = read_node(tree_value, ir_path)?;
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
        "wasm" => crate::wasm::emit_wasm(
            &app,
            &states,
            &computed,
            &tree,
            out,
            ir_path,
            has_effects,
            &theme_font_stack(&ir),
        ),
        "harmony" => {
            let pages = [("Index".to_string(), &tree)];
            emit_harmony(
                &app, &states, &computed, &pages, "Index", false, &ir, out, ir_path,
            )
        }
        _ => emit_mini(target, &app, &states, &computed, &tree, &ir, out, ir_path),
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
    // A pop carries no target; it is a reserved "@pop" navigation signal.
    if op == "pop" {
        return Ok((
            String::new(),
            "Patch { name: \"@nav\", value: \"@pop\".to_string() }".to_string(),
        ));
    }
    let target = action_str(action, "target", ir_path)?;
    // A push is a navigation signal, mirroring the kernel's reserved patch.
    if op == "push" {
        return Ok((
            String::new(),
            format!(
                "Patch {{ name: \"@nav\", value: {}.to_string() }}",
                rust_str(target)
            ),
        ));
    }
    if op == "validate" {
        return Ok((
            String::new(),
            format!(
                "Patch {{ name: \"@validate\", value: {}.to_string() }}",
                rust_str(target)
            ),
        ));
    }
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
        "// Generated by opuromc for app {app}; lowered from IR 0.1.0. Do not edit.\n"
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
        "fn apply(_state: &mut State, event_node_id: u32, event: &str, _input: Option<&str>) -> Vec<Patch> {\n",
    );
    s.push_str("    match (event_node_id, event) {\n");
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    for n in &nodes {
        for event in ["tap", "doubleTap", "longPress", "drag", "input"] {
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
                        "        ({}, {:?}) => {{\n{effect_code}{body}            {patch_expr}\n        }}\n",
                        n.id, event
                    ));
                } else {
                    s.push_str(&format!(
                        "        ({}, {:?}) => {{\n{effect_code}{body}            let mut patches = {patch_expr};\n            patches.extend(apply_computed(_state));\n            patches\n        }}\n",
                        n.id, event
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
    // through Opurom_FRAME_WIDTH/HEIGHT; the frame path is the first argument,
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
        &r##"// Rendered frame: re-renders the current IR with the live state, then
// writes the PPM frame. The IR text starts as the embedded constant and can
// be swapped at startup through Opurom_IR_PATH or at runtime through the
// `reload` stdin command. Default viewport 800x600, overridable through
// Opurom_FRAME_WIDTH and Opurom_FRAME_HEIGHT. The frame path is the first
// argument, defaulting to the system temp directory so the state machine never
// writes into the caller's directory.
const IR_TEXT: &str = {IR_LIT};
const FRAME_WIDTH: u32 = 800;
const FRAME_HEIGHT: u32 = 600;

fn frame_width() -> u32 {
    std::env::var("Opurom_FRAME_WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(FRAME_WIDTH)
}

fn frame_height() -> u32 {
    std::env::var("Opurom_FRAME_HEIGHT")
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

fn render_frame(state: &State, ir_text: &str) {
    let ir = match opurom_render::parse_ir(ir_text) {
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
        "    // The embedded constant stays the default; Opurom_IR_PATH swaps it\n",
        "    // at startup so a preview host can hand the binary fresh IR without\n",
        "    // rebuilding. A bad override keeps the embedded IR.\n",
        "    let mut ir_text = IR_TEXT.to_string();\n",
        "    if let Ok(path) = std::env::var(\"Opurom_IR_PATH\") {\n",
        "        match std::fs::read_to_string(&path) {\n",
        "            Ok(text) => match opurom_render::parse_ir(&text) {\n",
        "                Ok(_) => ir_text = text,\n",
        "                Err(e) => println!(\"error: cannot load IR from {path}: {e}\"),\n",
        "            },\n",
        "            Err(e) => println!(\"error: cannot load IR from {path}: {e}\"),\n",
        "        }\n",
        "    }\n",
        "    render_frame(&state, &ir_text);\n",
        "    let stdin = io::stdin();\n",
        "    for line in stdin.lock().lines() {\n",
        "        let line = match line { Ok(l) => l, Err(_) => break };\n",
        "        let parts: Vec<&str> = line.split_whitespace().collect();\n",
        "        if parts.is_empty() { continue; }\n",
        "        match parts[0] {\n",
        "            \"quit\" => break,\n",
        "            \"reload\" => {\n",
        "                if parts.len() < 2 {\n",
        "                    println!(\"error: usage: reload <path>\");\n",
        "                    continue;\n",
        "                }\n",
        "                let path = parts[1..].join(\" \");\n",
        "                match std::fs::read_to_string(&path) {\n",
        "                    Ok(text) => match opurom_render::parse_ir(&text) {\n",
        "                        Ok(_) => {\n",
        "                            ir_text = text;\n",
        "                            render_frame(&state, &ir_text);\n",
        "                        }\n",
        "                        Err(e) => println!(\"error: invalid IR in {path}: {e}\"),\n",
        "                    },\n",
        "                    Err(e) => println!(\"error: cannot reload from {path}: {e}\"),\n",
        "                }\n",
        "            }\n",
        "            \"state\" => print_state(&state),\n",
        "            \"tap\" | \"doubleTap\" | \"longPress\" | \"drag\" | \"input\" => {\n",
        "                if parts.len() < 2 {\n",
        "                    println!(\"error: usage: {} <id> [text]\", parts[0]);\n",
        "                    continue;\n",
        "                }\n",
        "                let id = match parts[1].parse::<u32>() {\n",
        "                    Ok(id) => id,\n",
        "                    Err(_) => { println!(\"error: invalid node id {:?}\", parts[1]); continue; }\n",
        "                };\n",
        "                let text = parts[2..].join(\" \");\n",
        "                let input = if parts[0] == \"input\" { Some(text.as_str()) } else { None };\n",
        "                let patches = apply(&mut state, id, parts[0], input);\n",
        "                print_state(&state);\n",
        "                for p in patches { println!(\"patch {}={}\", p.name, p.value); }\n",
        "                render_frame(&state, &ir_text);\n",
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

/// The fill colour a `highContrast` node outlines against: its own `bg`, a
/// container-kind `color`, or the first gradient stop. `None` when it has no
/// fill, or when the `highContrast` prop is absent.
fn contrast_fill(n: &ENode) -> Option<String> {
    if !n
        .props
        .get("highContrast")
        .and_then(Json::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let bg = n.props.get("bg").and_then(Json::as_str).map(str::to_string);
    let container_color = matches!(
        n.kind.as_str(),
        "Container" | "Card" | "Dialog" | "List" | "Grid" | "Scroll"
    )
    .then(|| prop_str(&n.props, "color", "#f1f5f9"));
    let grad = n
        .props
        .get("gradient")
        .and_then(Json::as_str)
        .and_then(|s| s.split_once(','))
        .map(|(a, _)| a.trim().to_string());
    bg.or(container_color).or(grad)
}

/// Resolve an icon's SVG path, preferring the IR `icons` table (package icons
/// emitted by pkg_icons) over the built-in 31-name atlas. Returns the `d`
/// string of the first path, matching the built-in single-path convention.
fn ir_icon_path<'a>(icons: &'a [Json], name: &str) -> Option<&'a str> {
    let entry = icons
        .iter()
        .find(|e| e.get("name").and_then(Json::as_str) == Some(name))?;
    entry
        .get("paths")
        .and_then(Json::as_array)?
        .first()?
        .get("d")
        .and_then(Json::as_str)
}

fn draw_node(
    n: &ENode,
    boxes: &HashMap<u64, Box2>,
    out: &mut String,
    ir_path: &str,
    icons: &[Json],
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
            // `highContrast` recolours the glyphs to the contrast colour of the
            // text's background (its own bg/color, else the page surface),
            // matching the native renderer; absent the prop the colour is
            // unchanged.
            let color = if n
                .props
                .get("highContrast")
                .and_then(Json::as_bool)
                .unwrap_or(false)
            {
                let surface = n
                    .props
                    .get("bg")
                    .and_then(Json::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        n.props
                            .get("color")
                            .and_then(Json::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "#ffffff".to_string());
                js_lit(&Json::String(contrast_colour(&surface)))
            } else {
                js_lit(&Json::String(prop_str(&n.props, "color", "#111111")))
            };
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
            let resolved = ir_icon_path(icons, &name).or_else(|| icons::icon_svg_path(&name));
            match resolved {
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
    // `highContrast`: the 2px outline goes after the node's own fill so the fill
    // cannot paint over it. Absent the prop nothing is drawn and the output is
    // unchanged. A node with no fill gets no outline, matching the native
    // renderer.
    if let Some(fill) = contrast_fill(n) {
        out.push_str(&format!(
            "  ctx.strokeStyle = {};\n  ctx.lineWidth = 2;\n  ctx.strokeRect({x}, {y}, {w}, {h});\n",
            js_lit(&Json::String(contrast_colour(&fill)))
        ));
    }
    for child in &n.children {
        draw_node(child, boxes, out, ir_path, icons)?;
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
    if op == "push" {
        return Ok(format!(
            "state[\"@nav\"] = {};",
            js_lit(&Json::String(target.to_string()))
        ));
    }
    if op == "validate" {
        return Ok(format!(
            "state[\"@validate\"] = {};",
            js_lit(&Json::String(target.to_string()))
        ));
    }
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

    let icons = ir
        .get("icons")
        .and_then(Json::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let mut draw = String::new();
    draw_node(tree, &boxes, &mut draw, ir_path, icons)?;

    let mut tap_arms = String::new();
    let mut input_arms = String::new();
    let mut clicks = String::new();
    let mut gesture_arms = [String::new(), String::new(), String::new()];
    let mut gesture_hits = String::new();
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
        // Gesture nodes get one hit-test arm plus an action chain per event.
        let has_gesture = n
            .events
            .keys()
            .any(|k| matches!(k.as_str(), "doubleTap" | "longPress" | "drag"));
        if has_gesture {
            let b = boxes.get(&n.id).copied().unwrap_or(Box2 {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            });
            gesture_hits.push_str(&format!(
                "  if (x >= {} && x <= {} && y >= {} && y <= {}) return {};\n",
                fmt_num(b.x),
                fmt_num(b.x + b.w),
                fmt_num(b.y),
                fmt_num(b.y + b.h),
                n.id
            ));
        }
        for (i, event) in ["doubleTap", "longPress", "drag"].iter().enumerate() {
            if let Some(action) = n.events.get(*event) {
                let effect_code = web_effects(&event_effects(action, ir_path)?, ir_path)?;
                let code = js_action_chain(action, ir_path)?;
                gesture_arms[i].push_str(&format!(
                    "  if (id === {}) {{ {effect_code}{code}  {computed_call}render(); return; }}\n",
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
            let validation = validation_attrs(&n.props);
            input_html.push_str(&format!(
                "    <input id=\"n{}\" placeholder=\"{}\"{}>\n",
                n.id, placeholder, validation
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
    if !gesture_hits.is_empty() {
        for (i, name) in ["applyDoubleTap", "applyLongPress", "applyDrag"]
            .iter()
            .enumerate()
        {
            app_js.push_str(&format!("async function {name}(id) {{\n"));
            app_js.push_str(&gesture_arms[i]);
            app_js.push_str("}\n\n");
        }
        app_js.push_str("function gestureHit(x, y) {\n");
        app_js.push_str(&gesture_hits);
        app_js.push_str("  return -1;\n}\n\n");
        // The recognizer reads the event's own timeStamp (no wall clock) and
        // one pointer: a moved pointer is drag, a stationary hold of >=500 ms
        // is longPress, and two short taps within 300 ms are doubleTap.
        app_js.push_str(concat!(
            "let _g = null;\n",
            "let _lastTap = {};\n",
            "canvas.addEventListener(\"pointerdown\", function (e) {\n",
            "  const rect = canvas.getBoundingClientRect();\n",
            "  const id = gestureHit(e.clientX - rect.left, e.clientY - rect.top);\n",
            "  _g = id < 0 ? null : { id: id, x: e.clientX, y: e.clientY, t: e.timeStamp, moved: false };\n",
            "});\n",
            "canvas.addEventListener(\"pointermove\", function (e) {\n",
            "  if (!_g) return;\n",
            "  if (Math.abs(e.clientX - _g.x) > 10 || Math.abs(e.clientY - _g.y) > 10) _g.moved = true;\n",
            "});\n",
            "canvas.addEventListener(\"pointerup\", function (e) {\n",
            "  if (!_g) return;\n",
            "  const g = _g; _g = null;\n",
            "  const dt = e.timeStamp - g.t;\n",
            "  if (g.moved) applyDrag(g.id);\n",
            "  else if (dt >= 500) applyLongPress(g.id);\n",
            "  else if (_lastTap[g.id] !== undefined && g.t - _lastTap[g.id] <= 300) { _lastTap[g.id] = undefined; applyDoubleTap(g.id); }\n",
            "  else _lastTap[g.id] = g.t;\n",
            "});\n\n",
        ));
    }
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
    // All canvas font declarations use `sans-serif` as a placeholder; resolve
    // the platform-adaptive stack once and apply it to the whole script and the
    // host page, so text renders with the OS's installed typeface.
    let font_stack = theme_font_stack(ir);
    let app_js = app_js.replace("sans-serif", &font_stack);
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
    let html = html.replace(
        "font-family: sans-serif",
        &format!("font-family: {font_stack}"),
    );
    write(out, "index.html", &html, ir_path)
}

// ---------------------------------------------------- web multi-page ----

/// Per-route draw code plus the tap action chains, generated the same way as the
/// single-page emitter but one function per route. A `routeStack` array of route
/// names drives which function runs; push appends, pop removes.
fn web_route_draw(
    page: &EPage,
    ir_path: &str,
    icons: &[Json],
) -> DResult<(String, Vec<(u64, String)>, Vec<(u64, String)>)> {
    let mut boxes = HashMap::new();
    layout(&page.tree, 16.0, 16.0, &mut boxes);
    let mut nodes = Vec::new();
    collect(&page.tree, &mut nodes);

    let mut draw = String::new();
    draw_node(&page.tree, &boxes, &mut draw, ir_path, icons)?;

    let mut input_arms = Vec::new();
    let mut taps = Vec::new();
    for n in &nodes {
        if let Some(action) = n.events.get("tap") {
            let code = web_multipage_action_chain(action, ir_path)?;
            taps.push((n.id, code));
        }
        if let Some(action) = n.events.get("input") {
            let target = action_str(action, "target", ir_path)?;
            input_arms.push((n.id, target.to_string()));
        }
    }
    Ok((draw, taps, input_arms))
}

/// Emit the multi-page web bundle: one canvas, per-route draw functions, a
/// route stack with push/pop, and full-page redraw on navigation. ids stay
/// globally unique across routes because lowering numbers them app-wide.
fn emit_web_multipage(
    ir: &Json,
    app: &str,
    states: &[EState],
    computed: &[Computed],
    routes: &ERoutes,
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

    let mp_icons: &[Json] = ir
        .get("icons")
        .and_then(Json::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let mut draw_fns = String::new();
    let mut tap_arms = String::new();
    let mut all_taps: Vec<(String, Vec<(u64, String)>)> = Vec::new();
    let mut input_arms = String::new();
    let mut input_html = String::new();
    let mut input_listeners = String::new();
    let mut route_title_map = JsonMap::new();
    route_title_map.insert(routes.initial.clone(), json!(app));
    for page in &routes.pages {
        let (draw, taps, in_arms) = web_route_draw(page, ir_path, mp_icons)?;
        let fn_name = format!("draw_{}", page.name);
        draw_fns.push_str(&format!("function {fn_name}() {{\n  {draw}}}\n\n"));
        for (id, target) in in_arms {
            input_arms.push_str(&format!(
                "  if (id === {id}) {{ state.{target} = String(text); return; }}\n"
            ));
        }
        all_taps.push((page.name.clone(), taps));
        route_title_map.insert(
            page.name.clone(),
            json!(page.title.clone().unwrap_or_else(|| page.name.clone())),
        );
    }
    // Gather input widgets across every route; ids are app-wide unique.
    for page in &routes.pages {
        let mut nodes = Vec::new();
        collect(&page.tree, &mut nodes);
        for n in &nodes {
            if n.kind == "Input" {
                let placeholder = html_attr(&prop_str(&n.props, "placeholder", ""));
                let validation = validation_attrs(&n.props);
                input_html.push_str(&format!(
                    "    <input id=\"n{}\" placeholder=\"{}\"{}>\n",
                    n.id, placeholder, validation
                ));
                input_listeners.push_str(&format!(
                    "const el{} = document.getElementById(\"n{}\");\nif (el{}) {{ el{}.addEventListener(\"input\", function (e) {{ applyInput({}, e.target.value); }}); }}\n",
                    n.id, n.id, n.id, n.id, n.id
                ));
            }
        }
    }
    for (_route, taps) in &all_taps {
        for (id, code) in taps {
            tap_arms.push_str(&format!(
                "  if (id === {id}) {{ {code}  {computed_call}render(); return; }}\n",
            ));
        }
    }
    let draw_dispatch = routes
        .pages
        .iter()
        .map(|p| {
            format!(
                "  if (route === {}) {{ draw_{}(); return; }}",
                js_lit(&Json::String(p.name.clone())),
                p.name
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let initial_js = js_lit(&Json::String(routes.initial.clone()));
    let title_js = js_lit(&Json::Object(route_title_map));

    let mut app_js = String::new();
    app_js.push_str(&format!(
        "// Generated by opuromc for app {app}; multi-page, straight-line code, no IR interpreter.\n"
    ));
    app_js.push_str("\"use strict\";\n");
    app_js.push_str("const canvas = document.getElementById(\"app\");\n");
    app_js.push_str("const ctx = canvas.getContext(\"2d\");\n");
    app_js.push_str(&format!("const state = {state_js};\n"));
    app_js.push_str(&format!("const ROUTES = {title_js};\n"));
    app_js.push_str(&format!("const routeStack = [{initial_js}];\n\n"));
    app_js.push_str(&computed_fn);
    app_js.push_str(&draw_fns);
    app_js.push_str("function currentRoute() { return routeStack[routeStack.length - 1]; }\n");
    app_js.push_str("function render() {\n");
    app_js.push_str("  ctx.clearRect(0, 0, canvas.width, canvas.height);\n");
    app_js.push_str("  ctx.textBaseline = \"top\";\n");
    app_js.push_str("  const route = currentRoute();\n");
    app_js.push_str(&draw_dispatch);
    app_js.push_str("}\n\n");
    app_js.push_str("function push(route) { if (!ROUTES[route]) return; routeStack.push(route); document.title = ROUTES[route]; }\n");
    app_js.push_str("function pop() { if (routeStack.length > 1) routeStack.pop(); document.title = ROUTES[currentRoute()]; }\n\n");
    app_js.push_str("async function applyTap(id) {\n");
    app_js.push_str(&tap_arms);
    app_js.push_str("}\n\n");
    app_js.push_str("function applyInput(id, text) {\n");
    app_js.push_str(&input_arms);
    app_js.push_str("}\n\n");
    if !input_listeners.is_empty() {
        app_js.push_str(&input_listeners);
        app_js.push('\n');
    }
    app_js.push_str("canvas.addEventListener(\"click\", function (e) {\n");
    app_js.push_str("  const rect = canvas.getBoundingClientRect();\n");
    app_js.push_str("  const x = e.clientX - rect.left;\n");
    app_js.push_str("  const y = e.clientY - rect.top;\n");
    app_js.push_str("  const hits = tapHitTest(x, y);\n");
    app_js.push_str("  if (hits >= 0) applyTap(hits);\n");
    app_js.push_str("});\n\n");
    // A hit-test per route: the button boxes of the active route decide the tap.
    app_js.push_str("function tapHitTest(x, y) {\n  const route = currentRoute();\n");
    for page in &routes.pages {
        let mut boxes = HashMap::new();
        layout(&page.tree, 16.0, 16.0, &mut boxes);
        let mut nodes = Vec::new();
        collect(&page.tree, &mut nodes);
        let mut arms = String::new();
        for n in &nodes {
            if n.kind == "Button" && n.events.contains_key("tap") {
                let b = boxes.get(&n.id).copied().unwrap_or(Box2 {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                });
                arms.push_str(&format!(
                    "    if (x >= {} && x <= {} && y >= {} && y <= {}) return {};\n",
                    fmt_num(b.x),
                    fmt_num(b.x + b.w),
                    fmt_num(b.y),
                    fmt_num(b.y + b.h),
                    n.id
                ));
            }
        }
        app_js.push_str(&format!(
            "  if (route === {}) {{\n{arms}    return -1;\n  }}\n",
            js_lit(&Json::String(page.name.clone()))
        ));
    }
    app_js.push_str("  return -1;\n}\n\n");
    if computed.is_empty() {
        app_js.push_str("document.title = ROUTES[currentRoute()];\nrender();\n");
    } else {
        app_js.push_str("applyComputed();\ndocument.title = ROUTES[currentRoute()];\nrender();\n");
    }
    let font_stack = theme_font_stack(ir);
    let app_js = app_js.replace("sans-serif", &font_stack);
    write(out, "app.js", &app_js, ir_path)?;

    let title = html_attr(app);
    let sr_css = ".sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}";
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>body {{ font-family: sans-serif; margin: 0; padding: 16px; }} canvas {{ background: #fff; border: 1px solid #ddd; }} #inputs {{ margin-top: 12px; }}{sr_css}</style>\n</head>\n<body>\n<canvas id=\"app\" width=\"800\" height=\"600\"></canvas>\n<div id=\"inputs\">\n{input_html}</div>\n<script src=\"app.js\"></script>\n</body>\n</html>\n"
    );
    let html = html.replace(
        "font-family: sans-serif",
        &format!("font-family: {font_stack}"),
    );
    write(out, "index.html", &html, ir_path)
}

/// A route-aware JS action chain: `push`/`pop` map onto the route stack, other
/// ops mutate state exactly as the single-page `js_action` does.
fn web_multipage_action_chain(event: &Json, ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for action in event_actions(event) {
        let op = action_str(action, "op", ir_path)?;
        match op {
            "push" => {
                let target = action_str(action, "target", ir_path)?;
                code.push_str(&format!(
                    "  push({});\n",
                    js_lit(&Json::String(target.to_string()))
                ));
            }
            "pop" => code.push_str("  pop();\n"),
            _ => {
                code.push_str("  ");
                code.push_str(&js_action(action, ir_path)?);
                code.push('\n');
            }
        }
    }
    Ok(code)
}

// ----------------------------------------------------------- mini programs ----

/// Pure white on a dark background, pure black on a light one, from the integer
/// luminance `(r*299 + g*587 + b*114) / 1000`. Mirrors the native renderer's
/// `contrast_text` so every target picks the same high-contrast colour.
fn contrast_colour(hex: &str) -> String {
    let b = hex.trim_start_matches('#').as_bytes();
    let (r, g, bl) = if b.len() >= 6 {
        (
            u32::from_str_radix(&hex.trim_start_matches('#')[0..2], 16).unwrap_or(0),
            u32::from_str_radix(&hex.trim_start_matches('#')[2..4], 16).unwrap_or(0),
            u32::from_str_radix(&hex.trim_start_matches('#')[4..6], 16).unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    };
    if (r * 299 + g * 587 + bl * 114) / 1000 < 128 {
        "#ffffff".to_string()
    } else {
        "#000000".to_string()
    }
}

/// Whether a kind draws a foreground text run, so `highContrast` recolours its
/// glyphs. Mirrors the renderer's text-colour rule for Text and the primitive
/// kinds whose mini markup renders a `<text>` label.
/// Mirrors the web-dom emitter's elevated-glass set; these surfaces use the
/// desktop blur while every other glass node uses the compact blur.
fn is_elevated_glass_kind(kind: &str) -> bool {
    matches!(kind, "Dialog" | "Menu" | "Drawer" | "Dock" | "Modal")
}

fn is_text_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Text"
            | "Badge"
            | "Avatar"
            | "Progress"
            | "Switch"
            | "Slider"
            | "Checkbox"
            | "Radio"
            | "Tabs"
            | "Icon"
    )
}

fn style_of(
    props: &JsonMap<String, Json>,
    background: bool,
    justified: bool,
    dark: bool,
    elevated: bool,
) -> String {
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
    let glass = props.get("glass").and_then(Json::as_bool).unwrap_or(false);
    if let Some((a, b)) = grad {
        parts.push(format!(
            "background:linear-gradient(to bottom, {}, {})",
            a.trim(),
            b.trim()
        ));
    } else if let Some(c) = bg {
        parts.push(format!("background-color:{c}"));
    } else if glass {
        // the default UI rules framework rules is the mandatory override:
        // dark swaps only the fill tint, every other channel is mode-independent.
        let frost = if dark {
            "rgba(30, 35, 46, 0.38)"
        } else {
            "rgba(255, 255, 255, 0.42)"
        };
        parts.push(format!("background:{frost}"));
    }
    let shadow = props.get("shadow").and_then(Json::as_i64).unwrap_or(0);
    if shadow > 0 {
        parts.push(format!(
            "box-shadow:0 {shadow}px {shadow}px rgba(0, 0, 0, 0.15)"
        ));
    } else if glass {
        // Inline the v2 tier2 channel: WXSS/ACSS cannot resolve the DOM
        // emitter's custom properties, so the composed value is emitted here.
        parts.push(
            "box-shadow:0 2px 8px rgba(17, 24, 39, 0.06), inset 0 1px 0 rgba(255, 255, 255, 0.75), inset 0 -1px 0 rgba(0, 0, 0, 0.06)"
                .to_string(),
        );
    }
    if glass {
        // Elevated surfaces use the desktop blur, all others the compact blur.
        let blur = if elevated {
            "blur(20px) saturate(160%)"
        } else {
            "blur(12px) saturate(140%)"
        };
        parts.push(format!("backdrop-filter:{blur}"));
        let glass_border = if dark {
            "rgba(255, 255, 255, 0.12)"
        } else {
            "rgba(255, 255, 255, 0.5)"
        };
        parts.push(format!("border:1px solid {glass_border}"));
    }
    if let Some(Json::String(c)) = props.get("border") {
        parts.push(format!("border:1px solid {c}"));
    }
    // `highContrast` mirrors the native renderer: a 2px contrast outline on any
    // node with a fill, and a contrast text colour on a justified (text-bearing)
    // node. Text colour is applied here so it overrides an earlier `color`.
    if props
        .get("highContrast")
        .and_then(Json::as_bool)
        .unwrap_or(false)
    {
        let fill = bg
            .map(str::to_string)
            .or_else(|| (background).then(|| prop_str(props, "color", "")))
            .or_else(|| grad.map(|(a, _)| a.trim().to_string()))
            .filter(|s| !s.is_empty());
        if let Some(fill) = &fill {
            parts.push(format!("border:2px solid {}", contrast_colour(fill)));
        }
        if justified {
            let surface = fill.as_deref().unwrap_or("#ffffff");
            parts.push(format!("color:{}", contrast_colour(surface)));
        }
    }
    parts.join(";")
}

/// The entry-animation CSS class for a node's `animate` prop, mirroring the
/// native renderer and the web-dom emitter: spring/ease/inertia/collision
/// animate in from the centre, `none`/absent is static. The curve is mapped to
/// a CSS timing function (spring and collision are documented cubic-bezier
/// approximations).
fn anim_class(props: &JsonMap<String, Json>) -> String {
    let Some(Json::String(curve)) = props.get("animate") else {
        return String::new();
    };
    match curve.as_str() {
        "spring" => " anim-entry anim-spring".to_string(),
        "ease" => " anim-entry anim-ease".to_string(),
        "inertia" => " anim-entry anim-inertia".to_string(),
        "collision" => " anim-entry anim-collision".to_string(),
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
    dark: bool,
    icons: &[Json],
) -> DResult<()> {
    let pad = "  ".repeat(depth);
    let id = n.id;
    let style = style_of(
        &n.props,
        n.kind == "Container",
        is_text_kind(&n.kind),
        dark,
        is_elevated_glass_kind(&n.kind),
    );
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
    let mut a11y_attr = match n.props.get("role").and_then(Json::as_str) {
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
    // A node bound to doubleTap/longPress/drag carries the platform's touch
    // trio plus its node id; the generated recognizer dispatches by the id.
    if n.events
        .keys()
        .any(|k| matches!(k.as_str(), "doubleTap" | "longPress" | "drag"))
    {
        a11y_attr.push_str(&format!(
            " {}=\"onGestureStart\" {}=\"onGestureMove\" {}=\"onGestureEnd\" data-gid=\"{id}\"",
            p.touch_start_attr, p.touch_move_attr, p.touch_end_attr
        ));
    }
    if let Some(meta) = &n.repeat {
        a11y_attr.push_str(&format!(
            " data-repeat=\"{}\"",
            html_attr(&meta.to_string())
        ));
    }
    a11y_attr.push_str(&design_attrs(&n.props));
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
            let validation = validation_attrs(&n.props);
            out.push_str(&format!(
                "{pad}<input class=\"n{id}{anim}\"{style_attr}{cond_attr}{a11y_attr} placeholder=\"{placeholder}\"{value}{input}{validation} />\n"
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
            let resolved = ir_icon_path(icons, &name).or_else(|| icons::icon_svg_path(&name));
            match resolved {
                // A real icon element: an inline SVG data URI on an <image>, so
                // no platform runtime is needed and every target draws the same
                // canonical path the native renderer uses.
                Some(path) => {
                    let svg = format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"{path}\"/></svg>"
                    );
                    let b64 = base64_encode(&svg);
                    let mut style = style_of(
                        &n.props,
                        false,
                        is_text_kind(&n.kind),
                        dark,
                        is_elevated_glass_kind(&n.kind),
                    );
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
        k if EXTENDED_PRIMITIVE_KINDS.contains(&k) => {
            // Structured extended-primitive markup: a kind-specific class plus
            // every prop as a data attribute, so mini-program CSS and scripts
            // can style or drive each primitive without losing its numbers.
            let slug = k.to_lowercase();
            let mut data = format!(" data-kind=\"{}\"", html_attr(k));
            for (key, value) in &n.props {
                // Design props ride on `a11y_attr`, so skip them here to avoid
                // emitting the same data attribute twice.
                if is_design_prop(key) {
                    continue;
                }
                let rendered = match value {
                    Json::String(s) => markup_interp(s, p, ir_path)?,
                    other => other.to_string(),
                };
                let attr = key.replace('_', "-");
                data.push_str(&format!(" data-{}=\"{}\"", attr, html_attr(&rendered)));
            }
            if n.children.is_empty() {
                let label = markup_text(&fallback_text(n));
                let inner = if label.is_empty() {
                    String::new()
                } else {
                    format!("<text>{label}</text>")
                };
                out.push_str(&format!(
                    "{pad}<view class=\"opu-x opu-x-{slug} n{id}{anim}\"{data}{style_attr}{cond_attr}{a11y_attr}>{inner}</view>\n"
                ));
            } else {
                out.push_str(&format!(
                    "{pad}<view class=\"opu-x opu-x-{slug} n{id}{anim}\"{data}{style_attr}{cond_attr}{a11y_attr}>\n"
                ));
                for child in &n.children {
                    markup_node(child, p, depth + 1, out, ir_path, dark, icons)?;
                }
                out.push_str(&format!("{pad}</view>\n"));
            }
        }
        "For" => {
            // Runtime list loop (DYNAMIC_LISTS.md Phase B): the platform's loop
            // directive renders the template once per element of `source`.
            // cond_attr "wx:if" becomes "wx:for"; the item var rides "<ns>-item".
            let source = prop_str(&n.props, "source", "");
            let item = prop_str(&n.props, "item", "");
            // cond_attr is "<ns>:if" or "<ns>-if"; keep just the namespace.
            let ns = p
                .cond_attr
                .trim_end_matches("if")
                .trim_end_matches([':', '-']);
            let loop_attr = format!(
                " {ns}:for=\"{}{}{}\" {ns}:for-item=\"{}\" {ns}:key=\"index\"",
                p.open,
                html_attr(&source),
                p.close,
                html_attr(&item)
            );
            out.push_str(&format!(
                "{pad}<view class=\"col n{id}{anim}\"{loop_attr}{cond_attr}>
"
            ));
            for child in &n.children {
                markup_node(child, p, depth + 1, out, ir_path, dark, icons)?;
            }
            out.push_str(&format!(
                "{pad}</view>
"
            ));
        }
        kind => {
            // Documented fallback: an unknown kind renders as a Container
            // showing its label, so gallery exhibits stay visible in pages.
            // Grid is a wrapping row, not a column: mini-program WXSS has no
            // CSS grid, so `compactColumns`/`columns` maps to percentage-width
            // cells emitted in `emit_mini`'s stylesheet.
            let cls = if kind == "Row" {
                "row"
            } else if kind == "Grid" {
                "grid"
            } else {
                "col"
            };
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
                markup_node(child, p, depth + 1, out, ir_path, dark, icons)?;
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
    if op == "push" {
        return Ok(format!(
            "this.{set}({{\"@nav\": {}}});",
            js_lit(&Json::String(target.to_string()))
        ));
    }
    if op == "validate" {
        return Ok(format!(
            "this.{set}({{\"@validate\": {}}});",
            js_lit(&Json::String(target.to_string()))
        ));
    }
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
        "set" | "assign" => match action.get("valueFrom").and_then(Json::as_str) {
            Some(from) => format!("this.{set}({{\"{target}\": this.data.{from}}});"),
            None => {
                let v = action
                    .get("value")
                    .cloned()
                    .unwrap_or(Json::String(String::new()));
                format!("this.{set}({{\"{target}\": {}}});", js_lit(&v))
            }
        },
        // List mutations read the current array, edit a copy, and write it back
        // so the platform diff sees a new reference (DYNAMIC_LISTS Phase B).
        "append" => {
            let elem = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("this.data.{from}"),
                None => {
                    let v = action.get("value").cloned().unwrap_or(Json::Null);
                    js_lit(&v)
                }
            };
            format!("this.{set}({{\"{target}\": this.data.{target}.concat([{elem}])}});")
        }
        "removeAt" => {
            let idx = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => format!("this.data.{from}"),
                None => action
                    .get("value")
                    .and_then(Json::as_i64)
                    .unwrap_or(0)
                    .to_string(),
            };
            format!("this.{set}({{\"{target}\": this.data.{target}.slice(0, {idx}).concat(this.data.{target}.slice({idx} + 1))}});")
        }
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
    mini_effects_with(effects, p, ir_path, false)
}

/// `keep_old_on_fail` leaves state untouched when a fetch fails (a route's
/// pull-refresh keeps previous values, PLATFORM_DELIVERY.md §7); an async
/// handler keeps the historical `"error: request failed"` writeback.
fn mini_effects_with(
    effects: &[Effect],
    p: &Platform,
    ir_path: &str,
    keep_old_on_fail: bool,
) -> DResult<String> {
    mini_effects_for(effects, p, ir_path, keep_old_on_fail, &[])
}

/// `states` lets a fetch into a list state behave as `assign` (section 7):
/// the response must parse to an array of strings or the old value stays.
fn mini_effects_for(
    effects: &[Effect],
    p: &Platform,
    ir_path: &str,
    keep_old_on_fail: bool,
    states: &[EState],
) -> DResult<String> {
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
                let is_list = states
                    .iter()
                    .any(|st| st.name == *into && st.ty == "list");
                let path = js_lit(&Json::String(path.clone()));
                let target = js_lit(&Json::String(into.clone()));
                code.push_str("await new Promise(function (resolve) { ");
                code.push_str(&format!(
                    "{}({{url: {path}, method: \"GET\", success: function (res) {{ ",
                    p.request_api
                ));
                if is_list {
                    // A list `into` behaves as assign: only an array of strings
                    // writes; anything else keeps the old value (§7, D3).
                    code.push_str(&format!(
                        "let arr = res.data; if (typeof arr === \"string\") {{ try {{ arr = JSON.parse(arr); }} catch (e) {{ arr = null; }} }} if (Array.isArray(arr) && arr.every(function (x) {{ return typeof x === \"string\"; }})) {{ page.{}({{{target}: arr}}); }} resolve(); ",
                        p.state_set
                    ));
                } else {
                    code.push_str(&format!(
                        "page.{}({{{target}: typeof res.data === \"string\" ? res.data : JSON.stringify(res.data)}}); resolve(); ",
                        p.state_set
                    ));
                }
                code.push_str("}, fail: function () { ");
                if keep_old_on_fail || is_list {
                    // Keep the previous value on failure (§7 pull-refresh, D3).
                    code.push_str("resolve(); ");
                } else {
                    code.push_str(&format!(
                        "page.{}({{{target}: \"error: request failed\"}}); resolve(); ",
                        p.state_set
                    ));
                }
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

/// The theme subset a generated app must carry when it cannot embed the whole
/// IR (mini-program targets). Only the keys that no stylesheet reproduces are
/// surfaced: `fontMetrics` and the semantic tokens. Returns `None` when the
/// theme has none of them, so an authored IR without these keys keeps its
/// byte-identical mini-program bundle.
fn carried_theme_js(ir: &Json) -> Option<String> {
    let theme = ir.get("theme")?;
    let mut out = JsonMap::new();
    if let Some(fm) = theme.get("fontMetrics").and_then(Json::as_str) {
        out.insert("fontMetrics".to_string(), json!(fm));
    }
    if let Some(tokens) = theme.get("tokens").and_then(Json::as_object) {
        let mut semantic = JsonMap::new();
        for (key, value) in tokens {
            if !matches!(key.as_str(), "surface" | "onSurface" | "stage") {
                semantic.insert(key.clone(), value.clone());
            }
        }
        if !semantic.is_empty() {
            out.insert("tokens".to_string(), Json::Object(semantic));
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(serde_json::to_string(&Json::Object(out)).unwrap_or_else(|_| "{}".to_string()))
}

fn emit_mini(
    target: &str,
    app: &str,
    states: &[EState],
    computed: &[Computed],
    tree: &ENode,
    ir: &Json,
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
    let theme_js = carried_theme_js(ir);
    let app_js = match theme_js {
        Some(theme) => format!(
            "// Generated by opuromc for {}; no IR interpreter.\nApp({{}});\nconst Opurom_THEME = {theme};\n",
            p.key
        ),
        None => format!(
            "// Generated by opuromc for {}; no IR interpreter.\nApp({{}});\n",
            p.key
        ),
    };
    write(out, "app.js", &app_js, ir_path)?;

    let mut markup = String::new();
    markup.push_str(&format!(
        "<!-- Generated by opuromc for {}; no IR interpreter. -->\n",
        p.key
    ));
    let dark = ir
        .get("theme")
        .and_then(|t| t.get("mode"))
        .and_then(Json::as_str)
        == Some("dark");
    let icons = ir
        .get("icons")
        .and_then(Json::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    markup_node(tree, p, 0, &mut markup, ir_path, dark, icons)?;
    write(
        out,
        &format!("pages/index/index.{}", p.markup_ext),
        &markup,
        ir_path,
    )?;

    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    // The stage is the flat page backdrop a glass lamina refracts. Mini-program
    // WXSS has no body element, so `page` is the platform-native canvas; a
    // missing token falls back to `surface` and then to white. Only a strict
    // hex colour is accepted so an authored IR cannot inject CSS.
    let tokens = ir.get("theme").and_then(|t| t.get("tokens"));
    let stage = tokens
        .and_then(|t| t.get("stage"))
        .or_else(|| tokens.and_then(|t| t.get("surface")))
        .and_then(Json::as_str)
        .filter(|s| {
            let digits = s.strip_prefix('#').unwrap_or("");
            (digits.len() == 6 || digits.len() == 8)
                && digits.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .unwrap_or("#ffffff");
    let mut style = String::from(&format!(
        "page {{ background-color: {stage}; }}\n.col {{ display: flex; flex-direction: column; }}\n.row {{ display: flex; flex-direction: row; }}\n.grid {{ display: flex; flex-direction: row; flex-wrap: wrap; }}\n@keyframes opurom-entry {{ from {{ transform: scale(0); opacity: 0; }} to {{ transform: scale(1); opacity: 1; }} }}\n.anim-entry {{ transform-origin: center; animation: opurom-entry 480ms ease-out both; }}\n.anim-spring {{ animation-timing-function: cubic-bezier(0.34, 1.56, 0.64, 1); }}\n.anim-ease {{ animation-timing-function: ease-out; }}\n.anim-inertia {{ animation-timing-function: ease-in-out; }}\n.anim-collision {{ animation-timing-function: cubic-bezier(0.5, 1.6, 0.4, 1); }}\n",
    ));
    for n in &nodes {
        let s = style_of(
            &n.props,
            n.kind == "Container",
            is_text_kind(&n.kind),
            dark,
            is_elevated_glass_kind(&n.kind),
        );
        if !s.is_empty() {
            style.push_str(&format!(".n{} {{ {}; }}\n", n.id, s));
        }
        if n.kind == "Grid" {
            // Mini-program pages are phone-width, so a declared compact count
            // wins; otherwise the authored column count applies. Cells without
            // an explicit width share the row equally. WXSS has no CSS grid, so
            // percentage widths on a wrapping flex row are the native form.
            let cols = n
                .props
                .get("compactColumns")
                .or_else(|| n.props.get("columns"))
                .and_then(Json::as_i64)
                .unwrap_or(2)
                .clamp(1, 16);
            let pct = 100.0 / cols as f64;
            for child in &n.children {
                if child.props.contains_key("width") {
                    continue;
                }
                style.push_str(&format!(
                    ".n{} {{ width: {pct:.4}%; box-sizing: border-box; }}\n",
                    child.id
                ));
            }
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
        for (event, method) in [
            ("doubleTap", "onDoubleTap"),
            ("longPress", "onLongPress"),
            ("drag", "onDrag"),
        ] {
            if let Some(action) = n.events.get(event) {
                let effect_code = mini_effects(&event_effects(action, ir_path)?, p, ir_path)?;
                let code = mini_action_chain(action, p, ir_path)?;
                methods.push_str(&format!(
                    "  {method}{}: async function () {{ {effect_code}{code}{computed_call} }},\n",
                    n.id
                ));
            }
        }
    }
    // The recognizer is emitted only when at least one node binds a gesture.
    // It uses the event's own timeStamp (no wall clock) and one touch point,
    // so a tap that never moved becomes doubleTap/longPress, while a moved
    // touch becomes drag. Native tap keeps its own bindtap handler.
    let has_gestures = nodes.iter().any(|n| {
        n.events
            .keys()
            .any(|k| matches!(k.as_str(), "doubleTap" | "longPress" | "drag"))
    });
    if has_gestures {
        methods.push_str(
            "  onGestureStart: function (e) {\n\
             \x20   const g = this._gesture || (this._gesture = {});\n\
             \x20   const id = e.currentTarget.dataset.gid;\n\
             \x20   const t = (e.touches && e.touches[0]) || (e.changedTouches && e.changedTouches[0]) || {};\n\
             \x20   const prev = g[id];\n\
             \x20   g[id] = { x: t.clientX || 0, y: t.clientY || 0, t: e.timeStamp || 0, moved: false,\n\
             \x20              lastTap: prev ? prev.lastTap : undefined };\n\
             \x20 },\n\
             \x20 onGestureMove: function (e) {\n\
             \x20   const g = this._gesture || {}; const s = g[e.currentTarget.dataset.gid];\n\
             \x20   if (!s) return;\n\
             \x20   const t = (e.touches && e.touches[0]) || {};\n\
             \x20   if (Math.abs((t.clientX || 0) - s.x) > 10 || Math.abs((t.clientY || 0) - s.y) > 10) s.moved = true;\n\
             \x20 },\n\
             \x20 onGestureEnd: function (e) {\n\
             \x20   const g = this._gesture || {}; const id = e.currentTarget.dataset.gid;\n\
             \x20   const s = g[id]; if (!s) return;\n\
             \x20   const now = e.timeStamp || 0; const dt = now - s.t;\n\
             \x20   let name = null;\n\
             \x20   if (s.moved) name = 'onDrag' + id;\n\
             \x20   else if (dt >= 500) name = 'onLongPress' + id;\n\
             \x20   else if (s.lastTap !== undefined && now - s.lastTap <= 300) { name = 'onDoubleTap' + id; s.lastTap = undefined; }\n\
             \x20   else s.lastTap = now;\n\
             \x20   const handler = name && this[name];\n\
             \x20   if (handler) handler.call(this);\n\
             \x20 },\n",
        );
    }
    let mut prefix = format!(
        "// Generated by opuromc for {}; straight-line code, no IR interpreter.\n",
        p.key
    );
    if let Some(meta) = additive_meta(tree) {
        prefix.push_str("// Additive metadata: consumers repeat templates and validate inputs; this emitter renders the template once.\n");
        prefix.push_str(&format!("const Opurom_META = {meta};\n"));
    }
    let index_js = format!("{prefix}Page({{\n  data: {},\n{}}});\n", data_js, methods);
    write(out, "pages/index/index.js", &index_js, ir_path)
}

// --------------------------------------------------- mini multi-page ----

/// Mini-program route-aware action chain. `push`/`pop` update the page's route
/// stack through setData; every other op reuses the single-page action emitter.
fn mini_multipage_action_chain(event: &Json, p: &Platform, ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for action in event_actions(event) {
        let op = action_str(action, "op", ir_path)?;
        match op {
            "push" => {
                let target = action_str(action, "target", ir_path)?;
                code.push_str(&format!(
                    "{}({{url: \"pages/{}/index\"}}); ",
                    p.navigate_api,
                    target.to_lowercase()
                ));
            }
            "pop" => code.push_str(&format!("{}({{delta: 1}}); ", p.navigate_back_api)),
            _ => {
                code.push_str(&mini_action(action, p, ir_path)?);
                code.push(' ');
            }
        }
    }
    Ok(code)
}
/// Emit a true multi-page mini-program bundle (PLATFORM_DELIVERY.md §7). Each
/// route gets its own page quad `pages/<route>/index.{markup,style,js}`; the
/// launch page `pages/index` is a thin router that redirects to `initialRoute`.
/// `push`/`pop` lower to the platform navigation API, `routeStack`/`currentRoute`
/// disappear, and IR state lives in `App.globalData.states` with every page's
/// `onShow` mirroring it into `setData`. Single-route programs keep `emit_mini`.
fn emit_mini_multipage(
    target: &str,
    app: &str,
    states: &[EState],
    computed: &[Computed],
    routes: &ERoutes,
    ir: &Json,
    out: &Path,
    ir_path: &str,
) -> DResult<()> {
    let p = PLATFORMS
        .iter()
        .find(|p| p.key == target)
        .ok_or_else(|| bad_ir(ir_path, format!("no platform table entry for '{target}'")))?;

    // Page paths are lowercase route names (A1); two routes that lowercase to
    // the same path would collide on disk, so reject them up front.
    let page_path = |route: &str| format!("pages/{}/index", route.to_lowercase());
    let mut lower_seen: Vec<String> = Vec::new();
    for page in &routes.pages {
        let lower = page.name.to_lowercase();
        if lower_seen.contains(&lower) {
            return Err(bad_ir(
                ir_path,
                format!("routes '{}' and '{}' lower to the same page path", page.name, lower),
            ));
        }
        lower_seen.push(lower);
    }
    let mp_icons: &[Json] = ir
        .get("icons")
        .and_then(Json::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    // app.json: one entry per route (A2), the initial route first so it is the
    // launch page (A1). Routes with a subpackage go under subPackages (§7); the
    // initialRoute must not be subpackaged.
    let mut pages: Vec<String> = Vec::new();
    // page_path for a route that may live in a subpackage.
    let route_page_path = |page: &EPage| -> String {
        match &page.subpackage {
            Some(pack) => format!("packages/{pack}/pages/{}/index", page.name.to_lowercase()),
            None => page_path(&page.name),
        }
    };
    let mut sub_order: Vec<String> = Vec::new();
    let mut sub_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let initial = routes
        .pages
        .iter()
        .find(|p| p.name == routes.initial)
        .ok_or_else(|| bad_ir(ir_path, format!("initialRoute '{}' is not a declared route", routes.initial)))?;
    if initial.subpackage.is_some() {
        return Err(bad_ir(
            ir_path,
            format!("initialRoute '{}' must not declare a subpackage", routes.initial),
        ));
    }
    pages.push(route_page_path(initial));
    for page in &routes.pages {
        if page.name == routes.initial {
            continue;
        }
        if let Some(pack) = &page.subpackage {
            if !sub_map.contains_key(pack) {
                sub_order.push(pack.clone());
            }
            sub_map
                .entry(pack.clone())
                .or_default()
                .push(route_page_path(page));
        } else {
            pages.push(route_page_path(page));
        }
    }
    let mut window = JsonMap::new();
    window.insert(p.title_key.to_string(), json!(app));
    let mut app_obj = JsonMap::new();
    app_obj.insert("pages".to_string(), json!(pages));
    app_obj.insert("window".to_string(), Json::Object(window));
    if !sub_order.is_empty() {
        let subs: Vec<Json> = sub_order
            .iter()
            .map(|pack| {
                json!({
                    "root": format!("packages/{pack}"),
                    "pages": sub_map[pack].iter().map(|p| {
                        // app.json subpackage `pages` are relative to `root`.
                        p.trim_start_matches(&format!("packages/{pack}/")).to_string()
                    }).collect::<Vec<_>>()
                })
            })
            .collect();
        app_obj.insert("subPackages".to_string(), Json::Array(subs));
    }

    // app.js: global state lives in App.globalData.states, every page writes
    // back through it and mirrors it into setData on onShow.
    let theme_js = carried_theme_js(ir);
    let mut state_seed = JsonMap::new();
    for st in states {
        state_seed.insert(st.name.clone(), st.init.clone());
    }
    let states_js = js_lit(&Json::Object(state_seed));
    let mut app_js = format!(
        "// Generated by opuromc for {}; multi-page, no IR interpreter.\nApp({{\n  globalData: {{ states: {states_js} }}\n}});\n",
        p.key
    );
    if let Some(theme) = theme_js {
        app_js.push_str(&format!("const Opurom_THEME = {theme};\n"));
    }

    let dark = ir
        .get("theme")
        .and_then(|t| t.get("mode"))
        .and_then(Json::as_str)
        == Some("dark");
    let tokens = ir.get("theme").and_then(|t| t.get("tokens"));
    let stage = tokens
        .and_then(|t| t.get("stage"))
        .or_else(|| tokens.and_then(|t| t.get("surface")))
        .and_then(Json::as_str)
        .filter(|s| {
            let digits = s.strip_prefix('#').unwrap_or("");
            (digits.len() == 6 || digits.len() == 8)
                && digits.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .unwrap_or("#ffffff");
    let base_style = format!(
        "page {{ background-color: {stage}; }}\n.col {{ display: flex; flex-direction: column; }}\n.row {{ display: flex; flex-direction: row; }}\n.grid {{ display: flex; flex-direction: row; flex-wrap: wrap; }}\n@keyframes opurom-entry {{ from {{ transform: scale(0); opacity: 0; }} to {{ transform: scale(1); opacity: 1; }} }}\n.anim-entry {{ transform-origin: center; animation: opurom-entry 480ms ease-out both; }}\n.anim-spring {{ animation-timing-function: cubic-bezier(0.34, 1.56, 0.64, 1); }}\n.anim-ease {{ animation-timing-function: ease-out; }}\n.anim-inertia {{ animation-timing-function: ease-in-out; }}\n.anim-collision {{ animation-timing-function: cubic-bezier(0.5, 1.6, 0.4, 1); }}\n"
    );

    for page in &routes.pages {
        // Markup: only this route's subtree, rooted in one <view class="route">.
        let mut markup = format!(
            "<!-- Generated by opuromc for {}; route '{}', no IR interpreter. -->\n",
            p.key, page.name
        );
        markup.push_str("<view class=\"route\">\n");
        markup_node(&page.tree, p, 1, &mut markup, ir_path, dark, mp_icons)?;
        markup.push_str("</view>\n");

        // Style: base rules plus this route's node classes.
        let mut style = base_style.clone();
        let mut nodes: Vec<&ENode> = Vec::new();
        collect(&page.tree, &mut nodes);
        for n in &nodes {
            let s = style_of(
                &n.props,
                n.kind == "Container",
                is_text_kind(&n.kind),
                dark,
                is_elevated_glass_kind(&n.kind),
            );
            if !s.is_empty() {
                style.push_str(&format!(".n{} {{ {}; }}\n", n.id, s));
            }
            if n.kind == "Grid" {
                let cols = n
                    .props
                    .get("compactColumns")
                    .or_else(|| n.props.get("columns"))
                    .and_then(Json::as_i64)
                    .unwrap_or(2)
                    .clamp(1, 16);
                let pct = 100.0 / cols as f64;
                for child in &n.children {
                    if child.props.contains_key("width") {
                        continue;
                    }
                    style.push_str(&format!(
                        ".n{} {{ width: {pct:.4}%; box-sizing: border-box; }}\n",
                        child.id
                    ));
                }
            }
        }

        // Page JS: data seeded from global state, onShow mirrors globalData,
        // tap handlers write back to globalData and setData.
        let mut data = JsonMap::new();
        for st in states {
            data.insert(st.name.clone(), st.init.clone());
        }
        let data_js = js_lit(&Json::Object(data));
        let computed_call = if computed.is_empty() {
            ""
        } else {
            " this.applyComputed();"
        };
        let mut methods = String::new();
        // The truth lives in App.globalData.states; this page mirrors it.
        methods.push_str("  onShow: function () {\n    const g = getApp().globalData.states;\n    if (g) { this.setData(g); }\n  },\n");
        // Every write-back updates globalData then setData so a pushed page reads it.
        methods.push_str("  applyState: function (patch) {\n    const g = getApp().globalData.states = getApp().globalData.states || {};\n    Object.assign(g, patch);\n    this.setData(patch);\n  },\n");
        // Pull-refresh (§7): run the page's effects on pull-down, then stop the
        // animation; a failed fetch keeps the old value (mini_effects resolves
        // either way, so failure leaves state as it was).
        if let Some(effects) = &page.pull_refresh {
            let effect_code = mini_effects_for(effects, p, ir_path, true, states)?
                .replace("page.setData(", "page.applyState(")
                .replace("this.setData(", "this.applyState(");
            methods.push_str(&format!(
                "  onPullDownRefresh: async function () {{ {effect_code}{}(); }},\n",
                p.stop_refresh_api
            ));
        }
        let computed_methods = mini_computed(p, states, computed, ir_path)?
            .replace("this.setData(", "this.applyState(");
        methods.push_str(&computed_methods);
        for n in &nodes {
            if let Some(action) = n.events.get("tap") {
                let effect_code = mini_effects_for(&event_effects(action, ir_path)?, p, ir_path, false, states)?
                    .replace("page.setData(", "page.applyState(")
                    .replace("this.setData(", "this.applyState(");
                let code = mini_multipage_action_chain(action, p, ir_path)?
                    .replace("this.setData(", "this.applyState(");
                methods.push_str(&format!(
                    "  onTap{}: async function () {{ {effect_code}{code}{computed_call} }},\n",
                    n.id
                ));
            }
            if let Some(action) = n.events.get("input") {
                let target_name = action_str(action, "target", ir_path)?;
                methods.push_str(&format!(
                    "  onInput{}: function (e) {{ this.{}({{\"{}\": e.detail.value}});{} }},\n",
                    n.id, "applyState", target_name, computed_call
                ));
            }
        }
        let prefix = format!(
            "// Generated by opuromc for {}; route '{}', no IR interpreter.\n",
            p.key, page.name
        );
        let page_js = format!("{prefix}Page({{\n  data: {},\n{}}});\n", data_js, methods);

        let base = route_page_path(page);
        write(out, &format!("{base}.{}", p.markup_ext), &markup, ir_path)?;
        write(out, &format!("{base}.{}", p.style_ext), &style, ir_path)?;
        write(out, &format!("{base}.js"), &page_js, ir_path)?;
        // Page-level config (A3): the platform title key; a page declaring
        // pull-refresh enables pull-down (D1), every other page omits the key.
        let mut page_cfg = JsonMap::new();
        page_cfg.insert(
            p.title_key.to_string(),
            json!(page.title.clone().unwrap_or_else(|| page.name.clone())),
        );
        if page.pull_refresh.is_some() {
            page_cfg.insert("enablePullDownRefresh".to_string(), json!(true));
        }
        let page_cfg_json = serde_json::to_string_pretty(&Json::Object(page_cfg))
            .map_err(|e| bad_ir(ir_path, format!("cannot serialize page config: {e}")))?;
        write(out, &format!("{base}.json"), &(page_cfg_json + "\n"), ir_path)?;
    }

    write(
        out,
        "app.json",
        &(serde_json::to_string_pretty(&Json::Object(app_obj))
            .map_err(|e| bad_ir(ir_path, format!("cannot serialize app.json: {e}")))?
            + "\n"),
        ir_path,
    )?;
    write(out, "app.js", &app_js, ir_path)?;

    // Size report (§7): main package bytes plus per-subpackage bytes. Wechat's
    // 2 MB single-package / 20 MB total limits are warning/error thresholds.
    // Only emitted for multi-route bundles; a single-route program keeps the
    // byte-identical emit_mini output.
    write_size_report(out, &routes, p, ir_path)
}

/// Sum every regular file's byte length under `root`.
fn dir_bytes(root: &Path) -> std::io::Result<usize> {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len() as usize;
            }
        }
    }
    Ok(total)
}

/// Write a `size-report.txt` accounting for the main package and each
/// subpackage, and fail with a diagnostic when the wechat limits are exceeded
/// (2 MB per single package, 20 MB total). Other mini platforms share wechat's
/// limits as a conservative bound.
fn write_size_report(out: &Path, routes: &ERoutes, p: &Platform, ir_path: &str) -> DResult<()> {
    const LIMIT_PACKAGE: usize = 2 * 1024 * 1024;
    const LIMIT_TOTAL: usize = 20 * 1024 * 1024;
    let total = dir_bytes(out).unwrap_or(0);
    // Subpackage bytes are the files under packages/<name>/.
    let mut sub_packs: Vec<(String, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for page in &routes.pages {
        if let Some(pack) = &page.subpackage {
            if seen.contains(pack) {
                continue;
            }
            seen.push(pack.clone());
            let bytes = dir_bytes(&out.join("packages").join(pack)).unwrap_or(0);
            sub_packs.push((pack.clone(), bytes));
        }
    }
    let sub_total: usize = sub_packs.iter().map(|(_, b)| *b).sum();
    let main_bytes = total.saturating_sub(sub_total);
    let mut report = String::new();
    report.push_str(&format!("mini size report ({})\n", p.key));
    // F1: "main package <N> B" plus one "<name> <N> B" line per subpackage.
    let mut warnings: Vec<String> = Vec::new();
    let warn = |bytes: usize, what: &str, warnings: &mut Vec<String>| {
        if bytes > LIMIT_PACKAGE {
            warnings.push(format!(
                "warning: {what} {bytes} B exceeds the 2 MB single-package limit"
            ));
        }
    };
    warn(main_bytes, "main package", &mut warnings);
    report.push_str(&format!("main package {main_bytes} B\n"));
    for (pack, bytes) in &sub_packs {
        warn(*bytes, pack, &mut warnings);
        report.push_str(&format!("{pack} {bytes} B\n"));
    }
    for line in &warnings {
        report.push_str(line);
        report.push('\n');
    }
    report.push_str(&format!("total {total} B\n"));
    // F2: the 20 MB total limit is the hard error; the 2 MB single-package
    // limit is a warning line above.
    if total > LIMIT_TOTAL {
        return Err(bad_ir(
            ir_path,
            format!(
                "total package {total} B exceeds the 20 MB total limit; reduce assets or split into more subpackages"
            ),
        ));
    }
    write(out, "size-report.txt", &report, ir_path)
}

// ------------------------------------------------- native multi-page ----

/// Multi-page native: the generated program embeds the whole IR (routes and
/// initialRoute included) so the render host resolves the route stack, and its
/// event dispatcher enumerates handlers across every route tree.
fn emit_native_multipage(
    app: &str,
    states: &[EState],
    computed: &[Computed],
    routes: &ERoutes,
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
    // A synthetic root carries every route tree as a child purely so the event
    // dispatcher sees all handlers; it is never rendered (the host reads the
    // embedded IR). Its id is reserved so it cannot collide with a real node.
    let combined = ENode {
        id: u64::MAX,
        kind: "Column".to_string(),
        props: JsonMap::new(),
        events: JsonMap::new(),
        children: routes.pages.iter().map(|p| p.tree.clone()).collect(),
        when: None,
        unless: None,
        repeat: None,
    };
    let src = native_source(app, states, computed, &combined, ir, ir_path, has_effects)?;
    write(out, "src/main.rs", &src, ir_path)
}

// ------------------------------------------------------------- harmony ----

fn harmony_pages(routes: &ERoutes) -> Vec<(String, &ENode)> {
    routes
        .pages
        .iter()
        .map(|page| (page.name.clone(), &page.tree))
        .collect()
}

/// Braces are escaped inside every string literal so a naive syntax check sees
/// only balanced ArkTS code braces.
fn arkts_str(s: &str) -> String {
    js_lit(&Json::String(s.to_string()))
        .replace('{', "\\u007B")
        .replace('}', "\\u007D")
}

fn arkts_value(value: &Json) -> String {
    match value {
        Json::String(s) => arkts_str(s),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(arkts_value).collect::<Vec<_>>().join(", ")
        ),
        Json::Object(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(key, value)| format!("{}: {}", arkts_str(key), arkts_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => js_lit(value),
    }
}

fn harmony_state_index(states: &[EState], name: &str, ir_path: &str) -> DResult<usize> {
    let name = name.strip_prefix("state.").unwrap_or(name);
    states
        .iter()
        .position(|state| state.name == name)
        .ok_or_else(|| {
            bad_ir(
                ir_path,
                format!("unknown state '{name}' in HarmonyOS emitter"),
            )
        })
}

fn harmony_state_ref(states: &[EState], name: &str, ir_path: &str) -> DResult<String> {
    Ok(format!(
        "this.s{}",
        harmony_state_index(states, name, ir_path)?
    ))
}

fn harmony_state_ty(ty: &str) -> &'static str {
    match ty {
        "int" => "number",
        "bool" => "boolean",
        "list" => "string[]",
        _ => "string",
    }
}

fn harmony_state_init(state: &EState) -> String {
    match state.ty.as_str() {
        "int" => state.init.as_i64().unwrap_or(0).to_string(),
        "bool" => state.init.as_bool().unwrap_or(false).to_string(),
        "list" => match &state.init {
            Json::Array(_) => arkts_value(&state.init),
            Json::String(s) => format!("[{}]", arkts_str(s)),
            _ => "[]".to_string(),
        },
        _ => state
            .init
            .as_str()
            .map(arkts_str)
            .unwrap_or_else(|| "\"\"".to_string()),
    }
}

fn harmony_bundle_name(app: &str) -> String {
    let suffix: String = app
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let name = if suffix.is_empty() {
        "app"
    } else {
        suffix.as_str()
    };
    format!("com.opurom.{name}")
}

fn harmony_key(route: usize, id: u64) -> u64 {
    route as u64 * 1_000_000 + id
}

fn harmony_method(event: &str, route: usize, id: u64) -> String {
    format!("{event}_r{route}_n{id}")
}

fn harmony_input_field(route: usize, id: u64) -> String {
    format!("input_r{route}_n{id}")
}

fn harmony_condition(n: &ENode, states: &[EState], ir_path: &str) -> DResult<String> {
    let mut clauses = Vec::new();
    if let Some(name) = &n.when {
        clauses.push(harmony_state_ref(states, name, ir_path)?);
    }
    if let Some(name) = &n.unless {
        clauses.push(format!("!{}", harmony_state_ref(states, name, ir_path)?));
    }
    Ok(clauses.join(" && "))
}

fn harmony_interp(
    s: &str,
    states: &[EState],
    aliases: &HashMap<String, usize>,
    ir_path: &str,
) -> DResult<String> {
    let parts = interp_parts(s).map_err(|(off, msg)| interp_error(s, off, msg, ir_path))?;
    let mut exprs = Vec::new();
    for part in parts {
        match part {
            Part::Lit(text) => exprs.push(arkts_str(&text)),
            Part::Var(name, _) => match aliases.get(&name) {
                Some(index) => exprs.push(format!("this.repeatValue({index})")),
                None => exprs.push(format!(
                    "String({})",
                    harmony_state_ref(states, &name, ir_path)?
                )),
            },
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
        format!("this.opuText([{}])", exprs.join(", "))
    })
}

fn harmony_text_prop(
    n: &ENode,
    key: &str,
    default: &str,
    states: &[EState],
    aliases: &HashMap<String, usize>,
    ir_path: &str,
) -> DResult<String> {
    harmony_interp(&prop_str(&n.props, key, default), states, aliases, ir_path)
}

fn harmony_modifiers(
    n: &ENode,
    key: u64,
    route: usize,
    pad: &str,
    textual: bool,
    out: &mut String,
) {
    let modifier_pad = format!("{pad}  ");
    out.push_str(&format!(
        "{modifier_pad}.backgroundColor(this.nodeColor({key}))\n"
    ));
    out.push_str(&format!(
        "{modifier_pad}.borderRadius(Opurom_THEME_RADIUS)\n"
    ));
    if textual {
        let size = prop_num(&n.props, "size", 16.0) * prop_num(&n.props, "fontScale", 1.0);
        out.push_str(&format!(
            "{modifier_pad}.fontSize(this.nodeFontSize({}))\n",
            fmt_num(size)
        ));
        out.push_str(&format!(
            "{modifier_pad}.fontColor(this.nodeColor({key}))\n"
        ));
    }
    if let Some(padding) = n.props.get("padding").and_then(Json::as_f64) {
        out.push_str(&format!("{modifier_pad}.padding({})\n", fmt_num(padding)));
    }
    if let Some(margin) = n.props.get("margin").and_then(Json::as_f64) {
        out.push_str(&format!("{modifier_pad}.margin({})\n", fmt_num(margin)));
    }
    if let Some(opacity) = n.props.get("opacity").and_then(Json::as_f64) {
        out.push_str(&format!("{modifier_pad}.opacity({opacity})\n"));
    }
    if n.props.get("secure").and_then(Json::as_bool) == Some(true) {
        out.push_str(&format!("{modifier_pad}.type(InputType.Password)\n"));
    }
    if n.events.contains_key("tap") {
        let method = harmony_method("tap", route, n.id);
        out.push_str(&format!(
            "{modifier_pad}.onClick(() => {{ this.{method}(); }})\n"
        ));
    }
    if n.events.contains_key("input") {
        let method = harmony_method("input", route, n.id);
        out.push_str(&format!(
            "{modifier_pad}.onChange((value: string) => {{ this.{method}(value); }})\n"
        ));
    }
    if n.events.contains_key("doubleTap") {
        let method = harmony_method("doubleTap", route, n.id);
        out.push_str(&format!(
            "{modifier_pad}.gesture(TapGesture({{ count: 2 }}).onAction(() => {{ this.{method}(); }}))\n"
        ));
    }
    if n.events.contains_key("longPress") {
        let method = harmony_method("longPress", route, n.id);
        out.push_str(&format!(
            "{modifier_pad}.gesture(LongPressGesture().onAction(() => {{ this.{method}(); }}))\n"
        ));
    }
    if n.events.contains_key("drag") {
        let method = harmony_method("drag", route, n.id);
        out.push_str(&format!(
            "{modifier_pad}.gesture(PanGesture().onActionStart(() => {{ this.{method}(); }}))\n"
        ));
    }
    out.push_str(&format!("{modifier_pad}.width('100%');\n"));
}

fn harmony_node(
    n: &ENode,
    states: &[EState],
    aliases: &HashMap<String, usize>,
    route: usize,
    depth: usize,
    out: &mut String,
    ir_path: &str,
) -> DResult<()> {
    let mut local_aliases = aliases.clone();
    if let Some(repeat) = &n.repeat {
        let item = repeat.get("item").and_then(Json::as_str).unwrap_or("");
        let list = repeat.get("list").and_then(Json::as_str).unwrap_or("");
        let index = harmony_state_index(states, list, ir_path)?;
        local_aliases.insert(item.to_string(), index);
    }
    let condition = harmony_condition(n, states, ir_path)?;
    let conditional = !condition.is_empty();
    let pad = "  ".repeat(depth);
    if conditional {
        out.push_str(&format!("{pad}if ({condition}) {{\n"));
    }
    let body_depth = depth + usize::from(conditional);
    let body_pad = "  ".repeat(body_depth);
    let key = harmony_key(route, n.id);
    match n.kind.as_str() {
        "Text" => {
            let text = harmony_text_prop(n, "value", "", states, &local_aliases, ir_path)?;
            out.push_str(&format!("{body_pad}Text({text})\n"));
            harmony_modifiers(n, key, route, &body_pad, true, out);
        }
        "Button" => {
            let label = harmony_text_prop(n, "label", "", states, &local_aliases, ir_path)?;
            out.push_str(&format!("{body_pad}Button() {{\n"));
            out.push_str(&format!("{body_pad}  Text({label})\n"));
            for child in &n.children {
                harmony_node(
                    child,
                    states,
                    &local_aliases,
                    route,
                    body_depth + 1,
                    out,
                    ir_path,
                )?;
            }
            out.push_str(&format!("{body_pad}}}\n"));
            harmony_modifiers(n, key, route, &body_pad, false, out);
        }
        "Input" => {
            let placeholder =
                harmony_text_prop(n, "placeholder", "", states, &local_aliases, ir_path)?;
            let field = harmony_input_field(route, n.id);
            out.push_str(&format!(
                "{body_pad}TextInput({{ placeholder: {placeholder}, text: this.{field} }})\n"
            ));
            harmony_modifiers(n, key, route, &body_pad, true, out);
        }
        "Row" | "Grid" => {
            out.push_str(&format!("{body_pad}Row() {{\n"));
            for child in &n.children {
                harmony_node(
                    child,
                    states,
                    &local_aliases,
                    route,
                    body_depth + 1,
                    out,
                    ir_path,
                )?;
            }
            out.push_str(&format!("{body_pad}}}\n"));
            harmony_modifiers(n, key, route, &body_pad, false, out);
        }
        _ => {
            out.push_str(&format!("{body_pad}Column() {{\n"));
            let label = fallback_text(n);
            if !label.is_empty() {
                let text = harmony_interp(&label, states, &local_aliases, ir_path)?;
                out.push_str(&format!("{body_pad}  Text({text})\n"));
            }
            for child in &n.children {
                harmony_node(
                    child,
                    states,
                    &local_aliases,
                    route,
                    body_depth + 1,
                    out,
                    ir_path,
                )?;
            }
            out.push_str(&format!("{body_pad}}}\n"));
            harmony_modifiers(n, key, route, &body_pad, false, out);
        }
    }
    if conditional {
        out.push_str(&format!("{pad}}}\n"));
    }
    Ok(())
}

fn harmony_effect_code(effects: &[Effect], states: &[EState], ir_path: &str) -> DResult<String> {
    let mut code = String::new();
    for effect in effects {
        match effect {
            Effect::Delay { ms } => code.push_str(&format!("await this.delay({ms}); ")),
            Effect::Fetch { path, into } => {
                let index = harmony_state_index(states, into, ir_path)?;
                code.push_str(&format!(
                    "await this.fetchInto({}, {index}); ",
                    arkts_str(path)
                ));
            }
        }
    }
    Ok(code)
}

fn harmony_action(action: &Json, states: &[EState], ir_path: &str) -> DResult<String> {
    let op = action_str(action, "op", ir_path)?;
    if op == "pop" {
        return Ok("this.pop(); ".to_string());
    }
    let target = action_str(action, "target", ir_path)?;
    Ok(match op {
        "push" => format!("this.push({}); ", arkts_str(target)),
        "validate" => format!("this.validateForm({}); ", arkts_str(target)),
        "set" => {
            let index = harmony_state_index(states, target, ir_path)?;
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => {
                    format!("this.s{}", harmony_state_index(states, from, ir_path)?)
                }
                None => arkts_value(action.get("value").unwrap_or(&Json::String(String::new()))),
            };
            format!("this.s{index} = {rhs}; ")
        }
        op @ ("add" | "sub") => {
            let index = harmony_state_index(states, target, ir_path)?;
            let rhs = match action.get("valueFrom").and_then(Json::as_str) {
                Some(from) => {
                    format!("this.s{}", harmony_state_index(states, from, ir_path)?)
                }
                None => action
                    .get("value")
                    .and_then(Json::as_i64)
                    .ok_or_else(|| {
                        bad_ir(ir_path, format!("'{op}' action needs an integer 'value'"))
                    })?
                    .to_string(),
            };
            let sign = if op == "sub" { '-' } else { '+' };
            format!("this.s{index} = this.s{index} {sign} ({rhs}); ")
        }
        "toggle" => {
            let index = harmony_state_index(states, target, ir_path)?;
            format!("this.s{index} = !this.s{index}; ")
        }
        other => return Err(bad_ir(ir_path, format!("unknown event op '{other}'"))),
    })
}

fn harmony_event_method(
    event_name: &str,
    n: &ENode,
    route: usize,
    states: &[EState],
    computed: &[Computed],
    ir_path: &str,
) -> DResult<String> {
    let event = n
        .events
        .get(event_name)
        .ok_or_else(|| bad_ir(ir_path, format!("missing event '{event_name}'")))?;
    let method = harmony_method(event_name, route, n.id);
    let effects = harmony_effect_code(&event_effects(event, ir_path)?, states, ir_path)?;
    let mut body = String::new();
    if event_name == "input" {
        let field = harmony_input_field(route, n.id);
        body.push_str(&format!("this.{field} = value; "));
        for action in event_actions(event) {
            if action.get("op").and_then(Json::as_str) == Some("set") {
                let target = action_str(action, "target", ir_path)?;
                let index = harmony_state_index(states, target, ir_path)?;
                match states[index].ty.as_str() {
                    "int" => body.push_str(&format!("this.s{index} = Number(value); ")),
                    "bool" => body.push_str(&format!("this.s{index} = value === 'true'; ")),
                    _ => body.push_str(&format!("this.s{index} = value; ")),
                }
            } else {
                body.push_str(&harmony_action(action, states, ir_path)?);
            }
        }
    } else {
        for action in event_actions(event) {
            body.push_str(&harmony_action(action, states, ir_path)?);
        }
    }
    if !computed.is_empty() {
        body.push_str("this.applyComputed(); ");
    }
    let params = if event_name == "input" {
        "(value: string)"
    } else {
        "()"
    };
    Ok(format!(
        "  private async {method}{params}: Promise<void> {{\n    {effects}{body}\n  }}\n"
    ))
}

fn harmony_json_text(value: &Json, ir_path: &str) -> DResult<String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| {
        bad_ir(
            ir_path,
            format!("cannot serialize HarmonyOS project JSON: {e}"),
        )
    })?;
    Ok(text + "\n")
}

fn harmony_project_files(app: &str, out: &Path, ir_path: &str) -> DResult<()> {
    let bundle = harmony_bundle_name(app);
    let package = json!({
        "name": "opurom-harmony",
        "version": "1.0.0",
        "description": app,
        "main": "hvigorfile.ts",
        "author": "opuromc",
        "license": "Apache-2.0",
        "modelVersion": "5.0.0",
        "dependencies": {},
        "devDependencies": {
            "@ohos/hvigor": "5.0.4",
            "@ohos/hvigor-ohos-plugin": "5.0.4"
        }
    });
    write(
        out,
        "oh-package.json5",
        &harmony_json_text(&package, ir_path)?,
        ir_path,
    )?;
    let build_profile = json!({
        "app": {
            "signingConfigs": [],
            "products": [{
                "name": "default",
                "compatibleSdkVersion": "5.0.0(12)",
                "runtimeOS": "HarmonyOS",
                "buildOption": {
                    "strictMode": {
                        "caseSensitiveCheck": true,
                        "useNormalizedOHMUrl": true
                    }
                }
            }],
            "buildModeSet": [{ "name": "debug" }, { "name": "release" }]
        },
        "modules": [{
            "name": "entry",
            "srcPath": ".",
            "targets": [{
                "name": "default",
                "applyToProducts": ["default"]
            }]
        }],
        "apiType": "stageMode",
        "buildOption": {
            "arkOptions": { "runtimeOnly": { "packages": [] } }
        },
        "targets": [{ "name": "default" }, { "name": "ohosTest" }]
    });
    write(
        out,
        "build-profile.json5",
        &harmony_json_text(&build_profile, ir_path)?,
        ir_path,
    )?;
    write(
        out,
        "hvigorfile.ts",
        "import { appTasks, hapTasks } from '@ohos/hvigor-ohos-plugin';\n\n\
         export default {\n  system: appTasks,\n  plugins: [],\n};\n\n\
         export const moduleTasks = hapTasks;\n",
        ir_path,
    )?;
    let app_scope = json!({
        "app": {
            "bundleName": bundle,
            "vendor": "opurom",
            "versionCode": 1000000,
            "versionName": "1.0.0",
            "label": "$string:app_name",
            "minAPIVersion": 12,
            "targetAPIVersion": 12
        }
    });
    write(
        out,
        "AppScope/app.json5",
        &harmony_json_text(&app_scope, ir_path)?,
        ir_path,
    )?;
    let strings = json!({ "string": [{ "name": "app_name", "value": app }] });
    write(
        out,
        "AppScope/resources/base/element/string.json",
        &harmony_json_text(&strings, ir_path)?,
        ir_path,
    )?;
    let module = json!({
        "module": {
            "name": "entry",
            "type": "entry",
            "description": "Generated HarmonyOS HAP module",
            "mainElement": "EntryAbility",
            "deviceTypes": ["phone", "tablet", "2in1", "tv", "wearable", "car"],
            "deliveryWithInstall": true,
            "installationFree": false,
            "pages": "$profile:main_pages",
            "abilities": [{
                "name": "EntryAbility",
                "srcEntry": "./ets/entryability/EntryAbility.ets",
                "description": "Generated Opurom entry",
                "label": app,
                "exported": true,
                "skills": [{
                    "entities": ["entity.system.home"],
                    "actions": ["action.system.home"]
                }]
            }]
        }
    });
    write(
        out,
        "src/main/module.json5",
        &harmony_json_text(&module, ir_path)?,
        ir_path,
    )?;
    write(
        out,
        "src/main/ets/entryability/EntryAbility.ets",
        "import { UIAbility } from '@kit.AbilityKit';\n\
         import { window } from '@kit.ArkUI';\n\
         import { BusinessError } from '@kit.BasicServicesKit';\n\n\
         export default class EntryAbility extends UIAbility {\n\
         \x20 onWindowStageCreate(windowStage: window.WindowStage): void {\n\
         \x20   windowStage.loadContent('pages/Index', (err: BusinessError) => {\n\
         \x20     if (err.code) {\n\
         \x20       console.error('Failed to load Opurom content: ' + JSON.stringify(err));\n\
         \x20     }\n\
         \x20   });\n\
         \x20 }\n\
         }\n",
        ir_path,
    )?;
    let main_pages = json!({ "src": ["pages/Index"] });
    write(
        out,
        "src/main/resources/base/profile/main_pages.json",
        &harmony_json_text(&main_pages, ir_path)?,
        ir_path,
    )
}

fn harmony_theme_number(theme: Option<&Json>, key: &str, default: f64) -> f64 {
    theme
        .and_then(|value| value.get(key))
        .and_then(Json::as_f64)
        .unwrap_or(default)
}

fn harmony_theme_token(tokens: Option<&JsonMap<String, Json>>, key: &str, default: &str) -> String {
    tokens
        .and_then(|map| map.get(key))
        .and_then(Json::as_str)
        .unwrap_or(default)
        .to_string()
}

fn harmony_source(
    app: &str,
    states: &[EState],
    computed: &[Computed],
    pages: &[(String, &ENode)],
    initial: &str,
    multipage: bool,
    ir: &Json,
    ir_path: &str,
) -> DResult<String> {
    let theme = ir.get("theme");
    let tokens = theme
        .and_then(|value| value.get("tokens"))
        .and_then(Json::as_object);
    let ir_json = serde_json::to_string(ir)
        .map_err(|e| bad_ir(ir_path, format!("cannot serialize IR: {e}")))?;
    let theme_json = match theme {
        Some(value) => serde_json::to_string(value)
            .map_err(|e| bad_ir(ir_path, format!("cannot serialize theme: {e}")))?,
        None => "{}".to_string(),
    };
    let mut metas = Vec::new();
    let mut all_nodes = Vec::new();
    for (route, (_, tree_ref)) in pages.iter().enumerate() {
        let tree = *tree_ref;
        if let Some(meta) = additive_meta(tree) {
            metas.push(meta);
        }
        let mut page_nodes = Vec::new();
        collect(tree, &mut page_nodes);
        for node in page_nodes {
            all_nodes.push((route, node));
        }
    }
    let meta_json = if metas.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", metas.join(","))
    };
    let route_names = pages
        .iter()
        .map(|(name, _)| arkts_str(name))
        .collect::<Vec<_>>()
        .join(", ");
    let token_entries = tokens
        .map(|map| {
            map.iter()
                .map(|(key, value)| {
                    let text = match value {
                        Json::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("    [{}, {}]", arkts_str(key), arkts_str(&text))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let token_map = if token_entries.is_empty() {
        "new Map<string, string>()".to_string()
    } else {
        format!(
            "new Map<string, string>([\n{}\n  ])",
            token_entries.join(",\n")
        )
    };
    let mut design_entries = Vec::new();
    let mut validator_entries = Vec::new();
    let mut input_nodes = Vec::new();
    let mut methods = String::new();
    for (route, node) in &all_nodes {
        let key = harmony_key(*route, node.id);
        let state = node.props.get("state").and_then(Json::as_str).unwrap_or("");
        let variant = node
            .props
            .get("variant")
            .and_then(Json::as_str)
            .unwrap_or("");
        let tone = node.props.get("tone").and_then(Json::as_str).unwrap_or("");
        if !(state.is_empty() && variant.is_empty() && tone.is_empty()) {
            design_entries.push(format!(
                "new OpuromDesign({key}, {}, {}, {})",
                arkts_str(state),
                arkts_str(variant),
                arkts_str(tone)
            ));
        }
        let has_validator = node.kind == "Input"
            && VALIDATOR_PROPS
                .iter()
                .chain(TEXT_BUFFER_PROPS.iter())
                .any(|key| node.props.contains_key(*key));
        if has_validator {
            let required = node
                .props
                .get("required")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            let min_length = node
                .props
                .get("minLength")
                .and_then(Json::as_i64)
                .unwrap_or(-1);
            let max_length = node
                .props
                .get("maxLength")
                .and_then(Json::as_i64)
                .unwrap_or(-1);
            let pattern = node
                .props
                .get("pattern")
                .and_then(Json::as_str)
                .unwrap_or("");
            let text_buffer = node
                .props
                .get("textBuffer")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            let secure = node
                .props
                .get("secure")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            let value = node.props.get("value").and_then(Json::as_str).unwrap_or("");
            validator_entries.push(format!(
                "new OpuromValidator({key}, {required}, {min_length}, {max_length}, {}, {text_buffer}, {secure}, {})",
                arkts_str(pattern),
                arkts_str(value)
            ));
        }
        if node.kind == "Input" || node.events.contains_key("input") {
            input_nodes.push((*route, *node));
        }
        for event_name in ["tap", "input", "doubleTap", "longPress", "drag"] {
            if node.events.contains_key(event_name) {
                methods.push_str(&harmony_event_method(
                    event_name, node, *route, states, computed, ir_path,
                )?);
            }
        }
    }

    let mut code = String::new();
    code.push_str("import { http } from '@kit.NetworkKit';\n\n");
    code.push_str(&format!("const Opurom_APP: string = {};\n", arkts_str(app)));
    code.push_str(&format!(
        "const Opurom_IR_JSON: string = {};\n",
        arkts_str(&ir_json)
    ));
    code.push_str(&format!(
        "const Opurom_META_JSON: string = {};\n",
        arkts_str(&meta_json)
    ));
    code.push_str(&format!(
        "const Opurom_THEME_JSON: string = {};\n",
        arkts_str(&theme_json)
    ));
    code.push_str(&format!(
        "const Opurom_ROUTES: string[] = [{route_names}];\n"
    ));
    code.push_str("const Opurom_NAV_SIGNAL: string = '@nav';\n");
    code.push_str("const Opurom_VALIDATE_SIGNAL: string = '@validate';\n");
    code.push_str("const Opurom_BASE_URL: string = '';\n");
    code.push_str(&format!(
        "const Opurom_THEME_MODE: string = {};\n",
        arkts_str(
            theme
                .and_then(|value| value.get("mode"))
                .and_then(Json::as_str)
                .unwrap_or("system")
        )
    ));
    code.push_str(&format!(
        "const Opurom_THEME_ACCENT: string = {};\n",
        arkts_str(
            theme
                .and_then(|value| value.get("accent"))
                .and_then(Json::as_str)
                .unwrap_or("#097FEB")
        )
    ));
    code.push_str(&format!(
        "const Opurom_THEME_RADIUS: number = {};\n",
        fmt_num(harmony_theme_number(theme, "radius", 12.0))
    ));
    code.push_str(&format!(
        "const Opurom_THEME_FONT_SCALE: number = {};\n",
        fmt_num(harmony_theme_number(theme, "fontScale", 1.0))
    ));
    code.push_str(&format!(
        "const Opurom_THEME_SURFACE: string = {};\n",
        arkts_str(&harmony_theme_token(tokens, "surface", "#FFFFFF"))
    ));
    code.push_str(&format!(
        "const Opurom_THEME_STAGE: string = {};\n",
        arkts_str(&harmony_theme_token(tokens, "stage", "#F3F4F6"))
    ));
    code.push_str(&format!(
        "const Opurom_THEME_ON_SURFACE: string = {};\n",
        arkts_str(&harmony_theme_token(tokens, "onSurface", "#101828"))
    ));
    code.push_str(&format!(
        "const Opurom_THEME_TOKENS: Map<string, string> = {token_map};\n\n"
    ));
    code.push_str(
        r#"class OpuromDesign {
  id: number;
  state: string;
  variant: string;
  tone: string;

  constructor(id: number, state: string, variant: string, tone: string) {
    this.id = id;
    this.state = state;
    this.variant = variant;
    this.tone = tone;
  }
}

"#,
    );
    code.push_str("const Opurom_DESIGN: OpuromDesign[] = [\n");
    for entry in design_entries {
        code.push_str(&format!("  {entry},\n"));
    }
    code.push_str("];\n\n");
    code.push_str(
        r#"class OpuromValidator {
  id: number;
  required: boolean;
  minLength: number;
  maxLength: number;
  pattern: string;
  textBuffer: boolean;
  secure: boolean;
  value: string;

  constructor(id: number, required: boolean, minLength: number, maxLength: number,
    pattern: string, textBuffer: boolean, secure: boolean, value: string) {
    this.id = id;
    this.required = required;
    this.minLength = minLength;
    this.maxLength = maxLength;
    this.pattern = pattern;
    this.textBuffer = textBuffer;
    this.secure = secure;
    this.value = value;
  }
}

"#,
    );
    code.push_str("const Opurom_VALIDATORS: OpuromValidator[] = [\n");
    for entry in validator_entries {
        code.push_str(&format!("  {entry},\n"));
    }
    code.push_str("];\n\n");
    code.push_str("@Entry\n@Component\nstruct Index {\n");
    code.push_str(&format!(
        "  @State routeStack: string[] = [{}];\n",
        arkts_str(initial)
    ));
    code.push_str(&format!(
        "  @State currentRoute: string = {};\n",
        arkts_str(initial)
    ));
    code.push_str("  @State lastSignal: string = '';\n");
    code.push_str("  @State validationMessage: string = '';\n");
    for (index, state) in states.iter().enumerate() {
        code.push_str(&format!(
            "  @State s{index}: {} = {};\n",
            harmony_state_ty(&state.ty),
            harmony_state_init(state)
        ));
    }
    for (route, node) in &input_nodes {
        let field = harmony_input_field(*route, node.id);
        let init = match node.props.get("bind").and_then(Json::as_str) {
            Some(name) => {
                let index = harmony_state_index(states, name, ir_path)?;
                states[index]
                    .init
                    .as_str()
                    .map(arkts_str)
                    .unwrap_or_else(|| "\"\"".to_string())
            }
            None => "\"\"".to_string(),
        };
        code.push_str(&format!("  @State {field}: string = {init};\n"));
    }
    code.push_str("\n");
    code.push_str(
        r#"  private opuText(parts: string[]): string {
    return parts.join('');
  }

  private nodeFontSize(size: number): number {
    return size * Opurom_THEME_FONT_SCALE;
  }

  private setStateText(index: number, value: string): void {
    switch (index) {
"#,
    );
    for (index, state) in states.iter().enumerate() {
        if state.ty == "string" {
            code.push_str(&format!(
                "      case {index}: this.s{index} = value; break;\n"
            ));
        }
    }
    code.push_str(
        r#"      default: break;
    }
  }

  private repeatValue(index: number): string {
    switch (index) {
"#,
    );
    for (index, state) in states.iter().enumerate() {
        if state.ty == "list" {
            code.push_str(&format!(
                "      case {index}: return this.s{index}.length > 0 ? this.s{index}[0] : '';\n"
            ));
        }
    }
    code.push_str(
        r#"      default: return '';
    }
  }

  private inputValue(id: number): string {
    switch (id) {
"#,
    );
    for (route, node) in &input_nodes {
        let field = harmony_input_field(*route, node.id);
        let key = harmony_key(*route, node.id);
        code.push_str(&format!("      case {key}: return this.{field};\n"));
    }
    code.push_str(
        r#"      default: return '';
    }
  }

  private applyComputed(): void {
"#,
    );
    for entry in computed {
        let (_, expression) =
            computed_expr(&entry.expr, states, computed, ir_path, false, |state| {
                let index = harmony_state_index(states, &state.name, ir_path)?;
                Ok(format!("this.s{index}"))
            })?;
        let index = harmony_state_index(states, &entry.name, ir_path)?;
        code.push_str(&format!("    this.s{index} = {expression};\n"));
    }
    code.push_str(
        r#"  }

  private designOf(id: number): OpuromDesign {
    for (const design of Opurom_DESIGN) {
      if (design.id === id) {
        return design;
      }
    }
    return new OpuromDesign(id, '', '', '');
  }

  private nodeColor(id: number): string {
    const design = this.designOf(id);
    if (design.tone.length > 0) {
      const token = Opurom_THEME_TOKENS.get(design.tone);
      if (token !== undefined) {
        return token;
      }
    }
    if (design.variant === 'primary' || design.variant === 'fab') {
      return Opurom_THEME_ACCENT;
    }
    if (design.state === 'disabled') {
      return '#9CA3AF';
    }
    return Opurom_THEME_ON_SURFACE;
  }

  private pageBackground(): string {
    if (Opurom_THEME_MODE === 'dark') {
      return Opurom_THEME_STAGE;
    }
    return Opurom_THEME_SURFACE;
  }

  private push(route: string): void {
    if (Opurom_ROUTES.indexOf(route) >= 0) {
      this.routeStack = this.routeStack.concat([route]);
      this.currentRoute = route;
    }
    this.lastSignal = Opurom_NAV_SIGNAL + ':' + route;
  }

  private pop(): void {
    if (this.routeStack.length > 1) {
      this.routeStack = this.routeStack.slice(0, -1);
      this.currentRoute = this.routeStack[this.routeStack.length - 1];
    }
    this.lastSignal = Opurom_NAV_SIGNAL + ':@pop';
  }

  private matchesPattern(value: string, pattern: string): boolean {
    try {
      return new RegExp(pattern).test(value);
    } catch (error) {
      return false;
    }
  }

  private validateForm(name: string): void {
    this.lastSignal = Opurom_VALIDATE_SIGNAL + ':' + name;
    for (const validator of Opurom_VALIDATORS) {
      const value = this.inputValue(validator.id);
      if (validator.required && value.length === 0) {
        this.validationMessage = 'required';
        return;
      }
      if (validator.minLength >= 0 && value.length < validator.minLength) {
        this.validationMessage = 'too short';
        return;
      }
      if (validator.maxLength >= 0 && value.length > validator.maxLength) {
        this.validationMessage = 'too long';
        return;
      }
      if (validator.pattern.length > 0 && !this.matchesPattern(value, validator.pattern)) {
        this.validationMessage = 'pattern mismatch';
        return;
      }
      if (validator.secure && value.length > 0) {
        this.validationMessage = 'secure input accepted';
      }
    }
    this.validationMessage = this.validationMessage.length > 0 ? this.validationMessage : 'valid';
  }

  private delay(ms: number): Promise<void> {
    return new Promise<void>((resolve: () => void) => {
      setTimeout(resolve, ms);
    });
  }

  private async fetchInto(path: string, stateIndex: number): Promise<void> {
    const request = http.createHttp();
    try {
      const response = await request.request(Opurom_BASE_URL + path, {
        method: http.RequestMethod.GET
      });
      const text = typeof response.result === 'string' ? response.result : JSON.stringify(response.result);
      this.setStateText(stateIndex, text);
    } catch (error) {
      this.setStateText(stateIndex, 'error: request failed');
    } finally {
      request.destroy();
    }
  }

"#,
    );
    code.push_str(&methods);
    code.push_str("  build() {\n    Column() {\n");
    code.push_str(
        r#"      Text(Opurom_APP)
        .fontColor(Opurom_THEME_ACCENT)
        .fontSize(this.nodeFontSize(20))
        .width('100%');
      Text(this.lastSignal)
        .fontColor(this.nodeColor(0))
        .fontSize(this.nodeFontSize(12))
        .width('100%');
      Text(this.validationMessage)
        .fontColor(this.nodeColor(0))
        .fontSize(this.nodeFontSize(12))
        .width('100%');
"#,
    );
    let aliases = HashMap::new();
    if multipage {
        for (route, (name, tree_ref)) in pages.iter().enumerate() {
            let tree = *tree_ref;
            code.push_str(&format!(
                "      if (this.currentRoute === {}) {{\n",
                arkts_str(name)
            ));
            harmony_node(tree, states, &aliases, route, 4, &mut code, ir_path)?;
            code.push_str("      }\n");
        }
    } else {
        let tree = pages[0].1;
        harmony_node(tree, states, &aliases, 0, 3, &mut code, ir_path)?;
    }
    code.push_str(
        r#"    }
    .width('100%')
    .height('100%')
    .padding(Opurom_THEME_RADIUS)
    .backgroundColor(this.pageBackground());
  }
}
"#,
    );
    Ok(code)
}

fn emit_harmony(
    app: &str,
    states: &[EState],
    computed: &[Computed],
    pages: &[(String, &ENode)],
    initial: &str,
    multipage: bool,
    ir: &Json,
    out: &Path,
    ir_path: &str,
) -> DResult<()> {
    harmony_project_files(app, out, ir_path)?;
    write(
        out,
        "src/main/ets/pages/Index.ir.json",
        &harmony_json_text(ir, ir_path)?,
        ir_path,
    )?;
    let source = harmony_source(
        app, states, computed, pages, initial, multipage, ir, ir_path,
    )?;
    write(out, "src/main/ets/pages/Index.ets", &source, ir_path)
}
