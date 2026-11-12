use std::collections::{HashMap, HashSet};

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::diag::{Diagnostic, DResult};
use crate::lexer::lex;
use crate::parser::{self, Arg, Lit, Node, Pos, Program, Stmt, StmtKind, Value as AstValue};

pub const KINDS: &[&str] = &[
    "Column", "Row", "Container", "Text", "Button", "Input", "Image", "Spacer",
];

const NUMERIC: &[&str] = &["gap", "padding", "width", "height", "radius", "size"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Bool,
    Str,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Bool => "bool",
            Ty::Str => "string",
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

/// Split a v0.1 string into literal parts and `{state}` references. `{{` is a literal `{`.
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
    let toks = lex(src, path)?;
    let program = parser::parse(toks, path)?;
    lower(&program, path)
}

struct KindInfo {
    props: &'static [&'static str],
    required: Option<&'static str>,
    positional: Option<&'static str>,
    children: bool,
    tap: bool,
}

fn kind_info(kind: &str) -> Option<KindInfo> {
    let info = match kind {
        "Column" | "Row" => KindInfo {
            props: &["gap", "padding", "align", "width"],
            required: None,
            positional: None,
            children: true,
            tap: false,
        },
        "Container" => KindInfo {
            props: &["padding", "width", "height", "radius", "color"],
            required: None,
            positional: None,
            children: true,
            tap: false,
        },
        "Text" => KindInfo {
            props: &["value", "size", "color"],
            required: Some("value"),
            positional: Some("value"),
            children: false,
            tap: false,
        },
        "Button" => KindInfo {
            props: &["label"],
            required: Some("label"),
            positional: Some("label"),
            children: false,
            tap: true,
        },
        "Input" => KindInfo {
            props: &["placeholder", "bind"],
            required: None,
            positional: None,
            children: false,
            tap: false,
        },
        "Image" => KindInfo {
            props: &["src", "width", "height"],
            required: Some("src"),
            positional: None,
            children: false,
            tap: false,
        },
        "Spacer" => KindInfo {
            props: &["width", "height"],
            required: None,
            positional: None,
            children: false,
            tap: false,
        },
        _ => return None,
    };
    Some(info)
}

struct Ctx<'a> {
    path: &'a str,
    types: HashMap<String, Ty>,
    order: Vec<String>,
    next_id: u64,
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
}

fn json_lit(l: &Lit) -> Json {
    match l {
        Lit::Int(n) => json!(*n),
        Lit::Bool(b) => json!(*b),
        Lit::Str(s) => json!(s),
    }
}

fn value_desc(v: &AstValue) -> &'static str {
    match v {
        AstValue::Lit(Lit::Int(_)) => "int",
        AstValue::Lit(Lit::Bool(_)) => "bool",
        AstValue::Lit(Lit::Str(_)) => "string",
        AstValue::Ident(_) => "identifier",
    }
}

