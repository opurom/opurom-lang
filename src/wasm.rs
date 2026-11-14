//! The `wasm` emitter: a std-only `wasm32-unknown-unknown` cdylib with the
//! C ABI of [WASM_ABI.md], plus a browser host that loads the `.wasm`.

use std::path::Path;

use serde_json::Value as Json;

use crate::diag::DResult;
use crate::emit::{
    bad_ir, computed_expr, event_actions, event_effects, Computed, ENode, EState, Effect,
};

struct WasmState {
    name: String,
    init: i64,
}

fn read_int_states(states: &[EState]) -> Vec<WasmState> {
    states
        .iter()
        .filter(|s| s.ty == "int")
        .map(|s| WasmState {
            name: s.name.clone(),
            init: s.init.as_i64().unwrap_or(0),
        })
        .collect()
}

fn collect<'a>(n: &'a ENode, out: &mut Vec<&'a ENode>) {
    out.push(n);
    for c in &n.children {
        collect(c, out);
    }
}

/// One generated action step. `value` is a literal, `value_from` reads another state.
enum Step {
    Set { target: usize, value: i64 },
    Add { target: usize, value: i64 },
    Sub { target: usize, value: i64 },
    AddFrom { target: usize, from: usize },
    SubFrom { target: usize, from: usize },
    SetFrom { target: usize, from: usize },
}

fn int_index(states: &[WasmState], name: &str) -> Option<usize> {
    states.iter().position(|s| s.name == name)
}

/// Lower an event chain into integer steps. Bool/string states live in the host.
fn steps_of(event: &Json, states: &[WasmState], ir_path: &str) -> DResult<Vec<Step>> {
    let mut out = Vec::new();
    for action in event_actions(event) {
        let op = action
            .get("op")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "event action is missing string 'op'"))?;
        let target = action
            .get("target")
            .and_then(Json::as_str)
            .ok_or_else(|| bad_ir(ir_path, "event action is missing string 'target'"))?;
        let target_index = match int_index(states, target) {
            Some(i) => i,
            None => continue, // bool/string states are mirrored in host.js
        };
        match op {
            "set" => {
                if let Some(from) = action.get("valueFrom").and_then(Json::as_str) {
                    // A non-int source is mirrored in the host, not in the wasm.
                    if let Some(from_index) = int_index(states, from) {
                        out.push(Step::SetFrom {
                            target: target_index,
                            from: from_index,
                        });
                    }
                } else {
                    let value = action.get("value").and_then(Json::as_i64).unwrap_or(0);
                    out.push(Step::Set {
                        target: target_index,
                        value,
                    });
                }
            }
            "add" => {
                if let Some(from) = action.get("valueFrom").and_then(Json::as_str) {
                    // A non-int source is mirrored in the host, not in the wasm.
                    if let Some(from_index) = int_index(states, from) {
                        out.push(Step::AddFrom {
                            target: target_index,
                            from: from_index,
                        });
                    }
                } else {
                    let value = action.get("value").and_then(Json::as_i64).unwrap_or(0);
                    out.push(Step::Add {
                        target: target_index,
                        value,
                    });
                }
            }
            "sub" => {
                if let Some(from) = action.get("valueFrom").and_then(Json::as_str) {
                    // A non-int source is mirrored in the host, not in the wasm.
                    if let Some(from_index) = int_index(states, from) {
                        out.push(Step::SubFrom {
                            target: target_index,
                            from: from_index,
                        });
                    }
                } else {
                    let value = action.get("value").and_then(Json::as_i64).unwrap_or(0);
                    out.push(Step::Sub {
                        target: target_index,
                        value,
                    });
                }
            }
            // A toggle targets a bool state, which lives in the host mirror.
            "toggle" => {}
            other => {
                return Err(bad_ir(ir_path, format!("unknown event op '{other}'")));
            }
        }
    }
    Ok(out)
}

