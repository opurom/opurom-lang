use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde_json::Value as Json;

use crate::diag::{DResult, Diagnostic};
use crate::emit::bad_ir;
use crate::lower::{interp_parts, Part};

const IR_FIELDS: &[&str] = &[
    "ir", "app", "state", "tree", "theme", "models", "api", "imports", "computed",
];
const STATE_FIELDS: &[&str] = &["name", "type", "init"];
const COMPUTED_FIELDS: &[&str] = &["name", "type", "expr"];
const NODE_FIELDS: &[&str] = &[
    "id",
    "kind",
    "props",
    "children",
    "events",
    "when",
    "unless",
    "component",
    "repeat",
];

fn validate(ir: &Json, path: &str) -> DResult<()> {
    let o = ir
        .as_object()
        .ok_or_else(|| bad_ir(path, "IR must be a JSON object"))?;
    for key in o.keys() {
        if !IR_FIELDS.contains(&key.as_str()) {
            return Err(bad_ir(path, format!("unknown IR field '{key}'")));
        }
    }
    match o.get("ir").and_then(Json::as_str) {
        Some("0.1") => {}
        Some(v) => {
            return Err(Diagnostic::file(
                path,
                format!("unsupported IR version '{v}', expected '0.1'"),
                "recompile the source with this version of opuromc",
            ))
        }
        None => {
            return Err(Diagnostic::file(
                path,
                "missing 'ir' version field",
                "IR v0.1 must contain \"ir\": \"0.1\"",
            ))
        }
    }
    if let Some(state) = o.get("state") {
        let arr = state
            .as_array()
            .ok_or_else(|| bad_ir(path, "IR 'state' must be an array"))?;
        for entry in arr {
            let fields = entry
                .as_object()
                .ok_or_else(|| bad_ir(path, "IR state entry must be an object"))?;
            for key in fields.keys() {
                if !STATE_FIELDS.contains(&key.as_str()) {
                    return Err(bad_ir(path, format!("unknown IR state field '{key}'")));
                }
            }
            if fields.get("name").and_then(Json::as_str).is_none() {
                return Err(bad_ir(path, "IR state entry is missing a string 'name'"));
            }
        }
    }
    if let Some(imports) = o.get("imports") {
        let arr = imports
            .as_array()
            .ok_or_else(|| bad_ir(path, "IR 'imports' must be an array"))?;
        for entry in arr {
            if !entry.is_string() {
                return Err(bad_ir(path, "IR 'imports' entries must be strings"));
            }
        }
    }
    if let Some(computed) = o.get("computed") {
        let arr = computed
            .as_array()
            .ok_or_else(|| bad_ir(path, "IR 'computed' must be an array"))?;
        for entry in arr {
            let fields = entry
                .as_object()
                .ok_or_else(|| bad_ir(path, "IR computed entry must be an object"))?;
            for key in fields.keys() {
                if !COMPUTED_FIELDS.contains(&key.as_str()) {
                    return Err(bad_ir(path, format!("unknown IR computed field '{key}'")));
                }
            }
            if fields.get("name").and_then(Json::as_str).is_none() {
                return Err(bad_ir(path, "IR computed entry is missing a string 'name'"));
            }
            let ty = fields
                .get("type")
                .and_then(Json::as_str)
                .ok_or_else(|| bad_ir(path, "IR computed entry is missing a string 'type'"))?;
            if !matches!(ty, "int" | "bool" | "string") {
                return Err(bad_ir(path, format!("unsupported IR computed type '{ty}'")));
            }
            if fields.get("expr").and_then(Json::as_str).is_none() {
                return Err(bad_ir(path, "IR computed entry is missing a string 'expr'"));
            }
        }
    }
    let tree = o
        .get("tree")
        .ok_or_else(|| bad_ir(path, "IR is missing the 'tree' field"))?;
    validate_node(tree, path)
}

fn validate_node(v: &Json, path: &str) -> DResult<()> {
    let o = v
        .as_object()
        .ok_or_else(|| bad_ir(path, "IR tree node must be an object"))?;
    for key in o.keys() {
        if !NODE_FIELDS.contains(&key.as_str()) {
            return Err(bad_ir(path, format!("unknown IR node field '{key}'")));
        }
    }
    if let Some(props) = o.get("props") {
        if !props.is_object() {
            return Err(bad_ir(path, "IR node 'props' must be an object"));
        }
    }
    if let Some(events) = o.get("events") {
        if !events.is_object() {
            return Err(bad_ir(path, "IR node 'events' must be an object"));
        }
    }
    if let Some(repeat) = o.get("repeat") {
        let fields = repeat
            .as_object()
            .ok_or_else(|| bad_ir(path, "IR node 'repeat' must be an object"))?;
        if fields
            .keys()
            .any(|k| !matches!(k.as_str(), "item" | "list"))
            || fields.get("item").and_then(Json::as_str).is_none()
            || fields.get("list").and_then(Json::as_str).is_none()
        {
            return Err(bad_ir(
                path,
                "IR node 'repeat' needs string 'item' and 'list' fields",
            ));
        }
    }
    if let Some(children) = o.get("children") {
        let arr = children
            .as_array()
            .ok_or_else(|| bad_ir(path, "IR node 'children' must be an array"))?;
        for child in arr {
            validate_node(child, path)?;
        }
    }
    Ok(())
}

