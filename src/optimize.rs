use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::{json, Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};
use crate::emit::bad_ir;
use crate::lower::{interp_parts, Part};

const IR_FIELDS: &[&str] = &[
    "ir",
    "app",
    "state",
    "tree",
    "theme",
    "models",
    "api",
    "imports",
    "computed",
    "initialRoute",
    "routes",
    "icons",
    "stateRefs",
    "nodeBoundaries",
    "renderQuality",
    "transition",
];
const ROUTE_FIELDS: &[&str] = &["tree", "title"];
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
        Some("0.1.0") => {}
        Some(v) => {
            return Err(Diagnostic::file(
                path,
                format!("unsupported IR version '{v}', expected '0.1.0'"),
                "recompile the source with this version of opuromc",
            ))
        }
        None => {
            return Err(Diagnostic::file(
                path,
                "missing 'ir' version field",
                "IR 0.1.0 must contain \"ir\": \"0.1.0\"",
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
    let routes = o.get("routes");
    match (o.get("tree"), routes) {
        (Some(tree), None) => validate_node(tree, path)?,
        (None, Some(routes)) => {
            let map = routes
                .as_object()
                .ok_or_else(|| bad_ir(path, "IR 'routes' must be an object of route entries"))?;
            if map.is_empty() {
                return Err(bad_ir(path, "IR 'routes' must declare at least one route"));
            }
            for (name, entry) in map {
                let fields = entry
                    .as_object()
                    .ok_or_else(|| bad_ir(path, format!("IR route '{name}' must be an object")))?;
                for key in fields.keys() {
                    if !ROUTE_FIELDS.contains(&key.as_str()) {
                        return Err(bad_ir(
                            path,
                            format!("unknown IR route field '{key}' in route '{name}'"),
                        ));
                    }
                }
                let tree = fields.get("tree").ok_or_else(|| {
                    bad_ir(path, format!("IR route '{name}' is missing its 'tree'"))
                })?;
                validate_node(tree, path)?;
                if let Some(title) = fields.get("title") {
                    if !title.is_string() {
                        return Err(bad_ir(
                            path,
                            format!("IR route '{name}' 'title' must be a string"),
                        ));
                    }
                }
            }
            let initial = o
                .get("initialRoute")
                .ok_or_else(|| bad_ir(path, "multi-page IR must declare 'initialRoute'"))?;
            match initial.as_str() {
                Some(name) if map.contains_key(name) => {}
                Some(name) => {
                    return Err(bad_ir(
                        path,
                        format!("IR 'initialRoute' '{name}' is not a declared route"),
                    ))
                }
                None => {
                    return Err(bad_ir(
                        path,
                        "IR 'initialRoute' must be a route name string",
                    ))
                }
            }
        }
        (Some(_), Some(_)) => {
            return Err(bad_ir(
                path,
                "multi-page IR carries per-route trees and must not also have a top-level 'tree'",
            ))
        }
        (None, None) => {
            return Err(bad_ir(path, "IR is missing the 'tree' field"));
        }
    }
    Ok(())
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
                        Part::Lit(_) | Part::Catalog(_, _) => {}
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
                    "bind"
                        | "when"
                        | "unless"
                        | "target"
                        | "valueFrom"
                        | "into"
                        | "list"
                        | "result"
                        | "error"
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

/// Resolve a reference to the base states whose patches can change it. Computed
/// names expand through surviving operands; loop variables and reserved targets
/// are not state keys and drop out.
fn base_states(
    name: &str,
    live: &HashSet<String>,
    computed: &HashMap<String, Vec<String>>,
    out: &mut BTreeSet<String>,
) {
    let mut stack = vec![name.to_string()];
    let mut seen = HashSet::new();
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if live.contains(&name) {
            out.insert(name);
        } else if let Some(operands) = computed.get(&name) {
            stack.extend(operands.iter().cloned());
        }
    }
}

/// Aggregate the states one node directly observes: its own props, visibility
/// and events, not its children's subtrees. Each child gets its own entry.
fn export_state_refs(
    node: &Json,
    states: &[String],
    live: &HashSet<String>,
    computed: &HashMap<String, Vec<String>>,
    out: &mut BTreeMap<String, BTreeSet<u64>>,
) {
    let Some(id) = node["id"].as_u64() else {
        return;
    };
    let mut own = node.clone();
    if let Some(o) = own.as_object_mut() {
        o.remove("children");
    }
    let mut refs = HashSet::new();
    collect_refs(&own, states, &mut refs);
    let mut base = BTreeSet::new();
    for name in refs {
        base_states(&name, live, computed, &mut base);
    }
    for name in base {
        out.entry(name).or_default().insert(id);
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            export_state_refs(child, states, live, computed, out);
        }
    }
}

fn boundary_true(node: &Json, key: &str) -> bool {
    node["props"]
        .get(key)
        .and_then(Json::as_bool)
        .unwrap_or(false)
}

fn has_boundary(node: &Json) -> bool {
    boundary_true(node, "repaintBoundary")
        || boundary_true(node, "relayoutBoundary")
        || node["children"]
            .as_array()
            .is_some_and(|c| c.iter().any(has_boundary))
}

/// Precompute each node's nearest repaint and relayout boundary, inclusive of
/// the node itself. `null` means no boundary up to the tree root.
fn export_boundaries(
    node: &Json,
    repaint: Option<u64>,
    relayout: Option<u64>,
    out: &mut JsonMap<String, Json>,
) {
    let Some(id) = node["id"].as_u64() else {
        return;
    };
    let rp = if boundary_true(node, "repaintBoundary") {
        Some(id)
    } else {
        repaint
    };
    let rl = if boundary_true(node, "relayoutBoundary") {
        Some(id)
    } else {
        relayout
    };
    out.insert(id.to_string(), json!({"repaint": rp, "relayout": rl}));
    if let Some(children) = node["children"].as_array() {
        for child in children {
            export_boundaries(child, rp, rl, out);
        }
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
/// written without spaces around `*` still keeps its source state. Characters inside a double
/// quoted string literal are not identifiers: a literal's contents must never keep a state alive.
fn expr_idents(expr: &str) -> Vec<String> {
    // Blank quoted spans so a string literal's contents are never read as state.
    let mut masked = String::with_capacity(expr.len());
    let mut in_quote = false;
    let mut escaped = false;
    for c in expr.chars() {
        if in_quote {
            masked.push(' ');
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_quote = false;
            }
        } else if c == '"' {
            masked.push(' ');
            in_quote = true;
        } else {
            masked.push(c);
        }
    }
    masked
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|token| simple_ident(token))
        .map(str::to_string)
        .collect::<Vec<_>>()
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
                Some((name.to_string(), expr_idents(expr)))
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
    if let Some(tree) = ir.get("tree") {
        collect_refs(tree, &states, &mut refs);
    }
    if let Some(routes) = ir.get("routes").and_then(Json::as_object) {
        for entry in routes.values() {
            if let Some(tree) = entry.get("tree") {
                collect_refs(tree, &states, &mut refs);
            }
        }
    }
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
    if let Some(tree) = ir.get_mut("tree") {
        drop_null_props(tree);
    }
    if let Some(routes) = ir.get_mut("routes").and_then(Json::as_object_mut) {
        for entry in routes.values_mut() {
            if let Some(tree) = entry.get_mut("tree") {
                drop_null_props(tree);
            }
        }
    }
    let live: HashSet<String> = ir
        .get("state")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("name").and_then(Json::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let computed: HashMap<String, Vec<String>> = ir
        .get("computed")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?;
                    let expr = entry.get("expr")?.as_str()?;
                    Some((name.to_string(), expr_idents(expr)))
                })
                .collect()
        })
        .unwrap_or_default();
    let trees: Vec<&Json> = ir
        .get("tree")
        .into_iter()
        .chain(
            ir.get("routes")
                .and_then(Json::as_object)
                .into_iter()
                .flat_map(|routes| routes.values().filter_map(|entry| entry.get("tree"))),
        )
        .collect();
    if trees.iter().any(|tree| has_boundary(tree)) {
        let mut boundaries = JsonMap::new();
        let mut refs: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
        for tree in trees {
            export_boundaries(tree, None, None, &mut boundaries);
            export_state_refs(tree, &states, &live, &computed, &mut refs);
        }
        if let Some(o) = ir.as_object_mut() {
            o.insert("stateRefs".to_string(), json!(refs));
            o.insert("nodeBoundaries".to_string(), Json::Object(boundaries));
        }
    }
}