fn emit_steps(steps: &[Step], indent: &str) -> String {
    let mut out = String::new();
    for step in steps {
        match step {
            Step::Set { target, value } => {
                out.push_str(&format!("{indent}set_state({target}, {value});\n"))
            }
            Step::Add { target, value } => out.push_str(&format!(
                "{indent}set_state({target}, state({target}).saturating_add({value}));\n"
            )),
            Step::Sub { target, value } => out.push_str(&format!(
                "{indent}set_state({target}, state({target}).saturating_sub({value}));\n"
            )),
            Step::AddFrom { target, from } => out.push_str(&format!(
                "{indent}set_state({target}, state({target}).saturating_add(state({from})));\n"
            )),
            Step::SubFrom { target, from } => out.push_str(&format!(
                "{indent}set_state({target}, state({target}).saturating_sub(state({from})));\n"
            )),
            Step::SetFrom { target, from } => {
                out.push_str(&format!("{indent}set_state({target}, state({from}));\n"))
            }
        }
    }
    out
}

fn wasm_computed_source(
    states: &[EState],
    int_states: &[WasmState],
    computed: &[Computed],
    ir_path: &str,
) -> DResult<(String, usize)> {
    let mut body = String::new();
    let mut count = 0usize;
    for c in computed.iter().filter(|c| c.ty.as_str() == "int") {
        let target = int_index(int_states, &c.name).ok_or_else(|| {
            bad_ir(
                ir_path,
                format!("computed value '{}' is not an int state", c.name),
            )
        })?;
        let (_, expr) = computed_expr(
            &c.expr,
            states,
            computed,
            ir_path,
            true,
            |st| match int_index(int_states, &st.name) {
                Some(index) => Ok(format!("state({index})")),
                None => Err(bad_ir(
                    ir_path,
                    format!("IR computed source '{}' is not an int state", st.name),
                )),
            },
        )?;
        body.push_str(&format!("    set_state({target}, {expr});\n"));
        count += 1;
    }
    if count == 0 {
        return Ok((String::new(), 0));
    }
    Ok((
        format!(
            "#[allow(dead_code)]\nfn apply_computed() -> u32 {{\n    let mut count = 0u32;\n{body}    count += {count};\n    count\n}}\n\n"
        ),
        count,
    ))
}

fn manifest() -> String {
    concat!(
        "[package]\n",
        "name = \"opurom_wasm\"\n",
        "version = \"0.2.0\"\n",
        "edition = \"2021\"\n\n",
        "[lib]\n",
        "crate-type = [\"cdylib\"]\n",
        "path = \"src/lib.rs\"\n\n",
        "[workspace]\n\n",
        "[dependencies]\n",
    )
    .to_string()
}

