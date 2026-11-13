use std::collections::{HashMap, HashSet};

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::catalog;
use crate::diag::{DResult, Diagnostic};
use crate::lexer::lex;
use crate::modules;
use crate::parser::{
    self, Arg, ComponentDecl, Effect, ForIter, Lit, Node, NodeKind, Pos, Program, Stmt, StmtKind,
    Value as AstValue,
};
use crate::typecheck;

pub const KINDS: &[&str] = &[
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
    "Gauge",
    "Sparkline",
    "AreaChart",
    "PieChart",
    "DonutChart",
    "ScatterPlot",
    "StackedBar",
    "Histogram",
    "Heatmap",
    "Treemap",
    "Waterfall",
    "BulletChart",
    "FunnelChart",
    "Waveform",
    "Equalizer",
    "Candlestick",
    "BubbleChart",
    "PolarChart",
    "Breadcrumbs",
    "Pagination",
    "Stepper",
    "Sidebar",
    "TabStrip",
    "NavRail",
    "AnchorBar",
    "ProgressSteps",
    "MenuBar",
    "TocTree",
    "Rating",
    "Dial",
    "Knob",
    "ColorWell",
    "DateField",
    "TimeField",
    "SelectBox",
    "ComboBox",
    "SearchBar",
    "PinInput",
    "SegmentedInput",
    "UploadZone",
    "WaveformPlayer",
    "Spectrogram",
    "Filmstrip",
    "ThumbnailGrid",
    "QrGlyph",
    "BarcodeGlyph",
    "ColorPalette",
    "ZoomLens",
    "LevelMeter",
    "SignalBars",
    "BatteryMeter",
    "Thermometer",
    "Speedometer",
    "Compass",
    "ProgressRing",
    "ActivityRing",
    "KpiTile",
    "StatCard",
    "CornerFrame",
    "CrosshairMark",
    "LadderDivider",
    "ChevronDivider",
    "GridOverlay",
    "CheckerBackdrop",
    "DotPattern",
    "RingBadge",
    "StairDivider",
    "BracketPanel",
];

/// The 68 extended primitive kinds (COMPONENT_LIBRARY_HARDENING section 1):
/// each is a distinct drawn primitive with a documented prop schema and an a11y
/// role.
pub const EXTENDED_PRIMITIVE_KINDS: &[&str] = &[
    "Gauge",
    "Sparkline",
    "AreaChart",
    "PieChart",
    "DonutChart",
    "ScatterPlot",
    "StackedBar",
    "Histogram",
    "Heatmap",
    "Treemap",
    "Waterfall",
    "BulletChart",
    "FunnelChart",
    "Waveform",
    "Equalizer",
    "Candlestick",
    "BubbleChart",
    "PolarChart",
    "Breadcrumbs",
    "Pagination",
    "Stepper",
    "Sidebar",
    "TabStrip",
    "NavRail",
    "AnchorBar",
    "ProgressSteps",
    "MenuBar",
    "TocTree",
    "Rating",
    "Dial",
    "Knob",
    "ColorWell",
    "DateField",
    "TimeField",
    "SelectBox",
    "ComboBox",
    "SearchBar",
    "PinInput",
    "SegmentedInput",
    "UploadZone",
    "WaveformPlayer",
    "Spectrogram",
    "Filmstrip",
    "ThumbnailGrid",
    "QrGlyph",
    "BarcodeGlyph",
    "ColorPalette",
    "ZoomLens",
    "LevelMeter",
    "SignalBars",
    "BatteryMeter",
    "Thermometer",
    "Speedometer",
    "Compass",
    "ProgressRing",
    "ActivityRing",
    "KpiTile",
    "StatCard",
    "CornerFrame",
    "CrosshairMark",
    "LadderDivider",
    "ChevronDivider",
    "GridOverlay",
    "CheckerBackdrop",
    "DotPattern",
    "RingBadge",
    "StairDivider",
    "BracketPanel",
];

/// Generic props every kind accepts: canonical visual props plus accessibility keys.
const GENERIC_PROPS: &[&str] = &[
    "padding",
    "margin",
    "color",
    "radius",
    "shadow",
    "gradient",
    "glass",
    "opacity",
    "border",
    "animate",
    "label",
    "role",
    "fontScale",
    "highContrast",
    "particles",
];

/// Atomic source shortcuts and the canonical prop they lower to.
const SHORTCUTS: &[(&str, &str)] = &[
    ("p", "padding"),
    ("m", "margin"),
    ("bg", "color"),
    ("rounded", "radius"),
];

/// Numeric props every kind accepts (canonical visual props).
const GENERIC_NUMERIC: &[&str] = &["padding", "margin", "radius"];

const HANDLER_VERBS: &[&str] = &["list", "get", "create", "update", "delete"];

const ANIMATE_VALUES: &[&str] = &["spring", "ease", "none"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Bool,
    Str,
    List,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Bool => "bool",
            Ty::Str => "string",
            Ty::List => "list",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Part {
    Lit(String),
    Var(String, usize),
}

fn valid_ident(s: &str) -> bool {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Split a string into literal parts and `{state}` references. `{{` is a literal `{`.
/// Errors carry the char offset inside the string.
pub fn interp_parts(s: &str) -> Result<Vec<Part>, (usize, String)> {
    let cs: Vec<char> = s.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0usize;
    while i < cs.len() {
        if cs[i] == '{' {
            if i + 1 < cs.len() && cs[i + 1] == '{' {
                lit.push('{');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < cs.len() && cs[j] != '}' && cs[j] != '{' {
                j += 1;
            }
            if j >= cs.len() || cs[j] != '}' {
                return Err((i, "unterminated '{name}' interpolation".to_string()));
            }
            let name: String = cs[i + 1..j].iter().collect();
            if !valid_ident(&name) {
                return Err((i, format!("invalid interpolation '{{{name}}}'")));
            }
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            parts.push(Part::Var(name, i + 1));
            i = j + 1;
        } else if cs[i] == '}' {
            if i + 1 < cs.len() && cs[i + 1] == '}' {
                lit.push('}');
                i += 2;
                continue;
            }
            return Err((i, "unmatched '}' in a string".to_string()));
        } else {
            lit.push(cs[i]);
            i += 1;
        }
    }
    if !lit.is_empty() {
        parts.push(Part::Lit(lit));
    }
    Ok(parts)
}

pub fn offset_pos(base: Pos, s: &str, offset: usize) -> Pos {
    let prefix: Vec<char> = s.chars().take(offset).collect();
    let newlines = prefix.iter().filter(|c| **c == '\n').count();
    if newlines == 0 {
        return Pos {
            line: base.line,
            col: base.col + 1 + offset,
        };
    }
    let last = prefix.iter().rposition(|c| *c == '\n').unwrap_or(0);
    Pos {
        line: base.line + newlines,
        col: offset - last,
    }
}

pub fn compile_source(src: &str, path: &str) -> DResult<Json> {
    // Validate the vendored catalog once per compiler run, before any source is lowered.
    catalog::catalog()?;
    let toks = lex(src, path)?;
    let program = parser::parse(toks, path)?;
    let program = modules::resolve_imports(program, path)?;
    lower(&program, path)
}

struct KindInfo {
    props: &'static [&'static str],
    required: &'static [&'static str],
    /// Kind-specific props that must hold an integer.
    numeric: &'static [&'static str],
    positional: Option<&'static str>,
    children: bool,
    tap: bool,
}