/// Names a node can write at runtime: event action targets plus `input`
/// bindings. The generated native harness reaches state only through events,
/// so this set is closed over every runtime mutation path.
fn collect_writes(node: &Json, writes: &mut HashSet<String>) {
    if let Some(bind) = node
        .get("props")
        .and_then(Json::as_object)
        .and_then(|p| p.get("bind"))
        .and_then(Json::as_str)
    {
        writes.insert(bind.to_string());
    }
    if let Some(events) = node.get("events").and_then(Json::as_object) {
        for action in events.values() {
            let list: Vec<&Json> = match action {
                Json::Array(items) => items.iter().collect(),
                other => vec![other],
            };
            for one in list {
                let is_write = one.get("op").and_then(Json::as_str).is_some_and(|op| {
                    matches!(
                        op,
                        "set" | "add" | "sub" | "toggle" | "assign" | "append" | "removeAt"
                    )
                });
                if is_write {
                    if let Some(target) = one.get("target").and_then(Json::as_str) {
                        writes.insert(target.to_string());
                    }
                }
            }
        }
    }
    if let Some(children) = node.get("children").and_then(Json::as_array) {
        for child in children {
            collect_writes(child, writes);
        }
    }
}

/// Scalar states whose value cannot change at runtime, with their literal
/// init. Computeds enter only when none of their operands is mutable; their
/// init was already evaluated by `infer_computed`.
fn constant_scalars(ir: &Json) -> HashMap<String, Json> {
    let mut mutable = HashSet::new();
    if let Some(tree) = ir.get("tree") {
        collect_writes(tree, &mut mutable);
    }
    if let Some(routes) = ir.get("routes").and_then(Json::as_object) {
        for route in routes.values() {
            if let Some(tree) = route.get("tree") {
                collect_writes(tree, &mut mutable);
            }
        }
    }
    let exprs: Vec<(String, Vec<String>)> = ir
        .get("computed")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    Some((
                        c.get("name")?.as_str()?.to_string(),
                        expr_idents(c.get("expr")?.as_str()?),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    loop {
        let before = mutable.len();
        for (name, operands) in &exprs {
            if operands.iter().any(|op| mutable.contains(op)) {
                mutable.insert(name.clone());
            }
        }
        if mutable.len() == before {
            break;
        }
    }
    ir.get("state")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let name = s.get("name")?.as_str()?;
                    if mutable.contains(name) {
                        return None;
                    }
                    let ty = s.get("type")?.as_str()?;
                    if !matches!(ty, "int" | "bool" | "string") {
                        return None;
                    }
                    let init = s.get("init")?;
                    if !init.is_number() && !init.is_boolean() && !init.is_string() {
                        return None;
                    }
                    Some((name.to_string(), init.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn inline_string(text: &str, consts: &HashMap<String, Json>) -> Option<String> {
    let parts = interp_parts(text).ok()?;
    let mut out = String::new();
    let mut saw_state = false;
    for part in parts {
        match part {
            Part::Lit(lit) => out.push_str(&lit),
            // Catalog keys are resolved before IR; refuse to guess here.
            Part::Catalog(_, _) | Part::Index { .. } => return None,
            Part::Var(name, _) => {
                saw_state = true;
                match consts.get(&name)? {
                    Json::String(v) => out.push_str(v),
                    Json::Number(v) => out.push_str(&v.to_string()),
                    Json::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
                    _ => return None,
                }
            }
        }
    }
    (saw_state && out != text).then_some(out)
}

fn inline_walk(node: &mut Json, consts: &HashMap<String, Json>, count: &mut usize) {
    let Some(o) = node.as_object_mut() else {
        return;
    };
    if let Some(props) = o.get_mut("props").and_then(Json::as_object_mut) {
        for key in ["value", "label", "title", "placeholder", "name"] {
            let Some(text) = props.get(key).and_then(Json::as_str) else {
                continue;
            };
            if let Some(replacement) = inline_string(text, consts) {
                props.insert(key.to_string(), Json::String(replacement));
                *count += 1;
            }
        }
    }
    if let Some(children) = o.get_mut("children").and_then(Json::as_array_mut) {
        for child in children {
            inline_walk(child, consts, count);
        }
    }
}

/// Stage 2a step 1: replace state interpolations whose value cannot change
/// with their literal value. No node is added or removed and no id changes.
pub fn inline_constant_text(ir: &mut Json) -> usize {
    let consts = constant_scalars(ir);
    if consts.is_empty() {
        return 0;
    }
    let mut count = 0;
    if let Some(tree) = ir.get_mut("tree") {
        inline_walk(tree, &consts, &mut count);
    }
    if let Some(routes) = ir.get_mut("routes").and_then(Json::as_object_mut) {
        for route in routes.values_mut() {
            if let Some(tree) = route.get_mut("tree") {
                inline_walk(tree, &consts, &mut count);
            }
        }
    }
    count
}

/// Props and fields that make deleting a node unsafe: runtime interaction,
/// animation/effects, incremental-render boundaries and generated subtrees.
fn delete_blocked(node: &Json) -> bool {
    let Some(o) = node.as_object() else {
        return true;
    };
    if o.get("events")
        .and_then(Json::as_object)
        .is_some_and(|e| !e.is_empty())
    {
        return true;
    }
    if o.get("repeat").is_some() || o.get("component").is_some() {
        return true;
    }
    let Some(props) = o.get("props").and_then(Json::as_object) else {
        return false;
    };
    const BLOCKERS: &[&str] = &[
        "bind",
        "gesture",
        "role",
        "state",
        "disabled",
        "variant",
        "tone",
        "high_contrast",
        "animate",
        "duration",
        "delay",
        "animClock",
        "animInterval",
        "effects",
        "particles",
        "repaintBoundary",
        "relayoutBoundary",
        "source",
        "item",
    ];
    BLOCKERS.iter().any(|key| props.contains_key(*key))
}

/// True when no node in the subtree can be addressed by an event, gesture or
/// accessibility action. A hidden branch with a live handler can still be
/// fired by the native harness, so it must not be deleted.
fn dispatch_free(node: &Json) -> bool {
    if delete_blocked(node) {
        return false;
    }
    node.get("children")
        .and_then(Json::as_array)
        .is_none_or(|children| children.iter().all(dispatch_free))
}

fn count_nodes(node: &Json) -> usize {
    1 + node
        .get("children")
        .and_then(Json::as_array)
        .map(|c| c.iter().map(count_nodes).sum::<usize>())
        .unwrap_or(0)
}

/// Resolve `when`/`unless` on states proven constant. A permanently visible
/// key is dropped; a permanently hidden subtree is deleted when it carries no
/// dispatchable behaviour. Returns true when anything was deleted.
fn fold_visibility(
    node: &mut Json,
    bools: &HashMap<String, bool>,
    report: &mut (usize, usize),
    is_root: bool,
) -> bool {
    if let Some(children) = node.get_mut("children").and_then(Json::as_array_mut) {
        children.retain_mut(|child| !fold_visibility(child, bools, report, false));
    }
    let hide = node
        .get("when")
        .and_then(Json::as_str)
        .and_then(|name| bools.get(name))
        .is_some_and(|visible| !*visible)
        || node
            .get("unless")
            .and_then(Json::as_str)
            .and_then(|name| bools.get(name))
            .is_some_and(|visible| *visible);
    if hide && !is_root && !delete_blocked(node) && dispatch_free(node) {
        report.0 += count_nodes(node);
        return true;
    }
    let mut removed = false;
    if let Some(name) = node.get("when").and_then(Json::as_str).map(str::to_string) {
        if bools.get(&name) == Some(&true) {
            node.as_object_mut().map(|o| o.remove("when"));
            removed = true;
        }
    }
    if let Some(name) = node
        .get("unless")
        .and_then(Json::as_str)
        .map(str::to_string)
    {
        if bools.get(&name) == Some(&false) {
            node.as_object_mut().map(|o| o.remove("unless"));
            removed = true;
        }
    }
    report.1 += usize::from(removed);
    false
}

/// Renumber one tree in pre-order from its current root id and record the
/// old→new id map. Route trees keep their own root id, so no id crosses a
/// route boundary.
fn renumber_tree(root: &mut Json, map: &mut BTreeMap<String, String>) {
    fn walk(node: &mut Json, next: &mut u64, map: &mut BTreeMap<String, String>) {
        let Some(o) = node.as_object_mut() else {
            return;
        };
        if let Some(old) = o.get("id").and_then(Json::as_u64) {
            let new = *next;
            *next += 1;
            o.insert("id".to_string(), Json::Number(new.into()));
            map.insert(old.to_string(), new.to_string());
        }
        if let Some(children) = o.get_mut("children").and_then(Json::as_array_mut) {
            for child in children {
                walk(child, next, map);
            }
        }
    }
    let start = root.get("id").and_then(Json::as_u64).unwrap_or(0);
    let mut next = start;
    walk(root, &mut next, map);
}

/// Recompute the id-keyed export maps from the current tree without pruning
/// state or props. Called after a renumber so no stale id survives.
fn rebuild_id_maps(ir: &mut Json) {
    let states: Vec<String> = ir
        .get("state")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("name")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let live: HashSet<String> = states.iter().cloned().collect();
    let computed: HashMap<String, Vec<String>> = ir
        .get("computed")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?;
                    let expr = entry.get("expr")?.as_str()?;
                    Some((name.to_string(), expr_idents(expr)))
                })
                .collect()
        })
        .unwrap_or_default();
    let trees: Vec<&Json> = ir
        .get("tree")
        .into_iter()
        .chain(
            ir.get("routes")
                .and_then(Json::as_object)
                .into_iter()
                .flat_map(|routes| routes.values().filter_map(|entry| entry.get("tree"))),
        )
        .collect();
    let has_boundary = trees.iter().any(|tree| has_boundary(tree));
    let mut boundaries = JsonMap::new();
    let mut refs: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    if has_boundary {
        for tree in &trees {
            export_boundaries(tree, None, None, &mut boundaries);
            export_state_refs(tree, &states, &live, &computed, &mut refs);
        }
    }
    if let Some(o) = ir.as_object_mut() {
        o.remove("stateRefs");
        o.remove("nodeBoundaries");
        if has_boundary {
            o.insert("stateRefs".to_string(), json!(refs));
            o.insert("nodeBoundaries".to_string(), Json::Object(boundaries));
        }
    }
}

/// Default-path compile passes: fold constant numeric/string/boolean
/// expressions, prune dead states and computed entries, then remove
/// statically-unreachable nodes. Every pass is a byte no-op on programs
/// without foldable patterns, so goldens stay identical. Returns the old→new
/// id map when node ids changed, for the write-only build sidecar.
pub fn fold_default(ir: &mut Json) -> Option<Json> {
    fold_constants(ir);
    inline_constant_text(ir);
    fold_node_visibility(ir)
}

/// Stage 2a step 2: fold visibility of constant bool states, renumber every
/// affected tree and rebuild the id-keyed export maps. Returns the old→new id
/// map when node ids changed; callers write it as a build sidecar, never into
/// IR.
pub fn fold_node_visibility(ir: &mut Json) -> Option<Json> {
    let bools: HashMap<String, bool> = constant_scalars(ir)
        .into_iter()
        .filter_map(|(name, value)| value.as_bool().map(|b| (name, b)))
        .collect();
    if bools.is_empty() {
        return None;
    }
    let mut report = (0usize, 0usize);
    let mut map = BTreeMap::new();
    if let Some(tree) = ir.get_mut("tree") {
        let before = report.0;
        fold_visibility(tree, &bools, &mut report, true);
        if report.0 > before {
            renumber_tree(tree, &mut map);
        }
    }
    if let Some(routes) = ir.get_mut("routes").and_then(Json::as_object_mut) {
        for route in routes.values_mut() {
            if let Some(tree) = route.get_mut("tree") {
                let before = report.0;
                fold_visibility(tree, &bools, &mut report, true);
                if report.0 > before {
                    renumber_tree(tree, &mut map);
                }
            }
        }
    }
    if report.0 == 0 && report.1 == 0 {
        return None;
    }
    // The old maps were computed on the pre-fold ids. Rebuild them from the
    // folded tree, keeping state and props untouched.
    rebuild_id_maps(ir);
    if report.0 == 0 {
        return None;
    }
    Some(Json::Object(
        map.into_iter()
            .map(|(old, new)| (old, Json::String(new)))
            .collect(),
    ))
}

/// Evaluate a whitespace-separated integer expression when it has no state
/// operands. Uses the emitters' `+ - *` grammar and i64 checked arithmetic;
/// anything else (a state name, overflow, malformed input) returns None so the
/// caller leaves the IR alone.
fn folded_integer(expr: &str) -> Option<i64> {
    let toks: Vec<&str> = expr.split_whitespace().collect();
    fn term(toks: &[&str], at: &mut usize) -> Option<i64> {
        let mut value = toks.get(*at)?.parse::<i64>().ok()?;
        *at += 1;
        while toks.get(*at).copied() == Some("*") {
            *at += 1;
            let rhs = toks.get(*at)?.parse::<i64>().ok()?;
            *at += 1;
            value = value.checked_mul(rhs)?;
        }
        Some(value)
    }
    let mut at = 0;
    let mut value = term(&toks, &mut at)?;
    while at < toks.len() {
        let op = toks[at];
        at += 1;
        let rhs = term(&toks, &mut at)?;
        value = match op {
            "+" => value.checked_add(rhs)?,
            "-" => value.checked_sub(rhs)?,
            _ => return None,
        };
    }
    Some(value)
}

/// Fold constant-only computed expressions into their integer literal. The
/// state `init` already holds this value from `infer_computed`; only `expr`
/// changes, so target emitters write a literal instead of runtime arithmetic.
/// IR shape, node bytes and every golden stay untouched.
pub fn fold_constants(ir: &mut Json) -> usize {
    // Only fold when the compiler's own `init` already equals the folded
    // value, so a hand-written IR with an inconsistent init is never changed.
    let inits: HashMap<String, i64> = ir
        .get("state")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    Some((
                        s.get("name")?.as_str()?.to_string(),
                        s.get("init")?.as_i64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(list) = ir.get_mut("computed").and_then(Json::as_array_mut) else {
        return 0;
    };
    let mut folded = 0;
    for entry in list.iter_mut() {
        let Some(name) = entry.get("name").and_then(Json::as_str).map(str::to_string) else {
            continue;
        };
        let Some(expr) = entry.get("expr").and_then(Json::as_str) else {
            continue;
        };
        if let Some(value) = folded_integer(expr) {
            if inits.get(&name) != Some(&value) {
                continue;
            }
            if let Some(o) = entry.as_object_mut() {
                o.insert("expr".to_string(), Json::String(value.to_string()));
                folded += 1;
            }
        }
    }
    folded
}

pub fn run(ir_path: &str, out: &Path, fold: bool) -> DResult<()> {
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
    if fold {
        fold_constants(&mut ir);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_constant_text_skips_written_and_computed_mutable_states() {
        let mut ir = json!({
            "state": [
                {"name": "label", "type": "string", "init": "ready"},
                {"name": "count", "type": "int", "init": 0},
                {"name": "base", "type": "int", "init": 2},
                {"name": "double", "type": "int", "init": 4}
            ],
            "computed": [{"name": "double", "type": "int", "expr": "base * 2"}],
            "tree": {"id": 0, "kind": "Column", "props": {}, "events": {}, "children": [
                {"id": 1, "kind": "Text", "props": {"value": "{label} {double}"}, "events": {}, "children": []},
                {"id": 2, "kind": "Text", "props": {"value": "{count}"},
                 "events": {"tap": {"op": "set", "target": "count", "value": 1}}, "children": []}
            ]}
        });
        assert_eq!(inline_constant_text(&mut ir), 1);
        assert_eq!(ir["tree"]["children"][0]["props"]["value"], "ready 4");
        assert_eq!(ir["tree"]["children"][1]["props"]["value"], "{count}");
        assert_eq!(ir["tree"]["children"][0]["id"], 1, "no id changes");
    }

    #[test]
    fn folded_integer_respects_precedence_and_overflow() {
        assert_eq!(folded_integer("2 + 3 * 4"), Some(14));
        assert_eq!(folded_integer("-1 * 5 - 2"), Some(-7));
        assert_eq!(folded_integer("count * 2"), None);
        assert_eq!(folded_integer("9223372036854775807 + 1"), None);
        assert_eq!(folded_integer("2 ** 3"), None);
        // New comparison, boolean and string tokens are never folded: a
        // non-integer token makes the grammar fail and returns None.
        for expr in [
            "1 < 2",
            "1 == 2",
            "1 >= 2",
            "1 && 0",
            "1 || 0",
            "!1",
            "\"x\" == \"y\"",
            "1 + 2 < 4",
        ] {
            assert_eq!(folded_integer(expr), None, "must not fold '{expr}'");
        }
    }

    #[test]
    fn expr_idents_ignore_string_literal_contents() {
        assert_eq!(
            expr_idents("a + b * 2"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(expr_idents("s == \"alpha beta\""), vec!["s".to_string()]);
        // Literal text that looks like a state name must not be kept alive.
        assert!(
            !expr_idents("\"count flag\" == s").contains(&"count".to_string()),
            "literal contents counted as state"
        );
        assert!(
            expr_idents("\"count flag\" == s").contains(&"s".to_string()),
            "real state outside the literal must still be read"
        );
        // An escaped quote does not prematurely end the literal.
        assert!(
            !expr_idents("s == \"a\\\"count\"").contains(&"count".to_string()),
            "text after an escaped quote counted as state"
        );
    }

    #[test]
    fn fold_constants_only_rewrites_pure_expressions() {
        let mut ir = json!({
            "state": [
                {"name": "c", "type": "int", "init": 10},
                {"name": "d", "type": "int", "init": 0}
            ],
            "computed": [
                {"name": "c", "type": "int", "expr": "2 * 3 + 4"},
                {"name": "d", "type": "int", "expr": "count * 2"}
            ]
        });
        assert_eq!(fold_constants(&mut ir), 1);
        assert_eq!(ir["computed"][0]["expr"], "10");
        assert_eq!(ir["computed"][1]["expr"], "count * 2");
        assert_eq!(ir["state"][0]["init"], 10, "state is not rewritten");
    }

    #[test]
    fn fold_constants_skips_a_mismatched_init() {
        let mut ir = json!({
            "state": [{"name": "c", "type": "int", "init": 11}],
            "computed": [{"name": "c", "type": "int", "expr": "2 * 3 + 4"}]
        });
        assert_eq!(fold_constants(&mut ir), 0);
        assert_eq!(ir["computed"][0]["expr"], "2 * 3 + 4");
    }
}