fn lib_source(
    app: &str,
    states: &[EState],
    int_states: &[WasmState],
    computed: &[Computed],
    tree: &ENode,
    ir_path: &str,
) -> DResult<String> {
    let mut s = String::new();
    s.push_str(&format!(
        "// Generated by opuromc for app {app}; lowered from IR v0.1 to wasm32. Do not edit.\n"
    ));
    s.push_str("use std::sync::Mutex;\n\n");
    s.push_str("static STATE: Mutex<[i64; ");
    s.push_str(&int_states.len().to_string());
    s.push_str("]> = Mutex::new([");
    for (i, st) in int_states.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&st.init.to_string());
    }
    s.push_str("]);\n\n");
    s.push_str("fn state(index: usize) -> i64 {\n");
    s.push_str("    let guard = match STATE.lock() {\n");
    s.push_str("        Ok(g) => g,\n");
    s.push_str("        Err(poisoned) => poisoned.into_inner(),\n");
    s.push_str("    };\n");
    s.push_str("    if index >= guard.len() {\n        return -1;\n    }\n");
    s.push_str("    guard[index]\n}\n\n");
    s.push_str("fn set_state(index: usize, value: i64) {\n");
    s.push_str("    let mut guard = match STATE.lock() {\n");
    s.push_str("        Ok(g) => g,\n");
    s.push_str("        Err(poisoned) => poisoned.into_inner(),\n");
    s.push_str("    };\n");
    s.push_str("    if index < guard.len() {\n        guard[index] = value;\n    }\n");
    s.push_str("}\n\n");
    let (computed_source, computed_count) =
        wasm_computed_source(states, int_states, computed, ir_path)?;
    s.push_str(&computed_source);
    let computed_call = if computed_count > 0 {
        "    count += apply_computed();\n"
    } else {
        ""
    };

    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    for n in &nodes {
        for event in ["tap", "input"] {
            let Some(action) = n.events.get(event) else {
                continue;
            };
            let steps = steps_of(action, int_states, ir_path)?;
            if steps.is_empty() {
                continue;
            }
            let body = emit_steps(&steps, "    ");
            let count = steps.len();
            match event {
                "tap" => s.push_str(&format!(
                    "fn tap_{id}() -> u32 {{\n    let mut count = 0u32;\n{body}    count += {patch_count};\n{computed_call}    count\n}}\n\n",
                    id = n.id,
                    body = body,
                    patch_count = count
                )),
                _ => s.push_str(&format!(
                    "fn input_{id}(value: i32) -> u32 {{\n    let mut count = 0u32;\n{body}    count += {patch_count};\n{computed_call}    count\n}}\n\n",
                    id = n.id,
                    body = body,
                    patch_count = count
                )),
            }
        }
    }

    s.push_str(concat!(
        "#[no_mangle]\n",
        "pub extern \"C\" fn opu_version() -> u32 {\n",
        "    1\n",
        "}\n\n",
        "#[no_mangle]\n",
        "pub extern \"C\" fn opu_state_len() -> u32 {\n",
    ));
    s.push_str(&format!("    {}u32\n}}\n\n", int_states.len()));
    s.push_str(concat!(
        "#[no_mangle]\n",
        "pub extern \"C\" fn opu_state(index: u32) -> i32 {\n",
        "    state(index as usize) as i32\n",
        "}\n\n",
        "#[no_mangle]\n",
        "pub extern \"C\" fn opu_tap(node_id: u32) -> u32 {\n",
        "    match node_id {\n",
    ));
    for n in &nodes {
        let Some(action) = n.events.get("tap") else {
            continue;
        };
        if steps_of(action, int_states, ir_path)?.is_empty() {
            continue;
        }
        s.push_str(&format!("        {} => tap_{}(),\n", n.id, n.id));
    }
    s.push_str(concat!(
        "        _ => 0,\n",
        "    }\n",
        "}\n\n",
        "#[no_mangle]\n",
        "pub extern \"C\" fn opu_input(node_id: u32, v: i32) -> u32 {\n",
        "    let _ = v;\n",
        "    match node_id {\n",
    ));
    for n in &nodes {
        let Some(action) = n.events.get("input") else {
            continue;
        };
        let steps = steps_of(action, int_states, ir_path)?;
        if steps.is_empty() {
            continue;
        }
        s.push_str(&format!("        {} => input_{}(v),\n", n.id, n.id));
    }
    s.push_str(concat!("        _ => 0,\n", "    }\n", "}\n"));
    Ok(s)
}

fn js_str(s: &str) -> String {
    serde_json::to_string(&Json::String(s.to_string())).unwrap_or_else(|_| "\"\"".to_string())
}

fn effects_map(tree: &ENode, ir_path: &str) -> DResult<String> {
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut entries = Vec::new();
    for node in nodes {
        let Some(event) = node.events.get("tap") else {
            continue;
        };
        let effects = event_effects(event, ir_path)?;
        if effects.is_empty() {
            continue;
        }
        let mut items = Vec::new();
        for effect in effects {
            match effect {
                Effect::Delay { ms } => {
                    items.push(format!("{{ kind: \"delay\", ms: {ms} }}"));
                }
                Effect::Fetch { path, into } => items.push(format!(
                    "{{ kind: \"fetch\", path: {}, into: {} }}",
                    js_str(&path),
                    js_str(&into)
                )),
            }
        }
        entries.push(format!("  {}: [{}],", node.id, items.join(", ")));
    }
    Ok(format!("{{\n{}\n}}", entries.join("\n")))
}

fn host_effects(tree: &ENode, ir_path: &str) -> DResult<String> {
    let mut out = String::from("const EFFECTS = ");
    out.push_str(&effects_map(tree, ir_path)?);
    out.push_str(";\n");
    out.push_str(
        r#"const HOST_CALLBACKS = {
  delay: function (ms) {
    return new Promise(function (resolve) { setTimeout(resolve, ms); });
  },
  fetch: async function (path, into) {
    try {
      const base = (globalThis.OPR_BASE_URL || "").replace(/\/$/, "");
      const response = await fetch(base + path);
      mirror[into] = await response.text();
    } catch (error) {
      mirror[into] = "error: request failed";
    }
  }
};

async function runEffects(id) {
  for (const effect of EFFECTS[id] || []) {
    if (effect.kind === "delay") {
      await HOST_CALLBACKS.delay(effect.ms);
    } else if (effect.kind === "fetch") {
      await HOST_CALLBACKS.fetch(effect.path, effect.into);
    }
  }
}

"#,
    );
    Ok(out)
}