fn kind_info(kind: &str) -> Option<KindInfo> {
    let info = match kind {
        "Column" | "Row" => KindInfo {
            props: &["gap", "padding", "align", "width"],
            required: &[],
            numeric: &["gap", "width"],
            positional: None,
            children: true,
            tap: false,
        },
        "Container" => KindInfo {
            props: &["padding", "width", "height", "radius", "color"],
            required: &[],
            numeric: &["width", "height"],
            positional: None,
            children: true,
            tap: false,
        },
        "Text" => KindInfo {
            props: &["value", "size", "color"],
            required: &["value"],
            numeric: &["size"],
            positional: Some("value"),
            children: false,
            tap: false,
        },
        "Button" => KindInfo {
            props: &["label"],
            required: &["label"],
            numeric: &[],
            positional: Some("label"),
            children: false,
            tap: true,
        },
        "Input" => KindInfo {
            props: &["placeholder", "bind"],
            required: &[],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Image" => KindInfo {
            props: &["src", "width", "height"],
            required: &["src"],
            numeric: &["width", "height"],
            positional: None,
            children: false,
            tap: false,
        },
        "Spacer" => KindInfo {
            props: &["width", "height"],
            required: &[],
            numeric: &["width", "height"],
            positional: None,
            children: false,
            tap: false,
        },
        "Card" => KindInfo {
            props: &[],
            required: &[],
            numeric: &[],
            positional: None,
            children: true,
            tap: false,
        },
        "Divider" => KindInfo {
            props: &[],
            required: &[],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Icon" => KindInfo {
            props: &["name"],
            required: &["name"],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Badge" => KindInfo {
            props: &["label"],
            required: &["label"],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Progress" => KindInfo {
            props: &["value"],
            required: &["value"],
            numeric: &["value"],
            positional: None,
            children: false,
            tap: false,
        },
        "Switch" => KindInfo {
            props: &["bind"],
            required: &["bind"],
            numeric: &[],
            positional: None,
            children: false,
            tap: true,
        },
        "Slider" => KindInfo {
            props: &["bind", "min", "max"],
            required: &["bind", "min", "max"],
            numeric: &["min", "max"],
            positional: None,
            children: false,
            tap: false,
        },
        "Checkbox" => KindInfo {
            props: &["bind"],
            required: &["bind"],
            numeric: &[],
            positional: None,
            children: false,
            tap: true,
        },
        "Radio" => KindInfo {
            props: &["bind", "value"],
            required: &["bind", "value"],
            numeric: &[],
            positional: None,
            children: false,
            tap: true,
        },
        "Dialog" => KindInfo {
            props: &["title"],
            required: &["title"],
            numeric: &[],
            positional: None,
            children: true,
            tap: false,
        },
        "Tabs" => KindInfo {
            props: &["labels", "bind"],
            required: &["labels"],
            numeric: &[],
            positional: None,
            children: false,
            tap: true,
        },
        "List" => KindInfo {
            props: &[],
            required: &[],
            numeric: &[],
            positional: None,
            children: true,
            tap: false,
        },
        "Grid" => KindInfo {
            props: &["columns"],
            required: &["columns"],
            numeric: &["columns"],
            positional: None,
            children: true,
            tap: false,
        },
        "Scroll" => KindInfo {
            props: &["height"],
            required: &["height"],
            numeric: &["height"],
            positional: None,
            children: true,
            tap: false,
        },
        "Avatar" => KindInfo {
            props: &["label"],
            required: &["label"],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Spinner" => KindInfo {
            props: &[],
            required: &[],
            numeric: &[],
            positional: None,
            children: false,
            tap: false,
        },
        "Table" => KindInfo {
            props: &["columns", "rows", "gap"],
            required: &[],
            numeric: &["columns", "rows", "gap"],
            positional: None,
            children: true,
            tap: false,
        },
        "Chart" => KindInfo {
            props: &["type", "width", "height"],
            required: &[],
            numeric: &["width", "height"],
            positional: None,
            children: false,
            tap: false,
        },
        "Calendar" => KindInfo {
            props: &["date", "height"],
            required: &[],
            numeric: &["height"],
            positional: None,
            children: false,
            tap: false,
        },
        "RichText" => KindInfo {
            props: &["value", "size", "color"],
            required: &[],
            numeric: &["size"],
            positional: Some("value"),
            children: false,
            tap: false,
        },
        "Player" => KindInfo {
            props: &["src", "width", "height"],
            required: &[],
            numeric: &["width", "height"],
            positional: None,
            children: false,
            tap: false,
        },
        "Map" => KindInfo {
            props: &["center", "zoom", "height"],
            required: &[],
            numeric: &["zoom", "height"],
            positional: None,
            children: false,
            tap: false,
        },
        "Menu" => KindInfo {
            props: &["items", "width"],
            required: &[],
            numeric: &["width"],
            positional: None,
            children: true,
            tap: false,
        },
        "Tooltip" => KindInfo {
            props: &["text", "placement", "width"],
            required: &[],
            numeric: &["width"],
            positional: Some("text"),
            children: true,
            tap: false,
        },
        // Extended primitives share one documented, reasonable prop schema:
        // all props are optional, `value` is positional, and none take children.
        kind if EXTENDED_PRIMITIVE_KINDS.contains(&kind) => KindInfo {
            props: &[
                "value", "values", "segments", "labels", "min", "max", "size", "src", "type",
                "width", "height", "columns", "rows", "gap", "align",
            ],
            required: &[],
            numeric: &[
                "value", "segments", "min", "max", "size", "width", "height", "columns", "rows",
                "gap",
            ],
            positional: Some("value"),
            children: false,
            tap: false,
        },
        _ => return None,
    };
    Some(info)
}

#[derive(Clone, Debug)]
enum LoopValue {
    Int(i64),
    Str(String),
}