/// Collect every state name a tree can observe. Walking all strings also covers props
/// other than `Text.value`; plain state references (`bind`, `when`, actions) are keyed.
fn collect_refs(v: &Json, states: &[String], refs: &mut HashSet<String>) {
    match v {
        Json::String(s) => match interp_parts(s) {
            Ok(parts) => {
                for part in parts {
                    match part {
                        Part::Var(name, _) => {
                            refs.insert(name);
                        }
                        // A list read observes the list state, so keep it live.
                        Part::Index { list, .. } => {
                            refs.insert(list);
                        }
                        Part::Lit(_) => {}
                    }
                }
            }
            // shortcut: malformed interpolation keeps every state, upgrade when IR strings are validated
            Err(_) => refs.extend(states.iter().cloned()),
        },
        Json::Array(list) => {
            for item in list {
                collect_refs(item, states, refs);
            }
        }
        Json::Object(o) => {
            if o.get("op").and_then(Json::as_str) == Some("validate") {
                return;
            }
            for (key, value) in o {
                if matches!(
                    key.as_str(),
                    "bind" | "when" | "unless" | "target" | "valueFrom" | "into" | "list"
                ) {
                    if let Some(name) = value.as_str() {
                        refs.insert(name.to_string());
                    }
                }
                collect_refs(value, states, refs);
            }
        }
        _ => {}
    }
}

fn drop_null_props(node: &mut Json) {
    if let Some(props) = node.get_mut("props").and_then(Json::as_object_mut) {
        props.retain(|_, v| !v.is_null());
    }
    if let Some(children) = node.get_mut("children").and_then(Json::as_array_mut) {
        for child in children {
            drop_null_props(child);
        }
    }
}

fn simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Identifiers a computed expression can read. Operators are separators too, so an expression
/// written without spaces around `*` still keeps its source state.
fn expr_idents(expr: &str) -> impl Iterator<Item = &str> + '_ {
    expr.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|token| simple_ident(token))
}

/// Keep computed declarations whose names the tree or another surviving declaration reads.
/// Operands of a surviving declaration stay live because emitters recompute from those states;
/// a removed declaration must not keep its private source states alive.
fn prune_dead_computed(ir: &mut Json, refs: &mut HashSet<String>) {
    let exprs: Vec<(String, Vec<String>)> = match ir.get("computed").and_then(Json::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|entry| {
                let name = entry.get("name").and_then(Json::as_str)?;
                let expr = entry.get("expr").and_then(Json::as_str).unwrap_or("");
                Some((
                    name.to_string(),
                    expr_idents(expr).map(str::to_string).collect(),
                ))
            })
            .collect(),
        None => return,
    };
    loop {
        let before = refs.len();
        for (name, operands) in &exprs {
            if refs.contains(name) {
                refs.extend(operands.iter().cloned());
            }
        }
        if refs.len() == before {
            break;
        }
    }
    if let Some(list) = ir.get_mut("computed").and_then(Json::as_array_mut) {
        list.retain(|entry| {
            entry
                .get("name")
                .and_then(Json::as_str)
                .is_some_and(|name| refs.contains(name))
        });
    }
    if ir
        .get("computed")
        .and_then(Json::as_array)
        .is_some_and(|list| list.is_empty())
    {
        if let Some(o) = ir.as_object_mut() {
            o.remove("computed");
        }
    }
}

/// Dead-computed and dead-state elimination: nodes, ids, conditionals, component and state order
/// survive.
fn optimize(ir: &mut Json) {
    let states: Vec<String> = ir
        .get("state")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("name").and_then(Json::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut refs = HashSet::new();
    collect_refs(&ir["tree"], &states, &mut refs);
    prune_dead_computed(ir, &mut refs);
    if let Some(list) = ir.get_mut("state").and_then(Json::as_array_mut) {
        list.retain(|s| {
            let Some(name) = s.get("name").and_then(Json::as_str) else {
                return false;
            };
            let is_list = s.get("type").and_then(Json::as_str) == Some("list");
            is_list || refs.contains(name)
        });
    }
    drop_null_props(&mut ir["tree"]);
}

pub fn run(ir_path: &str, out: &Path) -> DResult<()> {
    let bytes = fs::read(ir_path).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot read IR file: {e}"),
            "compile a .opu file with 'opuromc compile' first",
        )
    })?;
    let mut ir: Json = serde_json::from_slice(&bytes).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("invalid IR JSON: {e}"),
            "compile a .opu file with 'opuromc compile' to produce valid IR",
        )
    })?;
    validate(&ir, ir_path)?;
    optimize(&mut ir);
    let text = serde_json::to_string_pretty(&ir).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot serialize IR: {e}"),
            "report this compiler bug",
        )
    })? + "\n";
    fs::write(out, &text).map_err(|e| {
        Diagnostic::file(
            ir_path,
            format!("cannot write {}: {e}", out.display()),
            "check the -o path and permissions",
        )
    })?;
    let output_bytes = fs::metadata(out)
        .map_err(|e| {
            Diagnostic::file(
                ir_path,
                format!("cannot stat {}: {e}", out.display()),
                "check the -o path and permissions",
            )
        })?
        .len();
    println!(
        "optimize: {} bytes in, {} bytes out",
        bytes.len(),
        output_bytes
    );
    Ok(())
}