/// The browser host: loads the `.wasm`, draws with Canvas 2D, forwards taps.
fn host_computed_source(
    states: &[EState],
    computed: &[Computed],
    ir_path: &str,
) -> DResult<String> {
    let mut body = String::new();
    for c in computed.iter().filter(|c| c.ty.as_str() != "int") {
        let (_, expr) = computed_expr(&c.expr, states, computed, ir_path, false, |st| {
            Ok(format!("mirror[{}]", js_str(&st.name)))
        })?;
        body.push_str(&format!("  mirror[{}] = {};\n", js_str(&c.name), expr));
    }
    if body.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("function applyComputed() {{\n{body}}}\n\n"))
    }
}

fn host_source(
    app: &str,
    states: &[EState],
    int_states: &[WasmState],
    computed: &[Computed],
    tree: &ENode,
    ir_path: &str,
    has_effects: bool,
) -> DResult<String> {
    let mut s = String::new();
    s.push_str(&format!(
        "// Generated by opuromc for app {app}; loads the wasm module and forwards events.\n"
    ));
    s.push_str("\"use strict\";\n");
    s.push_str("const canvas = document.getElementById(\"app\");\n");
    s.push_str("const ctx = canvas.getContext(\"2d\");\n");
    s.push_str("let wasm = null;\n");
    s.push_str("const mirror = {};\n");
    for st in states {
        let init = serde_json::to_string(&st.init).unwrap_or_else(|_| "null".to_string());
        s.push_str(&format!("mirror[{}] = {};\n", js_str(&st.name), init));
    }
    s.push_str("const STATE_NAMES = ");
    s.push_str(&json_array(int_states.iter().map(|s| s.name.clone())));
    s.push_str(";\n");
    s.push_str("const MIRROR_ACTIONS = ");
    s.push_str(&mirror_actions(tree, int_states));
    s.push_str(";\n");
    s.push_str("const TREE = ");
    s.push_str(&node_table(tree));
    s.push_str(";\n");
    s.push_str(concat!(
        "function textOf(value) {\n",
        "  return String(value).replace(/\\{([A-Za-z_][A-Za-z0-9_]*)\\}/g, function (_, name) {\n",
        "    if (name in mirror) { return String(mirror[name]); }\n",
        "    return \"\";\n",
        "  });\n",
        "}\n\n",
        "function intrinsic(node) {\n",
        "  if (node.kind === \"Text\") {\n",
        "    const size = node.size || 16;\n",
        "    return [String(node.value || \"\").length * size * 0.6 + 4, size * 1.5];\n",
        "  }\n",
        "  if (node.kind === \"Button\") {\n",
        "    return [String(node.label || \"\").length * 10 + 28, 32];\n",
        "  }\n",
        "  if (node.kind === \"Input\") { return [200, 32]; }\n",
        "  if (node.kind === \"Image\") { return [node.width || 100, node.height || 100]; }\n",
        "  return [0, 0];\n",
        "}\n\n",
        "function layout(node, x, y) {\n",
        "  const pad = node.padding || 0;\n",
        "  const gap = node.gap === undefined ? 4 : node.gap;\n",
        "  const kids = node.children || [];\n",
        "  const vertical = node.kind === \"Column\" || node.kind === \"List\" || node.kind === \"Card\";\n",
        "  const horizontal = node.kind === \"Row\" || node.kind === \"Container\";\n",
        "  if (kids.length > 0 && (vertical || horizontal)) {\n",
        "    let cx = x + pad, cy = y + pad, maxW = 0, maxH = 0;\n",
        "    for (const child of kids) {\n",
        "      const box = layout(child, cx, cy);\n",
        "      maxW = Math.max(maxW, box.w);\n",
        "      maxH = Math.max(maxH, box.h);\n",
        "      if (vertical) { cy += box.h + gap; } else { cx += box.w + gap; }\n",
        "    }\n",
        "    const w = vertical ? maxW + 2 * pad : cx - x - gap + pad;\n",
        "    const h = vertical ? cy - y - gap + pad : maxH + 2 * pad;\n",
        "    node.x = x; node.y = y; node.w = Math.max(w, 1); node.h = Math.max(h, 1);\n",
        "    return { w: node.w, h: node.h };\n",
        "  }\n",
        "  const size = intrinsic(node);\n",
        "  node.x = x; node.y = y; node.w = Math.max(size[0], 1); node.h = Math.max(size[1], 1);\n",
        "  return { w: node.w, h: node.h };\n",
        "}\n\n",
        "function render() {\n",
        "  layout(TREE, 16, 16);\n",
        "  ctx.clearRect(0, 0, canvas.width, canvas.height);\n",
        "  ctx.textBaseline = \"top\";\n",
        "  drawNode(TREE);\n",
        "}\n\n",
        "function drawNode(node) {\n",
        "  if (node.when && !mirror[node.when]) { return; }\n",
        "  if (node.unless && mirror[node.unless]) { return; }\n",
        "  const x = node.x, y = node.y, w = node.w, h = node.h;\n",
        "  if (node.kind === \"Text\") {\n",
        "    ctx.font = (node.size || 16) + \"px sans-serif\";\n",
        "    ctx.fillStyle = node.color || \"#111111\";\n",
        "    ctx.fillText(textOf(node.value || \"\"), x, y);\n",
        "  } else if (node.kind === \"Button\") {\n",
        "    ctx.fillStyle = \"#097FEB\";\n",
        "    ctx.fillRect(x, y, w, h);\n",
        "    ctx.fillStyle = \"#ffffff\";\n",
        "    ctx.font = \"16px sans-serif\";\n",
        "    ctx.fillText(node.label || \"\", x + 10, y + 8);\n",
        "  } else if (node.kind === \"Container\" || node.kind === \"Card\") {\n",
        "    ctx.fillStyle = node.color || \"#f1f5f9\";\n",
        "    ctx.fillRect(x, y, w, h);\n",
        "  }\n",
        "  for (const child of node.children || []) { drawNode(child); }\n",
        "}\n\n",
        "function syncStates() {\n",
        "  for (let i = 0; i < wasm.opu_state_len(); i++) {\n",
        "    const name = STATE_NAMES[i];\n",
        "    if (name) { mirror[name] = wasm.opu_state(i); }\n",
        "  }\n",
        "}\n\n",
        "function applyMirror(id) {\n",
        "  for (const action of MIRROR_ACTIONS[id] || []) {\n",
        "    const target = action.target;\n",
        "    if (action.op === \"toggle\") {\n",
        "      mirror[target] = !mirror[target];\n",
        "    } else if (action.op === \"set\") {\n",
        "      mirror[target] = action.valueFrom !== undefined ? mirror[action.valueFrom] : action.value;\n",
        "    } else if (action.op === \"add\") {\n",
        "      const from = action.valueFrom !== undefined ? mirror[action.valueFrom] : action.value;\n",
        "      mirror[target] = Number(mirror[target]) + Number(from);\n",
        "    } else if (action.op === \"sub\") {\n",
        "      const from = action.valueFrom !== undefined ? mirror[action.valueFrom] : action.value;\n",
        "      mirror[target] = Number(mirror[target]) - Number(from);\n",
        "    }\n",
        "  }\n",
        "}\n\n",
        "function applyTap(id) {\n",
        "  if (!wasm) { return 0; }\n",
        "  const patches = wasm.opu_tap(id);\n",
        "  applyMirror(id);\n",
        "  syncStates();\n",
        "  render();\n",
        "  return patches;\n",
        "}\n\n",
        "function applyInput(id, value) {\n",
        "  if (!wasm) { return 0; }\n",
        "  const n = parseInt(value, 10);\n",
        "  const patches = wasm.opu_input(id, isNaN(n) ? 0 : n);\n",
        "  applyMirror(id);\n",
        "  syncStates();\n",
        "  render();\n",
        "  return patches;\n",
        "}\n\n",
        "canvas.addEventListener(\"click\", function (event) {\n",
        "  const rect = canvas.getBoundingClientRect();\n",
        "  const x = event.clientX - rect.left;\n",
        "  const y = event.clientY - rect.top;\n",
        "  if (!wasm) { return; }\n",
        "  const hit = hitTest(TREE, x, y);\n",
        "  if (hit !== null) { applyTap(hit); }\n",
        "});\n\n",
        "function hitTest(node, x, y) {\n",
        "  for (const child of node.children || []) {\n",
        "    const found = hitTest(child, x, y);\n",
        "    if (found !== null) { return found; }\n",
        "  }\n",
        "  if (node.tap && x >= node.x && x <= node.x + node.w && y >= node.y && y <= node.y + node.h) {\n",
        "    return node.id;\n",
        "  }\n",
        "  return null;\n",
        "}\n\n",
        "async function boot() {\n",
        "  const bytes = await fetch(\"opurom_wasm.wasm\").then(function (r) { return r.arrayBuffer(); });\n",
        "  const module = await WebAssembly.instantiate(bytes, {});\n",
        "  wasm = module.instance.exports;\n",
        "  render();\n",
        "}\n\n",
        "boot();\n",
    ));
    let computed_fn = host_computed_source(states, computed, ir_path)?;
    if !computed_fn.is_empty() {
        s = s.replace(
            "function syncStates() {\n",
            &format!("{computed_fn}function syncStates() {{\n"),
        );
        s = s.replace(
            "  render();\n  return patches;",
            "  applyComputed();\n  render();\n  return patches;",
        );
        s = s.replace(
            "  wasm = module.instance.exports;\n  render();",
            "  wasm = module.instance.exports;\n  applyComputed();\n  render();",
        );
    }
    if has_effects {
        let mut replacement = host_effects(tree, ir_path)?;
        replacement.push_str("async function applyTap(id) {\n");
        s = s.replace("function applyTap(id) {\n", &replacement);
        s = s.replace(
            "  const patches = wasm.opu_tap(id);\n",
            "  await runEffects(id);\n  const patches = wasm.opu_tap(id);\n",
        );
    }
    Ok(s)
}