impl LoopValue {
    fn text(&self) -> String {
        match self {
            LoopValue::Int(n) => n.to_string(),
            LoopValue::Str(s) => s.clone(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Cond<'a> {
    when: Option<&'a str>,
    unless: Option<&'a str>,
}

struct Ctx<'a> {
    path: &'a str,
    types: HashMap<String, Ty>,
    order: Vec<String>,
    next_id: u64,
    computed: HashSet<String>,
    inits: HashMap<String, Json>,
    lists: HashMap<String, Vec<String>>,
    /// Loop variables of enclosing `for` blocks, innermost last.
    loops: Vec<(String, LoopValue)>,
}

impl<'a> Ctx<'a> {
    fn err(&self, pos: Pos, message: impl Into<String>, help: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.path, pos.line, pos.col, message, help)
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn state_list(&self) -> String {
        if self.order.is_empty() {
            "(no states declared)".to_string()
        } else {
            self.order.join(", ")
        }
    }

    fn loop_value(&self, name: &str) -> Option<String> {
        self.loops
            .iter()
            .rev()
            .find(|(v, _)| v == name)
            .map(|(_, value)| value.text())
    }
}

fn json_lit(l: &Lit) -> Json {
    match l {
        Lit::Int(n) => json!(*n),
        Lit::Float(n) => json!(*n),
        Lit::Bool(b) => json!(*b),
        Lit::Str(s) => json!(s),
        Lit::List(values) => json!(values),
    }
}

fn value_desc(v: &AstValue) -> &'static str {
    match v {
        AstValue::Lit(Lit::Int(_)) => "int",
        AstValue::Lit(Lit::Float(_)) => "number",
        AstValue::Lit(Lit::Bool(_)) => "bool",
        AstValue::Lit(Lit::Str(_)) => "string",
        AstValue::Lit(Lit::List(_)) => "list",
        AstValue::Ident(_) => "identifier",
    }
}

/// Replace `{i}` with the loop index for every enclosing `for` variable.
fn substitute_loops(ctx: &Ctx, s: &str) -> String {
    if ctx.loops.is_empty() {
        return s.to_string();
    }
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < cs.len() {
        if cs[i] == '{' {
            if i + 1 < cs.len() && cs[i + 1] == '{' {
                out.push('{');
                out.push('{');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < cs.len() && cs[j] != '}' && cs[j] != '{' {
                j += 1;
            }
            if j < cs.len() && cs[j] == '}' {
                let name: String = cs[i + 1..j].iter().collect();
                if let Some(v) = ctx.loop_value(&name) {
                    out.push_str(&v);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

fn is_color(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

fn check_color(ctx: &Ctx, pos: Pos, key: &str, value: &str) -> DResult<()> {
    if is_color(value) {
        return Ok(());
    }
    Err(ctx.err(
        pos,
        format!("invalid color '{value}' for '{key}'"),
        "write a color as '#RRGGBB', for example '#097FEB'".to_string(),
    ))
}

fn prop_value(ctx: &Ctx, _kind: &str, key: &str, arg: &Arg, info: &KindInfo) -> DResult<Json> {
    if key == "bind" {
        return match &arg.value {
            AstValue::Ident(name) => Ok(json!(name)),
            _ => Err(ctx.err(
                arg.pos,
                format!(
                    "'bind' expects a state name, found {}",
                    value_desc(&arg.value)
                ),
                "write bind: name, where 'name' is a declared state".to_string(),
            )),
        };
    }
    if key == "opacity" {
        let n = match &arg.value {
            AstValue::Lit(Lit::Int(n)) => *n as f64,
            AstValue::Lit(Lit::Float(n)) => *n,
            _ => {
                return Err(ctx.err(
                    arg.pos,
                    format!(
                        "prop 'opacity' expects a number, found {}",
                        value_desc(&arg.value)
                    ),
                    "write a number between 0 and 1, such as opacity: 0.95".to_string(),
                ))
            }
        };
        if !(0.0..=1.0).contains(&n) {
            return Err(ctx.err(
                arg.pos,
                format!("opacity must be between 0 and 1, found {n}"),
                "write a number between 0 and 1, such as opacity: 0.95".to_string(),
            ));
        }
        return Ok(json!(n));
    }
    if key == "shadow" {
        let n = match &arg.value {
            AstValue::Lit(Lit::Int(n)) => *n,
            _ => {
                return Err(ctx.err(
                    arg.pos,
                    format!(
                        "prop 'shadow' expects an integer, found {}",
                        value_desc(&arg.value)
                    ),
                    "write an integer between 0 and 8, such as shadow: 2".to_string(),
                ))
            }
        };
        if !(0..=8).contains(&n) {
            return Err(ctx.err(
                arg.pos,
                format!("shadow must be between 0 and 8, found {n}"),
                "write an integer between 0 and 8, such as shadow: 2".to_string(),
            ));
        }
        return Ok(json!(n));
    }
    if key == "color" || key == "border" {
        let s = match &arg.value {
            AstValue::Lit(Lit::Str(s)) => s.clone(),
            other => {
                return Err(ctx.err(
                    arg.pos,
                    format!(
                        "prop '{key}' expects a color string, found {}",
                        value_desc(other)
                    ),
                    "write a color as '#RRGGBB', for example '#097FEB'".to_string(),
                ))
            }
        };
        check_color(ctx, arg.pos, key, &s)?;
        return Ok(json!(s));
    }
    if key == "gradient" {
        let s = match &arg.value {
            AstValue::Lit(Lit::Str(s)) => s.clone(),
            other => {
                return Err(ctx.err(
                    arg.pos,
                    format!(
                        "prop 'gradient' expects a color string, found {}",
                        value_desc(other)
                    ),
                    "write two colors as '#RRGGBB,#RRGGBB'".to_string(),
                ))
            }
        };
        let stops: Vec<&str> = s.split(',').collect();
        if stops.len() != 2 {
            return Err(ctx.err(
                arg.pos,
                format!("gradient needs exactly two colors, found {}", stops.len()),
                "write two colors as '#097FEB,#101828'".to_string(),
            ));
        }
        for stop in stops {
            check_color(ctx, arg.pos, "gradient", stop.trim())?;
        }
        return Ok(json!(s));
    }
    if key == "glass" {
        return match &arg.value {
            AstValue::Lit(Lit::Bool(b)) => Ok(json!(*b)),
            other => Err(ctx.err(
                arg.pos,
                format!(
                    "prop 'glass' expects true or false, found {}",
                    value_desc(other)
                ),
                "write glass: true or glass: false".to_string(),
            )),
        };
    }
    if key == "animate" {
        let s = match &arg.value {
            AstValue::Lit(Lit::Str(s)) => s.clone(),
            AstValue::Ident(s) => s.clone(),
            other => {
                return Err(ctx.err(
                    arg.pos,
                    format!(
                        "prop 'animate' expects a keyword, found {}",
                        value_desc(other)
                    ),
                    "write animate: spring, animate: ease or animate: none".to_string(),
                ))
            }
        };
        if !ANIMATE_VALUES.contains(&s.as_str()) {
            return Err(ctx.err(
                arg.pos,
                format!("unknown animate value '{s}'"),
                "use one of: spring, ease, none".to_string(),
            ));
        }
        return Ok(json!(s));
    }
    if key == "particles" {
        return particles_value(ctx, arg);
    }
    if info.numeric.contains(&key) || GENERIC_NUMERIC.contains(&key) {
        return match &arg.value {
            AstValue::Lit(Lit::Int(n)) => Ok(json!(*n)),
            _ => Err(ctx.err(
                arg.pos,
                format!(
                    "prop '{}' expects an integer, found {}",
                    key,
                    value_desc(&arg.value)
                ),
                format!("write an integer such as {key}: 24"),
            )),
        };
    }
    Ok(match &arg.value {
        AstValue::Lit(l) => json_lit(l),
        AstValue::Ident(s) => json!(s),
    })
}

/// One accepted `particles` field: name, inclusive bounds and the value used
/// when the source omits it. `count` has no default because the renderer
/// requires it. The bounds mirror the renderer's particle validation, so a
/// rejected value fails at its source position instead of at view time.
const PARTICLE_FIELDS: &[(&str, i64, i64, Option<i64>)] = &[
    ("count", 0, 512, None),
    ("seed", 0, 4_294_967_295, Some(1)),
    ("radius", 0, 32, Some(2)),
    ("alpha", 0, 1024, Some(1024)),
];

/// Lower the `particles` prop: a comma-separated `name=value` field list that
/// becomes the bounded object the renderer reads as `props.particles`. The
/// field set is static so the raster and the GPU backend hash identical bytes
/// on every run of the same document.
fn particles_value(ctx: &Ctx, arg: &Arg) -> DResult<Json> {
    let text = match &arg.value {
        AstValue::Lit(Lit::Str(s)) => s.clone(),
        other => {
            return Err(ctx.err(
                arg.pos,
                format!(
                    "prop 'particles' expects a field list, found {}",
                    value_desc(other)
                ),
                "write fields as 'particles: \"count=64,seed=7\"'".to_string(),
            ))
        }
    };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut values: Vec<(&str, i64)> = Vec::new();
    let mut color: Option<String> = None;
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, raw) = part.split_once('=').ok_or_else(|| {
            ctx.err(
                arg.pos,
                format!("particles field '{part}' must be written name=value"),
                "write fields as 'count=64,seed=7,radius=2,alpha=1024,color=#097FEB'".to_string(),
            )
        })?;
        let name = name.trim();
        let raw = raw.trim();
        if !seen.insert(name) {
            return Err(ctx.err(
                arg.pos,
                format!("duplicate particles field '{name}'"),
                "keep one value per field".to_string(),
            ));
        }
        if name == "color" {
            check_color(ctx, arg.pos, "particles.color", raw)?;
            color = Some(raw.to_string());
            continue;
        }
        let field = PARTICLE_FIELDS
            .iter()
            .find(|f| f.0 == name)
            .ok_or_else(|| {
                ctx.err(
                    arg.pos,
                    format!("unknown particles field '{name}'"),
                    "accepted fields: count, seed, radius, alpha, color".to_string(),
                )
            })?;
        let (lo, hi, default) = (field.1, field.2, field.3);
        let example = default.unwrap_or(64);
        let n = raw.parse::<i64>().map_err(|_| {
            ctx.err(
                arg.pos,
                format!("particles.{name} must be an integer {lo}..={hi}, found '{raw}'"),
                format!("write '{name}={example}'"),
            )
        })?;
        if !(lo..=hi).contains(&n) {
            return Err(ctx.err(
                arg.pos,
                format!("particles.{name} {n} must be {lo}..={hi}"),
                format!("write '{name}={example}'"),
            ));
        }
        values.push((name, n));
    }
    let count = values
        .iter()
        .find(|(name, _)| *name == "count")
        .map(|(_, n)| *n)
        .ok_or_else(|| {
            ctx.err(
                arg.pos,
                "particles requires the 'count' field".to_string(),
                "write 'particles: \"count=64\"' with optional seed, radius, alpha and color"
                    .to_string(),
            )
        })?;
    let mut out = JsonMap::new();
    out.insert("count".to_string(), json!(count));
    for (name, _, _, default) in PARTICLE_FIELDS {
        let Some(default) = default else {
            continue;
        };
        let value = values
            .iter()
            .find(|(field, _)| field == name)
            .map_or(*default, |(_, n)| *n);
        out.insert((*name).to_string(), json!(value));
    }
    if let Some(color) = color {
        out.insert("color".to_string(), json!(color));
    }
    Ok(Json::Object(out))
}

/// Lower one event statement into an IR action. The types are already checked
/// by the shared type checker, so this pass only builds the IR.
fn lower_action(ctx: &Ctx, stmt: &Stmt) -> DResult<Json> {
    match &stmt.kind {
        StmtKind::Add(v, p) | StmtKind::Sub(v, p) => {
            let value = match v {
                AstValue::Lit(Lit::Int(n)) => {
                    if matches!(stmt.kind, StmtKind::Sub(..)) {
                        n.checked_neg().ok_or_else(|| {
                            ctx.err(
                                *p,
                                "integer overflow in '-='",
                                "use a value within the i64 range".to_string(),
                            )
                        })?
                    } else {
                        *n
                    }
                }
                AstValue::Ident(name) => {
                    // A state reference cannot be negated, so '-=' needs its own op.
                    let op = if matches!(stmt.kind, StmtKind::Sub(..)) {
                        "sub"
                    } else {
                        "add"
                    };
                    return Ok(json!({"op": op, "target": stmt.target, "valueFrom": name}));
                }
                other => {
                    return Err(ctx.err(
                        *p,
                        format!(
                            "'+=' / '-=' requires an int literal, found {}",
                            value_desc(other)
                        ),
                        "write an integer such as 'x += 1'".to_string(),
                    ));
                }
            };
            Ok(json!({"op": "add", "target": stmt.target, "value": value}))
        }
        StmtKind::Set(v, _p) => match v {
            AstValue::Lit(l) => {
                Ok(json!({"op": "set", "target": stmt.target, "value": json_lit(l)}))
            }
            AstValue::Ident(name) => {
                Ok(json!({"op": "set", "target": stmt.target, "valueFrom": name}))
            }
        },
        StmtKind::Toggle => Ok(json!({"op": "toggle", "target": stmt.target})),
    }
}

fn lower_actions(ctx: &Ctx, stmts: &[Stmt]) -> DResult<Vec<Json>> {
    let mut actions = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        actions.push(lower_action(ctx, stmt)?);
    }
    Ok(actions)
}

fn lower_effects(ctx: &Ctx, effects: &[Effect]) -> DResult<Vec<Json>> {
    let mut out = Vec::with_capacity(effects.len());
    for effect in effects {
        match effect {
            Effect::Delay { ms, pos } => {
                if !(0..=60000).contains(ms) {
                    return Err(ctx.err(
                        *pos,
                        format!("delay must be between 0 and 60000 ms, found {ms}"),
                        "write a delay from 0 to 60000 milliseconds",
                    ));
                }
                out.push(json!({"kind": "delay", "ms": ms}));
            }
            Effect::Fetch { path, into, .. } => {
                out.push(json!({"kind": "fetch", "path": path, "into": into}));
            }
        }
    }
    Ok(out)
}

fn lower_node(ctx: &mut Ctx, node: &Node, cond: Cond) -> DResult<Json> {
    let id = ctx.take_id();
    let source_kind = match &node.kind {
        NodeKind::Widget(k) => k.as_str(),
        _ => unreachable!("conditional nodes are expanded before lowering"),
    };
    // A catalog name expands to its primitive at compile time; the 32 primitives stay unchanged.
    let component = if KINDS.contains(&source_kind) {
        None
    } else {
        catalog::catalog()?.get(source_kind)
    };
    let kind = component.map_or(source_kind, |c| c.primitive.as_str());
    let info = kind_info(kind).ok_or_else(|| {
        ctx.err(
            node.pos,
            format!("unknown node kind '{source_kind}'"),
            format!(
                "use one of: {}; see 'opuromc components' for catalog names",
                KINDS.join(", ")
            ),
        )
    })?;
    let mut props = JsonMap::new();
    let mut seen = HashSet::new();
    let mut value_pos: Option<Pos> = None;
    for (idx, arg) in node.args.iter().enumerate() {
        let raw_key = match &arg.name {
            Some(k) => k.clone(),
            None => {
                if idx != 0 {
                    return Err(ctx.err(
                        arg.pos,
                        "positional argument must be the first argument".to_string(),
                        "write it as 'prop: value' or use Text(\"...\") / Button(\"...\")"
                            .to_string(),
                    ));
                }
                match info.positional {
                    Some(p) => p.to_string(),
                    None => {
                        return Err(ctx.err(
                            arg.pos,
                            format!("'{source_kind}' does not accept a positional argument"),
                            format!(
                                "write props explicitly, e.g. {}({}: value)",
                                source_kind,
                                info.props.first().copied().unwrap_or("value")
                            ),
                        ));
                    }
                }
            }
        };
        let key = SHORTCUTS
            .iter()
            .find(|(short, _)| *short == raw_key)
            .map(|(_, canonical)| canonical.to_string())
            .unwrap_or_else(|| raw_key.clone());
        if !info.props.contains(&key.as_str()) && !GENERIC_PROPS.contains(&key.as_str()) {
            return Err(ctx.err(
                arg.pos,
                format!("unknown prop '{raw_key}' on {source_kind}"),
                format!(
                    "{source_kind} accepts: {}; remove '{raw_key}'",
                    info.props.join(", ")
                ),
            ));
        }
        if !seen.insert(key.clone()) {
            return Err(ctx.err(
                arg.pos,
                format!("duplicate prop '{key}' on {source_kind}"),
                format!("keep one '{key}' prop per node"),
            ));
        }
        if kind == "Text" && key == "value" {
            value_pos = Some(arg.pos);
        }
        let val = prop_value(ctx, kind, &key, arg, &info)?;
        let val = match val {
            Json::String(s) => Json::String(substitute_loops(ctx, &s)),
            other => other,
        };
        props.insert(key, val);
    }
    // Defaults are compiler-supplied: inject them after explicit validation, explicit wins.
    if let Some(c) = component {
        for (key, value) in &c.props {
            if !props.contains_key(key) {
                props.insert(key.clone(), value.clone());
            }
        }
    }
    for req in info.required {
        if !props.contains_key(*req) {
            return Err(ctx.err(
                node.pos,
                format!("{source_kind} requires the '{req}' prop"),
                format!("write {source_kind}({req}: ...) with every required prop"),
            ));
        }
    }
    if let (Some(Json::String(s)), Some(base)) = (props.get("value"), value_pos) {
        // The interpolation shape is checked here; the referenced states are
        // checked by the shared type checker.
        interp_parts(s).map_err(|(off, msg)| {
            let p = offset_pos(base, s, off);
            ctx.err(
                p,
                msg,
                "close it with '}' or write '{{' for a literal '{'".to_string(),
            )
        })?;
    }

    let mut events = JsonMap::new();
    let mut children = Vec::new();
    if let Some(block) = &node.block {
        if block.is_async && !info.tap {
            return Err(ctx.err(
                node.pos,
                "'async' requires a tap-handler node",
                "put async on a Button, Switch, Checkbox, Radio or Taps block",
            ));
        }
        if info.tap {
            if !block.nodes.is_empty() {
                return Err(ctx.err(
                    block.pos,
                    "a block may not mix children and statements".to_string(),
                    "a Button block is a tap handler: keep statements, put nodes in a container"
                        .to_string(),
                ));
            }
            let actions = lower_actions(ctx, &block.stmts)?;
            let effects = lower_effects(ctx, &block.effects)?;
            let has_actions = !actions.is_empty();
            let has_effects = !effects.is_empty();
            let mut event = if actions.len() == 1 {
                actions.into_iter().next().unwrap()
            } else {
                json!({ "actions": actions })
            };
            if block.is_async {
                let object = event.as_object_mut().expect("lowered event is an object");
                object.insert("async".to_string(), json!(true));
                object.insert("effects".to_string(), Json::Array(effects));
            }
            if block.is_async || has_actions || has_effects {
                events.insert("tap".to_string(), event);
            }
        } else if info.children {
            if let Some(stmt) = block.stmts.first() {
                return Err(ctx.err(
                    stmt.pos,
                    format!("a {source_kind} block cannot contain statements"),
                    "put state changes inside a Button { ... } tap handler".to_string(),
                ));
            }
            if let Some(effect) = block.effects.first() {
                return Err(ctx.err(
                    effect.pos(),
                    format!("a {source_kind} block cannot contain effects"),
                    "put effects inside an async tap-handler block".to_string(),
                ));
            }
            children = lower_node_list(ctx, &block.nodes, cond)?;
        } else {
            return Err(ctx.err(
                block.pos,
                format!("'{source_kind}' does not support children or tap handlers"),
                "remove the trailing block; only container kinds and Button accept blocks"
                    .to_string(),
            ));
        }
    }
    match kind {
        "Input" => {
            if let Some(Json::String(name)) = props.get("bind") {
                events.insert("input".to_string(), json!({"op": "set", "target": name}));
            }
        }
        "Switch" | "Checkbox" => {
            if let Some(Json::String(name)) = props.get("bind") {
                events
                    .entry("tap".to_string())
                    .or_insert(json!({"op": "toggle", "target": name}));
            }
        }
        "Slider" => {
            if let Some(Json::String(name)) = props.get("bind") {
                events
                    .entry("input".to_string())
                    .or_insert(json!({"op": "set", "target": name}));
            }
        }
        "Radio" => {
            if let (Some(Json::String(name)), Some(value)) = (props.get("bind"), props.get("value"))
            {
                events.entry("tap".to_string()).or_insert(json!({
                    "op": "set",
                    "target": name,
                    "value": value,
                }));
            }
        }
        "Tabs" => {
            if let (Some(Json::String(name)), Some(Json::String(labels))) =
                (props.get("bind"), props.get("labels"))
            {
                let first = labels.split(',').next().unwrap_or(labels.as_str()).trim();
                events.entry("tap".to_string()).or_insert(json!({
                    "op": "set",
                    "target": name,
                    "value": first,
                }));
            }
        }
        _ => {}
    }
    let mut node_json = JsonMap::new();
    node_json.insert("id".to_string(), json!(id));
    node_json.insert("kind".to_string(), json!(kind));
    if let Some(c) = component {
        node_json.insert("component".to_string(), json!(c.name));
    }
    node_json.insert("props".to_string(), Json::Object(props));
    node_json.insert("children".to_string(), Json::Array(children));
    node_json.insert("events".to_string(), Json::Object(events));
    if let Some(w) = cond.when {
        node_json.insert("when".to_string(), json!(w));
    }
    if let Some(u) = cond.unless {
        node_json.insert("unless".to_string(), json!(u));
    }
    Ok(Json::Object(node_json))
}

/// Number of tree nodes produced by a node list after `if`/`for` expansion.
fn count_expanded(nodes: &[Node], ctx: &Ctx) -> DResult<usize> {
    let mut total = 0usize;
    for n in nodes {
        let count = match &n.kind {
            NodeKind::Widget(_) => 1,
            NodeKind::If { else_block, .. } => {
                let block = n.block.as_ref().ok_or_else(|| {
                    ctx.err(n.pos, "'if' needs a block", "write 'if (dark) { ... }'")
                })?;
                let mut count = count_expanded(&block.nodes, ctx)?;
                if let Some(else_block) = else_block {
                    count = count.saturating_add(count_expanded(&else_block.nodes, ctx)?);
                }
                count
            }
            NodeKind::For { iter, .. } => {
                let block = n.block.as_ref().ok_or_else(|| {
                    ctx.err(n.pos, "'for' needs a block", "write 'for (i in 3) { ... }'")
                })?;
                let repeats = match iter {
                    ForIter::Count { count, pos } => {
                        if *count > 64 {
                            return Err(ctx.err(
                                *pos,
                                format!("'for' count must be at most 64, found {count}"),
                                "unroll at most 64 copies; split the work into two blocks",
                            ));
                        }
                        *count
                    }
                    ForIter::List { name, pos } => {
                        match (ctx.types.get(name), ctx.lists.get(name)) {
                            (Some(&Ty::List), Some(values)) => {
                                if values.len() > 64 {
                                    return Err(ctx.err(
                                        *pos,
                                        format!(
                                            "list '{name}' has more than 64 values, found {}",
                                            values.len()
                                        ),
                                        "split the list or use an integer count loop",
                                    ));
                                }
                                values.len()
                            }
                            (Some(ty), _) => {
                                return Err(ctx.err(
                                    *pos,
                                    format!(
                                        "'for' requires a list state, found {} state '{name}'",
                                        ty.name()
                                    ),
                                    "declare the state with a string list or use an integer count",
                                ));
                            }
                            (None, _) => {
                                return Err(ctx.err(
                                    *pos,
                                    format!("unknown list state '{name}' in 'for'"),
                                    format!(
                                        "declare it with 'state {name} = [\"a\", \"b\"]' or use one of: {}",
                                        ctx.state_list()
                                    ),
                                ));
                            }
                        }
                    }
                };
                repeats.saturating_mul(count_expanded(&block.nodes, ctx)?)
            }
        };
        total = total.saturating_add(count);
    }
    Ok(total)
}

/// Expand `if`/`else`/`for` blocks and lower every resulting node.
fn lower_node_list(ctx: &mut Ctx, nodes: &[Node], cond: Cond) -> DResult<Vec<Json>> {
    let mut out = Vec::new();
    for node in nodes {
        match &node.kind {
            NodeKind::If {
                cond: name,
                cond_pos,
                else_block,
            } => {
                let _ = cond_pos;
                let block = node.block.as_ref().ok_or_else(|| {
                    ctx.err(node.pos, "'if' needs a block", "write 'if (dark) { ... }'")
                })?;
                if let Some(stmt) = block.stmts.first() {
                    return Err(ctx.err(
                        stmt.pos,
                        "an 'if' block cannot contain statements".to_string(),
                        "put state changes inside a Button { ... } tap handler".to_string(),
                    ));
                }
                if let Some(effect) = block.effects.first() {
                    return Err(ctx.err(
                        effect.pos(),
                        "an 'if' block cannot contain effects".to_string(),
                        "put effects inside an async tap-handler block".to_string(),
                    ));
                }
                if let Some(else_block) = else_block {
                    if let Some(stmt) = else_block.stmts.first() {
                        return Err(ctx.err(
                            stmt.pos,
                            "an 'else' block cannot contain statements".to_string(),
                            "put state changes inside a Button { ... } tap handler".to_string(),
                        ));
                    }
                    if let Some(effect) = else_block.effects.first() {
                        return Err(ctx.err(
                            effect.pos(),
                            "an 'else' block cannot contain effects".to_string(),
                            "put effects inside an async tap-handler block".to_string(),
                        ));
                    }
                    if cond.unless.is_some() {
                        return Err(ctx.err(
                            node.pos,
                            "nested 'else' inside an 'else' block is not supported yet",
                            "flatten the conditional or move it outside the else block",
                        ));
                    }
                }
                let branch = Cond {
                    when: Some(name),
                    unless: cond.unless,
                };
                out.extend(lower_node_list(ctx, &block.nodes, branch)?);
                if let Some(else_block) = else_block {
                    let branch = Cond {
                        when: cond.when,
                        unless: Some(name),
                    };
                    out.extend(lower_node_list(ctx, &else_block.nodes, branch)?);
                }
            }
            NodeKind::For { var, iter } => {
                let block = node.block.as_ref().ok_or_else(|| {
                    ctx.err(
                        node.pos,
                        "'for' needs a block",
                        "write 'for (i in 3) { ... }'",
                    )
                })?;
                if let Some(stmt) = block.stmts.first() {
                    return Err(ctx.err(
                        stmt.pos,
                        "a 'for' block cannot contain statements".to_string(),
                        "put state changes inside a Button { ... } tap handler".to_string(),
                    ));
                }
                if let Some(effect) = block.effects.first() {
                    return Err(ctx.err(
                        effect.pos(),
                        "a 'for' block cannot contain effects".to_string(),
                        "put effects inside an async tap-handler block".to_string(),
                    ));
                }
                let values: Vec<LoopValue> = match iter {
                    ForIter::Count { count, pos } => {
                        if *count > 64 {
                            return Err(ctx.err(
                                *pos,
                                format!("'for' count must be at most 64, found {count}"),
                                "unroll at most 64 copies; split the work into two blocks"
                                    .to_string(),
                            ));
                        }
                        (0..*count).map(|i| LoopValue::Int(i as i64)).collect()
                    }
                    ForIter::List { name, pos } => {
                        let ty = ctx.types.get(name).copied().ok_or_else(|| {
                            ctx.err(
                                *pos,
                                format!("unknown list state '{name}' in 'for'"),
                                format!(
                                    "declare it with 'state {name} = [\"a\", \"b\"]' or use one of: {}",
                                    ctx.state_list()
                                ),
                            )
                        })?;
                        if ty != Ty::List {
                            return Err(ctx.err(
                                *pos,
                                format!(
                                    "'for' requires a list state, found {} state '{name}'",
                                    ty.name()
                                ),
                                "declare the state with a string list or use an integer count"
                                    .to_string(),
                            ));
                        }
                        let values = ctx.lists.get(name).cloned().ok_or_else(|| {
                            ctx.err(
                                *pos,
                                format!("list state '{name}' has no initializer"),
                                "declare it with 'state items = [\"a\", \"b\"]'",
                            )
                        })?;
                        if values.len() > 64 {
                            return Err(ctx.err(
                                *pos,
                                format!(
                                    "list '{name}' has more than 64 values, found {}",
                                    values.len()
                                ),
                                "split the list or use an integer count loop".to_string(),
                            ));
                        }
                        values.into_iter().map(LoopValue::Str).collect()
                    }
                };
                for value in values {
                    ctx.loops.push((var.clone(), value));
                    let lowered = lower_node_list(ctx, &block.nodes, cond);
                    ctx.loops.pop();
                    out.extend(lowered?);
                }
            }
            NodeKind::Widget(_) => out.push(lower_node(ctx, node, cond)?),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------- components ------

fn param_ty(name: &str) -> Ty {
    match name {
        "int" => Ty::Int,
        "bool" => Ty::Bool,
        "string" => Ty::Str,
        _ => unreachable!("the parser validates parameter types"),
    }
}

fn component_lit(value: &AstValue, pos: Pos, path: &str, param: &str) -> DResult<Lit> {
    match value {
        AstValue::Lit(l @ (Lit::Int(_) | Lit::Bool(_) | Lit::Str(_))) => Ok(l.clone()),
        other => Err(Diagnostic::new(
            path,
            pos.line,
            pos.col,
            format!(
                "component argument for '{param}' must be an int, bool or string literal, found {}",
                value_desc(other)
            ),
            "pass a literal such as Card(\"Hi\"), Card(3) or Card(true)",
        )),
    }
}

fn component_lit_ty(l: &Lit) -> Ty {
    match l {
        Lit::Int(_) => Ty::Int,
        Lit::Bool(_) => Ty::Bool,
        Lit::Str(_) => Ty::Str,
        _ => unreachable!("component arguments are int, bool or string literals"),
    }
}

fn component_lit_text(l: &Lit) -> String {
    match l {
        Lit::Int(n) => n.to_string(),
        Lit::Bool(b) => b.to_string(),
        Lit::Str(s) => s.clone(),
        _ => unreachable!("component arguments are int, bool or string literals"),
    }
}

/// Substitute `{param}` inside string props; unknown braces stay for loop/state lowering.
fn substitute_params(s: &str, bindings: &HashMap<String, Lit>) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < cs.len() {
        if cs[i] == '{' && i + 1 < cs.len() && cs[i + 1] == '{' {
            out.push_str("{{");
            i += 2;
            continue;
        }
        if cs[i] == '}' && i + 1 < cs.len() && cs[i + 1] == '}' {
            out.push_str("}}");
            i += 2;
            continue;
        }
        if cs[i] == '{' {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != '}' && cs[j] != '{' {
                j += 1;
            }
            if j < cs.len() && cs[j] == '}' {
                let name: String = cs[i + 1..j].iter().collect();
                if let Some(v) = bindings.get(&name) {
                    out.push_str(&component_lit_text(v));
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

fn substitute_arg(arg: &Arg, bindings: &HashMap<String, Lit>) -> Arg {
    Arg {
        value: substitute_value(&arg.value, bindings),
        ..arg.clone()
    }
}

fn substitute_value(value: &AstValue, bindings: &HashMap<String, Lit>) -> AstValue {
    match value {
        AstValue::Ident(name) => match bindings.get(name) {
            Some(lit) => AstValue::Lit(lit.clone()),
            None => value.clone(),
        },
        AstValue::Lit(Lit::Str(s)) => AstValue::Lit(Lit::Str(substitute_params(s, bindings))),
        _ => value.clone(),
    }
}

/// Statements keep their state targets; only the right-hand side takes parameter literals.
fn substitute_stmt(path: &str, stmt: &Stmt, bindings: &HashMap<String, Lit>) -> DResult<Stmt> {
    if bindings.is_empty() {
        return Ok(stmt.clone());
    }
    if bindings.contains_key(&stmt.target) {
        return Err(Diagnostic::new(
            path,
            stmt.pos.line,
            stmt.pos.col,
            format!(
                "component parameter '{}' is read-only in event statements",
                stmt.target
            ),
            "write to a declared state; parameters only supply node props",
        ));
    }
    let kind = match &stmt.kind {
        StmtKind::Add(v, p) => StmtKind::Add(substitute_value(v, bindings), *p),
        StmtKind::Sub(v, p) => StmtKind::Sub(substitute_value(v, bindings), *p),
        StmtKind::Set(v, p) => StmtKind::Set(substitute_value(v, bindings), *p),
        StmtKind::Toggle => StmtKind::Toggle,
    };
    Ok(Stmt {
        target: stmt.target.clone(),
        pos: stmt.pos,
        kind,
    })
}

fn component_uses(nodes: &[Node], out: &mut Vec<(String, Pos)>) {
    for node in nodes {
        if let NodeKind::Widget(name) = &node.kind {
            out.push((name.clone(), node.pos));
        }
        if let Some(block) = &node.block {
            component_uses(&block.nodes, out);
        }
        if let NodeKind::If {
            else_block: Some(else_block),
            ..
        } = &node.kind
        {
            component_uses(&else_block.nodes, out);
        }
    }
}

fn find_cycle(
    name: &str,
    graph: &HashMap<String, Vec<String>>,
    stack: &mut Vec<String>,
    done: &mut HashSet<String>,
) -> Option<String> {
    if done.contains(name) {
        return None;
    }
    if let Some(start) = stack.iter().position(|s| s == name) {
        let mut chain: Vec<String> = stack[start..].to_vec();
        chain.push(name.to_string());
        return Some(chain.join(" -> "));
    }
    stack.push(name.to_string());
    for next in graph.get(name).into_iter().flatten() {
        if let Some(chain) = find_cycle(next, graph, stack, done) {
            return Some(chain);
        }
    }
    stack.pop();
    done.insert(name.to_string());
    None
}

/// Validate every declared body, including unused components, before expansion.
fn check_components(path: &str, components: &[ComponentDecl]) -> DResult<()> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for decl in components {
        let mut uses = Vec::new();
        component_uses(&decl.body, &mut uses);
        let mut edges = Vec::new();
        for (name, pos) in uses {
            if !components.iter().any(|c| c.name == name)
                && !KINDS.contains(&name.as_str())
                && catalog::catalog()?.get(&name).is_none()
            {
                return Err(Diagnostic::new(
                    path,
                    pos.line,
                    pos.col,
                    format!("unknown component '{name}' in component '{}'", decl.name),
                    format!(
                        "declare it with 'component {name}(...) {{ ... }}' or use a built-in kind"
                    ),
                ));
            }
            edges.push(name);
        }
        graph.insert(decl.name.clone(), edges);
    }
    let mut done = HashSet::new();
    for decl in components {
        let mut stack = Vec::new();
        if let Some(chain) = find_cycle(&decl.name, &graph, &mut stack, &mut done) {
            return Err(Diagnostic::new(
                path,
                decl.pos.line,
                decl.pos.col,
                format!("component cycle detected: {chain}"),
                "remove the component use that closes the cycle; components expand at compile time",
            ));
        }
    }
    Ok(())
}

/// Replace component use sites with ordinary nodes; `types` infers untyped parameters
/// from their first use. Nothing about components survives into the IR.
fn expand_nodes(
    path: &str,
    nodes: &[Node],
    comps: &HashMap<&str, &ComponentDecl>,
    types: &mut typecheck::ComponentTypes,
    stack: &mut Vec<String>,
    bindings: &HashMap<String, Lit>,
) -> DResult<Vec<Node>> {
    let mut out = Vec::new();
    for node in nodes {
        let args: Vec<Arg> = node
            .args
            .iter()
            .map(|a| substitute_arg(a, bindings))
            .collect();
        if let NodeKind::Widget(name) = &node.kind {
            if let Some(decl) = comps.get(name.as_str()).copied() {
                if args.len() != decl.params.len() {
                    let plural = if decl.params.len() == 1 {
                        "argument"
                    } else {
                        "arguments"
                    };
                    return Err(Diagnostic::new(
                        path,
                        node.pos.line,
                        node.pos.col,
                        format!(
                            "component '{name}' expects {} {plural}, found {}",
                            decl.params.len(),
                            args.len()
                        ),
                        "pass one literal per declared parameter",
                    ));
                }
                for arg in &args {
                    if arg.name.is_some() {
                        return Err(Diagnostic::new(
                            path,
                            arg.pos.line,
                            arg.pos.col,
                            format!("component '{name}' arguments must be positional"),
                            "write 'Card(\"Hi\")' instead of 'Card(title: \"Hi\")'",
                        ));
                    }
                }
                let mut inner = HashMap::new();
                {
                    for (idx, (arg, param)) in args.iter().zip(&decl.params).enumerate() {
                        let lit = component_lit(&arg.value, arg.pos, path, &param.name)?;
                        let got = component_lit_ty(&lit);
                        typecheck::check_component_param(
                            types,
                            name,
                            idx,
                            &param.name,
                            got,
                            path,
                            arg.pos,
                        )?;
                        inner.insert(param.name.clone(), lit);
                    }
                }
                if let Some(start) = stack.iter().position(|s| s == name) {
                    let mut chain: Vec<&str> = stack[start..].iter().map(String::as_str).collect();
                    chain.push(name.as_str());
                    return Err(Diagnostic::new(
                        path,
                        node.pos.line,
                        node.pos.col,
                        format!("component cycle detected: {}", chain.join(" -> ")),
                        "remove the component use that closes the cycle; components expand at compile time",
                    ));
                }
                stack.push(name.clone());
                let expanded = expand_nodes(path, &decl.body, comps, types, stack, &inner);
                stack.pop();
                out.extend(expanded?);
                continue;
            }
            if !comps.is_empty()
                && !KINDS.contains(&name.as_str())
                && catalog::catalog()?.get(name).is_none()
            {
                return Err(Diagnostic::new(
                    path,
                    node.pos.line,
                    node.pos.col,
                    format!("unknown component '{name}'"),
                    format!(
                        "declare it with 'component {name}(...) {{ ... }}' or use a built-in kind"
                    ),
                ));
            }
        }
        // Parameters supply literals only: conditions and loop iterables read state.
        if let NodeKind::If { cond, cond_pos, .. } = &node.kind {
            if let Some(p) = bindings.get(cond) {
                let text = component_lit_text(p);
                return Err(Diagnostic::new(
                    path,
                    cond_pos.line,
                    cond_pos.col,
                    format!("component parameter '{cond}' cannot be used as an 'if' condition"),
                    format!(
                        "read a bool state instead; the parameter expands to the literal '{text}'"
                    ),
                ));
            }
        }
        if let NodeKind::For {
            iter: ForIter::List { name, pos },
            ..
        } = &node.kind
        {
            if bindings.contains_key(name) {
                return Err(Diagnostic::new(
                    path,
                    pos.line,
                    pos.col,
                    format!("component parameter '{name}' cannot be used as a 'for' iterable"),
                    "loop over a declared list state; parameters are int, bool or string",
                ));
            }
        }
        let mut node = Node {
            args,
            ..node.clone()
        };
        if let Some(block) = node.block.as_mut() {
            let inner_stmts = std::mem::take(&mut block.stmts);
            let mut stmts = Vec::with_capacity(inner_stmts.len());
            for stmt in &inner_stmts {
                stmts.push(substitute_stmt(path, stmt, bindings)?);
            }
            block.stmts = stmts;
            let inner = std::mem::take(&mut block.nodes);
            block.nodes = expand_nodes(path, &inner, comps, types, stack, bindings)?;
        }
        if let NodeKind::If {
            else_block: Some(else_block),
            ..
        } = &mut node.kind
        {
            let inner = std::mem::take(&mut else_block.nodes);
            else_block.nodes = expand_nodes(path, &inner, comps, types, stack, bindings)?;
        }
        out.push(node);
    }
    Ok(out)
}

// ---------------------------------------------------------- full-stack ------

fn model_types(program: &Program) -> Vec<String> {
    program.models.iter().map(|m| m.name.clone()).collect()
}

fn lower_models(program: &Program, ctx: &Ctx) -> DResult<Vec<Json>> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for model in &program.models {
        if !seen.insert(model.name.clone()) {
            return Err(ctx.err(
                model.pos,
                format!("duplicate model '{}'", model.name),
                format!(
                    "declare every model once; declared models are: {}",
                    model_types(program).join(", ")
                ),
            ));
        }
        let mut primaries = 0usize;
        let mut fields = Vec::new();
        let mut field_names: HashSet<String> = HashSet::new();
        for field in &model.fields {
            if !field_names.insert(field.name.clone()) {
                return Err(ctx.err(
                    field.pos,
                    format!("duplicate field '{}' in model '{}'", field.name, model.name),
                    "declare every field once per model".to_string(),
                ));
            }
            let ty = match field.ty.as_str() {
                "int" | "string" | "bool" => field.ty.as_str(),
                other => {
                    return Err(ctx.err(
                        field.pos,
                        format!("unknown field type '{other}' in model '{}'", model.name),
                        "field types are int, string or bool".to_string(),
                    ))
                }
            };
            if field.primary {
                primaries += 1;
                if primaries > 1 {
                    return Err(ctx.err(
                        field.pos,
                        format!("model '{}' has more than one primary field", model.name),
                        "mark at most one field as 'primary'".to_string(),
                    ));
                }
            }
            if let Some(default) = &field.default {
                let given = match default {
                    Lit::Int(_) => "int",
                    Lit::Float(_) => "number",
                    Lit::Bool(_) => "bool",
                    Lit::Str(_) => "string",
                    Lit::List(_) => "list",
                };
                if given != ty {
                    return Err(ctx.err(
                        field.pos,
                        format!(
                            "default value of field '{}' must be {ty}, found {given}",
                            field.name
                        ),
                        format!("write a {ty} default or drop '= {given}'"),
                    ));
                }
            }
            fields.push(json!({
                "name": field.name,
                "type": ty,
                "primary": field.primary,
                "default": match &field.default {
                    Some(d) => json_lit(d),
                    None => Json::Null,
                },
            }));
        }
        out.push(json!({"name": model.name, "fields": fields}));
    }
    Ok(out)
}

fn lower_apis(program: &Program, ctx: &Ctx) -> DResult<Vec<Json>> {
    let mut out = Vec::new();
    let models = model_types(program);
    for api in &program.apis {
        if api.handlers.is_empty() {
            return Err(ctx.err(
                api.pos,
                format!("api '{}' has no handler", api.path),
                "write at least one handler such as 'list Todo'".to_string(),
            ));
        }
        let mut model = String::new();
        let mut verbs = Vec::new();
        for handler in &api.handlers {
            if !HANDLER_VERBS.contains(&handler.verb.as_str()) {
                return Err(ctx.err(
                    handler.pos,
                    format!("unknown handler verb '{}'", handler.verb),
                    format!("use one of: {}", HANDLER_VERBS.join(", ")),
                ));
            }
            if !models.contains(&handler.model) {
                return Err(ctx.err(
                    handler.pos,
                    format!("unknown model '{}' in api '{}'", handler.model, api.path),
                    format!(
                        "declare it with 'model {} {{ ... }}' or use one of: {}",
                        handler.model,
                        if models.is_empty() {
                            "(no models declared)".to_string()
                        } else {
                            models.join(", ")
                        }
                    ),
                ));
            }
            if model.is_empty() {
                model = handler.model.clone();
            } else if model != handler.model {
                return Err(ctx.err(
                    handler.pos,
                    format!(
                        "api '{}' mixes models '{}' and '{}'",
                        api.path, model, handler.model
                    ),
                    "one api block serves one model; declare a second api block for the other model"
                        .to_string(),
                ));
            }
            if matches!(handler.verb.as_str(), "get" | "update" | "delete")
                && !api.path.contains(':')
            {
                return Err(ctx.err(
                    handler.pos,
                    format!(
                        "api '{}' needs a path parameter for '{}'",
                        api.path, handler.verb
                    ),
                    "write a path such as '/todos/:id'".to_string(),
                ));
            }
            verbs.push(handler.verb.clone());
        }
        out.push(json!({"path": api.path, "model": model, "handlers": verbs}));
    }
    Ok(out)
}

fn lower(program: &Program, path: &str) -> DResult<Json> {
    let mut ctx = Ctx {
        path,
        types: HashMap::new(),
        order: Vec::new(),
        next_id: 0,
        computed: HashSet::new(),
        inits: HashMap::new(),
        lists: HashMap::new(),
        loops: Vec::new(),
    };
    let mut state_json = Vec::new();
    let (state_types, state_inits) = typecheck::infer_states(program, path)?;
    for s in &program.states {
        let ty = state_types[&s.name];
        let init = state_inits[&s.name].clone();
        if let Lit::List(values) = &s.init {
            ctx.lists.insert(s.name.clone(), values.clone());
        }
        ctx.types.insert(s.name.clone(), ty);
        ctx.order.push(s.name.clone());
        ctx.inits.insert(s.name.clone(), init.clone());
        state_json.push(json!({"name": s.name, "type": ty.name(), "init": init}));
    }
    let computed_names: HashSet<String> = program.computed.iter().map(|c| c.name.clone()).collect();
    let model_names = model_types(program);
    let mut computed_seen = HashSet::new();
    let mut computed_json = Vec::new();
    for c in &program.computed {
        if ctx.types.contains_key(&c.name)
            || model_names.contains(&c.name)
            || !computed_seen.insert(c.name.clone())
        {
            return Err(ctx.err(
                c.pos,
                format!("duplicate name '{}'", c.name),
                "rename the computed value or the conflicting declaration",
            ));
        }
        let env = typecheck::Env {
            types: ctx.types.clone(),
            inits: ctx.inits.clone(),
            computed: ctx.computed.clone(),
            order: ctx.order.clone(),
        };
        let (ty, init) = typecheck::infer_computed(&c.expr, &env, &computed_names, path)?;
        computed_json.push(json!({
            "name": c.name,
            "type": ty.name(),
            "expr": c.expr.text(),
        }));
        state_json.push(json!({"name": c.name, "type": ty.name(), "init": init.clone()}));
        ctx.types.insert(c.name.clone(), ty);
        ctx.order.push(c.name.clone());
        ctx.inits.insert(c.name.clone(), init);
        ctx.computed.insert(c.name.clone());
    }
    let models = lower_models(program, &ctx)?;
    let apis = lower_apis(program, &ctx)?;
    let mut comps: HashMap<&str, &ComponentDecl> = HashMap::new();
    let mut component_types: typecheck::ComponentTypes = HashMap::new();
    for decl in &program.components {
        comps.insert(decl.name.as_str(), decl);
        component_types.insert(
            decl.name.clone(),
            decl.params
                .iter()
                .map(|p| match p.ty.as_deref() {
                    Some(name) => typecheck::ParamTy::Declared(param_ty(name)),
                    None => typecheck::ParamTy::Unknown,
                })
                .collect(),
        );
    }
    check_components(path, &program.components)?;
    let expanded = expand_nodes(
        path,
        &program.nodes,
        &comps,
        &mut component_types,
        &mut Vec::new(),
        &HashMap::new(),
    )?;
    let env = typecheck::Env {
        types: ctx.types.clone(),
        inits: ctx.inits.clone(),
        computed: ctx.computed.clone(),
        order: ctx.order.clone(),
    };
    let conditions = typecheck::check_nodes(&expanded, env.clone(), path)?;
    let total = count_expanded(&expanded, &ctx)?;
    if total == 0 {
        return Err(Diagnostic::new(
            path,
            1,
            1,
            format!("app '{}' has no UI node", program.app),
            "add a root node such as Column(gap: 8) { Text(\"hi\") }",
        ));
    }
    let tree = if total == 1 {
        let mut nodes = lower_node_list(&mut ctx, &expanded, Cond::default())?;
        nodes.remove(0)
    } else {
        let root_id = ctx.take_id();
        let nodes = lower_node_list(&mut ctx, &expanded, Cond::default())?;
        json!({
            "id": root_id,
            "kind": "Column",
            "props": {},
            "children": nodes,
            "events": {},
        })
    };
    let theme = match &program.theme {
        Some(t) => {
            let mode = t.mode.clone().unwrap_or_else(|| "system".to_string());
            let accent = t.accent.clone().unwrap_or_else(|| "#097FEB".to_string());
            check_color(&ctx, t.pos, "accent", &accent)?;
            let radius = t.radius.unwrap_or(12);
            if radius < 0 {
                return Err(ctx.err(
                    t.pos,
                    format!("theme radius must not be negative, found {radius}"),
                    "write a radius such as 'radius: 8'".to_string(),
                ));
            }
            let mut theme = json!({"mode": mode, "accent": accent, "radius": radius});
            if let Some(fs) = t.font_scale {
                theme["fontScale"] = json!(fs);
            }
            theme
        }
        None => json!({"mode": "system", "accent": "#097FEB", "radius": 12}),
    };
    let mut out = JsonMap::new();
    out.insert("ir".to_string(), json!("0.1"));
    out.insert("app".to_string(), json!(program.app));
    out.insert("state".to_string(), Json::Array(state_json));
    out.insert("tree".to_string(), tree.clone());
    out.insert("theme".to_string(), theme);
    if !models.is_empty() {
        out.insert("models".to_string(), Json::Array(models));
    }
    if !apis.is_empty() {
        out.insert("api".to_string(), Json::Array(apis));
    }
    if !program.imports.is_empty() {
        let imports = program
            .imports
            .iter()
            .map(|i| json!(i.path))
            .collect::<Vec<Json>>();
        out.insert("imports".to_string(), Json::Array(imports));
    }
    if !computed_json.is_empty() {
        out.insert("computed".to_string(), Json::Array(computed_json));
    }
    typecheck::check_ir_conditions(&tree, env, &conditions, path)?;
    Ok(Json::Object(out))
}