fn prop_value(ctx: &Ctx<'_>, key: &str, arg: &Arg, info: &KindInfo) -> DResult<Json> {
    if NUMERIC.contains(&key) && info.props.contains(&key) {
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
    if key == "bind" {
        return match &arg.value {
            AstValue::Ident(name) => {
                let ty = ctx.types.get(name).copied().ok_or_else(|| {
                    ctx.err(
                        arg.pos,
                        format!("unknown state '{name}' in 'bind'"),
                        format!(
                            "declare it with 'state {name} = \"\"' or bind one of: {}",
                            ctx.state_list()
                        ),
                    )
                })?;
                if ty != Ty::Str {
                    return Err(ctx.err(
                        arg.pos,
                        format!(
                            "'bind' requires a string state, found {} state '{name}'",
                            ty.name()
                        ),
                        "declare the state with a string initializer or remove 'bind'".to_string(),
                    ));
                }
                Ok(json!(name))
            }
            _ => Err(ctx.err(
                arg.pos,
                format!("'bind' expects a state name, found {}", value_desc(&arg.value)),
                "write bind: name, where 'name' is a declared string state".to_string(),
            )),
        };
    }
    Ok(match &arg.value {
        AstValue::Lit(l) => json_lit(l),
        AstValue::Ident(s) => json!(s),
    })
}

fn lower_action(ctx: &Ctx<'_>, stmt: &Stmt) -> DResult<Json> {
    let ty = ctx.types.get(&stmt.target).copied().ok_or_else(|| {
        ctx.err(
            stmt.pos,
            format!("unknown state '{}' in event statement", stmt.target),
            format!(
                "declare it with 'state {} = 0' or use one of: {}",
                stmt.target,
                ctx.state_list()
            ),
        )
    })?;
    match &stmt.kind {
        StmtKind::Add(v, p) | StmtKind::Sub(v, p) => {
            if ty != Ty::Int {
                return Err(ctx.err(
                    *p,
                    format!(
                        "'+=' / '-=' requires an int state, found {} state '{}'",
                        ty.name(),
                        stmt.target
                    ),
                    format!(
                        "use '=' for a {} state or declare '{}' as int",
                        ty.name(),
                        stmt.target
                    ),
                ));
            }
            let n = match v {
                AstValue::Lit(Lit::Int(n)) => *n,
                AstValue::Ident(name) => {
                    if !ctx.types.contains_key(name) {
                        return Err(ctx.err(
                            *p,
                            format!("unknown state '{name}' in event expression"),
                            format!("declare it with 'state {name} = 0' or use one of: {}", ctx.state_list()),
                        ));
                    }
                    return Err(ctx.err(
                        *p,
                        format!("reading state '{name}' in an event expression is not yet supported in v0.1"),
                        "use a literal here; state-to-state expressions are planned for a later round"
                            .to_string(),
                    ));
                }
                _ => {
                    return Err(ctx.err(
                        *p,
                        format!("'+=' / '-=' requires an int literal, found {}", value_desc(v)),
                        "write an integer such as 'x += 1'".to_string(),
                    ));
                }
            };
            let value = if matches!(stmt.kind, StmtKind::Sub(..)) {
                n.checked_neg().ok_or_else(|| {
                    ctx.err(
                        *p,
                        "integer overflow in '-='",
                        "use a value within the i64 range".to_string(),
                    )
                })?
            } else {
                n
            };
            Ok(json!({"op": "add", "target": stmt.target, "value": value}))
        }
        StmtKind::Set(v, p) => {
            let lit = match v {
                AstValue::Lit(l) => l,
                AstValue::Ident(name) => {
                    if !ctx.types.contains_key(name) {
                        return Err(ctx.err(
                            *p,
                            format!("unknown state '{name}' in event expression"),
                            format!("declare it with 'state {name} = 0' or use one of: {}", ctx.state_list()),
                        ));
                    }
                    return Err(ctx.err(
                        *p,
                        format!("reading state '{name}' in an event expression is not yet supported in v0.1"),
                        "use a literal here; state-to-state assignment is planned for a later round"
                            .to_string(),
                    ));
                }
            };
            let given = value_desc(&AstValue::Lit(lit.clone()));
            if given != ty.name() {
                return Err(ctx.err(
                    *p,
                    format!(
                        "cannot assign {} to {} state '{}'",
                        given,
                        ty.name(),
                        stmt.target
                    ),
                    format!(
                        "use a {} literal or change the state initializer",
                        ty.name()
                    ),
                ));
            }
            Ok(json!({"op": "set", "target": stmt.target, "value": json_lit(lit)}))
        }
        StmtKind::Toggle => {
            if ty != Ty::Bool {
                return Err(ctx.err(
                    stmt.pos,
                    format!(
                        "'toggle()' requires a bool state, found {} state '{}'",
                        ty.name(),
                        stmt.target
                    ),
                    "declare the state with true/false or use '+=' / '=' instead".to_string(),
                ));
            }
            Ok(json!({"op": "toggle", "target": stmt.target}))
        }
    }
}

fn lower_node(ctx: &mut Ctx<'_>, node: &Node) -> DResult<Json> {
    let id = ctx.take_id();
    let info = kind_info(&node.kind).ok_or_else(|| {
        ctx.err(
            node.pos,
            format!("unknown node kind '{}'", node.kind),
            format!("use one of: {}", KINDS.join(", ")),
        )
    })?;
    let mut props = JsonMap::new();
    let mut seen = HashSet::new();
    let mut value_pos: Option<Pos> = None;
    for (idx, arg) in node.args.iter().enumerate() {
        let key = match &arg.name {
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
                            format!("'{}' does not accept a positional argument", node.kind),
                            format!(
                                "write props explicitly, e.g. {}({}: value)",
                                node.kind,
                                info.props.first().copied().unwrap_or("value")
                            ),
                        ));
                    }
                }
            }
        };
        if !info.props.contains(&key.as_str()) {
            return Err(ctx.err(
                arg.pos,
                format!("unknown prop '{}' on {}", key, node.kind),
                format!(
                    "{} accepts: {}; remove '{}'",
                    node.kind,
                    info.props.join(", "),
                    key
                ),
            ));
        }
        if !seen.insert(key.clone()) {
            return Err(ctx.err(
                arg.pos,
                format!("duplicate prop '{}' on {}", key, node.kind),
                format!("keep one '{key}' prop per node"),
            ));
        }
        if node.kind == "Text" && key == "value" {
            value_pos = Some(arg.pos);
        }
        let val = prop_value(ctx, &key, arg, &info)?;
        props.insert(key, val);
    }
    if let Some(req) = info.required {
        if !props.contains_key(req) {
            return Err(ctx.err(
                node.pos,
                format!("{} requires the '{}' prop", node.kind, req),
                format!("write {}(\"...\") or {}({}: \"...\")", node.kind, node.kind, req),
            ));
        }
    }
    if let (Some(Json::String(s)), Some(base)) = (props.get("value"), value_pos) {
        let parts = interp_parts(s).map_err(|(off, msg)| {
            let p = offset_pos(base, s, off);
            ctx.err(
                p,
                msg,
                "close it with '}' or write '{{' for a literal '{'".to_string(),
            )
        })?;
        for part in parts {
            if let Part::Var(name, off) = part {
                if !ctx.types.contains_key(&name) {
                    let p = offset_pos(base, s, off);
                    return Err(ctx.err(
                        p,
                        format!("unknown state '{name}' in Text interpolation"),
                        format!(
                            "declare it with 'state {name} = 0' or use one of: {}",
                            ctx.state_list()
                        ),
                    ));
                }
            }
        }
    }

    let mut events = JsonMap::new();
    let mut children = Vec::new();
    if let Some(block) = &node.block {
        if info.tap {
            if !block.nodes.is_empty() {
                return Err(ctx.err(
                    block.pos,
                    "a block may not mix children and statements".to_string(),
                    "a Button block is a tap handler: keep one statement, put nodes in a container"
                        .to_string(),
                ));
            }
            if block.stmts.len() > 1 {
                return Err(ctx.err(
                    block.stmts[1].pos,
                    "a Button tap handler supports exactly one statement in v0.1".to_string(),
                    "split the work across buttons or wait for v0.2 multi-statement handlers"
                        .to_string(),
                ));
            }
            if let Some(stmt) = block.stmts.first() {
                events.insert("tap".to_string(), lower_action(ctx, stmt)?);
            }
        } else if info.children {
            if let Some(stmt) = block.stmts.first() {
                return Err(ctx.err(
                    stmt.pos,
                    format!("a {} block cannot contain statements", node.kind),
                    "put state changes inside a Button { ... } tap handler".to_string(),
                ));
            }
            for child in &block.nodes {
                children.push(lower_node(ctx, child)?);
            }
        } else {
            return Err(ctx.err(
                block.pos,
                format!("'{}' does not support children or tap handlers", node.kind),
                "remove the trailing block; only Column, Row, Container and Button accept blocks"
                    .to_string(),
            ));
        }
    }
    if node.kind == "Input" {
        if let Some(Json::String(name)) = props.get("bind") {
            events.insert("input".to_string(), json!({"op": "set", "target": name}));
        }
    }
    Ok(json!({
        "id": id,
        "kind": node.kind,
        "props": Json::Object(props),
        "children": children,
        "events": Json::Object(events),
    }))
}