/// Actions that target a bool or string state; those live in the host mirror.
fn mirror_actions(tree: &ENode, states: &[WasmState]) -> String {
    let mut nodes = Vec::new();
    collect(tree, &mut nodes);
    let mut out = String::from("{\n");
    for n in &nodes {
        let mut entries = Vec::new();
        for event in ["tap", "input"] {
            let Some(action) = n.events.get(event) else {
                continue;
            };
            for one in event_actions(action) {
                let Some(op) = one.get("op").and_then(Json::as_str) else {
                    continue;
                };
                let Some(target) = one.get("target").and_then(Json::as_str) else {
                    continue;
                };
                if int_index(states, target).is_some() {
                    continue;
                }
                let mut item = format!(
                    "{{ op: {}, target: {}",
                    serde_json::to_string(op).unwrap_or_else(|_| "\"set\"".to_string()),
                    serde_json::to_string(target).unwrap_or_else(|_| "\"\"".to_string())
                );
                if let Some(from) = one.get("valueFrom").and_then(Json::as_str) {
                    item.push_str(&format!(
                        ", valueFrom: {}",
                        serde_json::to_string(from).unwrap_or_else(|_| "\"\"".to_string())
                    ));
                } else if let Some(v) = one.get("value") {
                    item.push_str(&format!(
                        ", value: {}",
                        serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
                    ));
                }
                item.push_str(" }");
                entries.push(item);
            }
        }
        if !entries.is_empty() {
            out.push_str(&format!("  {}: [{}],\n", n.id, entries.join(", ")));
        }
    }
    out.push('}');
    out
}

/// A JSON array of plain strings.
fn json_array(items: impl Iterator<Item = String>) -> String {
    let list: Vec<Json> = items.map(Json::String).collect();
    serde_json::to_string(&Json::Array(list)).unwrap_or_else(|_| "[]".to_string())
}

fn node_table(tree: &ENode) -> String {
    fn write_node(n: &ENode, out: &mut String, depth: usize) {
        let pad = "  ".repeat(depth);
        let mut props = String::new();
        for key in ["value", "label", "color", "title", "name"] {
            if let Some(Json::String(v)) = n.props.get(key) {
                props.push_str(&format!(
                    ", {}: {}",
                    key,
                    serde_json::to_string(v).unwrap_or_else(|_| "\"\"".to_string())
                ));
            }
        }
        for key in ["size", "width", "height", "padding", "gap"] {
            if let Some(Json::Number(v)) = n.props.get(key) {
                props.push_str(&format!(", {}: {v}", key));
            }
        }
        let when = match &n.when {
            Some(w) => format!(
                ", when: {}",
                serde_json::to_string(w).unwrap_or_else(|_| "\"\"".to_string())
            ),
            None => String::new(),
        };
        let unless = match &n.unless {
            Some(u) => format!(
                ", unless: {}",
                serde_json::to_string(u).unwrap_or_else(|_| "\"\"".to_string())
            ),
            None => String::new(),
        };
        let tap = if n.events.contains_key("tap") {
            ", tap: true"
        } else {
            ""
        };
        out.push_str(&format!(
            "{pad}{{ id: {}, kind: {}, x: 0, y: 0, w: 0, h: 0{props}{when}{unless}{tap}",
            n.id,
            serde_json::to_string(&n.kind).unwrap_or_else(|_| "\"Container\"".to_string())
        ));
        if n.children.is_empty() {
            out.push_str(" }");
        } else {
            out.push_str(", children: [\n");
            for (i, child) in n.children.iter().enumerate() {
                write_node(child, out, depth + 1);
                if i + 1 < n.children.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&format!("{pad}] }}"));
        }
    }
    let mut out = String::new();
    write_node(tree, &mut out, 0);
    out
}