fn lower(program: &Program, path: &str) -> DResult<Json> {
    let mut ctx = Ctx {
        path,
        types: HashMap::new(),
        order: Vec::new(),
        next_id: 0,
    };
    let mut state_json = Vec::new();
    for s in &program.states {
        if ctx.types.contains_key(&s.name) {
            return Err(ctx.err(
                s.pos,
                format!("duplicate state '{}'", s.name),
                format!(
                    "rename the second declaration; declared states are: {}",
                    ctx.state_list()
                ),
            ));
        }
        let (ty, init) = match &s.init {
            Lit::Int(n) => (Ty::Int, json!(*n)),
            Lit::Bool(b) => (Ty::Bool, json!(*b)),
            Lit::Str(v) => (Ty::Str, json!(v)),
        };
        ctx.types.insert(s.name.clone(), ty);
        ctx.order.push(s.name.clone());
        state_json.push(json!({"name": s.name, "type": ty.name(), "init": init}));
    }
    if program.nodes.is_empty() {
        return Err(Diagnostic::new(
            path,
            1,
            1,
            format!("app '{}' has no UI node", program.app),
            "add a root node such as Column(gap: 8) { Text(\"hi\") }",
        ));
    }
    let tree = if program.nodes.len() == 1 {
        lower_node(&mut ctx, &program.nodes[0])?
    } else {
        let root_id = ctx.take_id();
        let mut children = Vec::new();
        for n in &program.nodes {
            children.push(lower_node(&mut ctx, n)?);
        }
        json!({
            "id": root_id,
            "kind": "Column",
            "props": {},
            "children": children,
            "events": {},
        })
    };
    Ok(json!({
        "ir": "0.1",
        "app": program.app,
        "state": state_json,
        "tree": tree,
        "theme": {"mode": "system", "accent": "#4F46E5", "radius": 12},
    }))
}