fn index_html(app: &str) -> String {
    let title = app.replace('&', "&amp;").replace('"', "&quot;");
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>body {{ font-family: sans-serif; margin: 0; padding: 16px; }} canvas {{ background: #fff; border: 1px solid #ddd; }}</style>\n</head>\n<body>\n<canvas id=\"app\" width=\"800\" height=\"600\"></canvas>\n<script src=\"host.js\"></script>\n</body>\n</html>\n"
    )
}

fn readme(app: &str, has_effects: bool, computed: &[Computed]) -> String {
    let effects = if has_effects {
        "\n## Async effects\n\n`host.js` runs delay and fetch host callbacks before `opu_tap`. The wasm\nmodule has no imports; string state lives in the host mirror. This approximates\nlanguage-level async IO rather than putting an OS event loop inside wasm.\n"
    } else {
        ""
    };
    let computed_section = if computed.is_empty() {
        ""
    } else {
        "\n## Computed values\n\nInt computed values are recomputed inside the wasm module after events;\nbool and string computed values are recomputed in the `host.js` mirror. Lists\nare compile-time only; no runtime list mutation or OS async IO is emitted.\n"
    };
    format!(
        "# {app} - wasm target\n\nGenerated by `opuromc target <ir> --target wasm -o <dir>`.\n\n## Build\n\n```sh\ncargo build --release --target wasm32-unknown-unknown\n```\n\nThe module is written to `target/wasm32-unknown-unknown/release/opurom_wasm.wasm`.\n\n## Run\n\nServe this directory and open `index.html`; `host.js` loads the `.wasm` with\n`WebAssembly.instantiate(bytes, {{}})` and forwards taps through `opu_tap`.\n\n## ABI\n\n| Export | Meaning |\n| --- | --- |\n| `opu_version()` | ABI version, always 1 |\n| `opu_state_len()` | number of int states |\n| `opu_state(index)` | int state by declaration order, -1 when out of range |\n| `opu_tap(node_id)` | applies a tap chain, returns the patch count |\n| `opu_input(node_id, value)` | applies an input chain, returns the patch count |\n\nOnly int states live in the wasm module; bool and string states are mirrored in `host.js`.\n{effects}{computed_section}"
    )
}

pub fn emit_wasm(
    app: &str,
    states: &[EState],
    computed: &[Computed],
    tree: &ENode,
    out: &Path,
    ir_path: &str,
    has_effects: bool,
    font_stack: &str,
) -> DResult<()> {
    let int_states = read_int_states(states);
    let src = lib_source(app, states, &int_states, computed, tree, ir_path)?;
    crate::emit::write(out, "Cargo.toml", &manifest(), ir_path)?;
    crate::emit::write(out, "src/lib.rs", &src, ir_path)?;
    // Resolve the platform-adaptive stack for the host page and canvas text.
    let page = index_html(app).replace(
        "font-family: sans-serif",
        &format!("font-family: {font_stack}"),
    );
    crate::emit::write(out, "index.html", &page, ir_path)?;
    crate::emit::write(
        out,
        "host.js",
        &host_source(
            app,
            states,
            &int_states,
            computed,
            tree,
            ir_path,
            has_effects,
        )?
        .replace("sans-serif", font_stack),
        ir_path,
    )?;
    crate::emit::write(
        out,
        "README.md",
        &readme(app, has_effects, computed),
        ir_path,
    )?;
    Ok(())
}
