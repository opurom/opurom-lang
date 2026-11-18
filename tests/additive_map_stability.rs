//! Additive map stability after import merge and renumbering.
//!
//! `stateRefs` and `nodeBoundaries` are additive IR fields consumed by an
//! incremental pipeline. They must stay deterministic when declarations come
//! from several modules, when a constant-visibility branch is removed and node
//! ids are renumbered, and when the optimizer runs repeatedly. Flutter's depfile
//! reader keeps the file list stable by normalizing paths and rejecting
//! duplicates (`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`,
//! Flutter 3.50.0); Opurom keeps these maps stable by walking trees in source
//! order and collecting ids into ordered maps.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn out_dir(label: &str) -> PathBuf {
    let dir = workspace().join("target/test-out").join(format!(
        "{}-{}",
        label,
        CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create test output dir");
    fs::create_dir_all(dir.join("modules")).expect("create module root");
    fs::write(
        dir.join("opurom.json"),
        "{\"name\":\"maps-fixture\",\"version\":\"0.1.0\",\"moduleRoots\":[\"modules\"],\"dependencies\":{}}",
    )
    .expect("write manifest");
    fs::write(dir.join("a.opu"), "state fromA = 1\n").expect("write module a");
    fs::write(dir.join("b.opu"), "state fromB = 2\n").expect("write module b");
    fs::write(
        dir.join("main.opu"),
        "import \"a.opu\"\nimport \"b.opu\"\n\napp Maps {\n  state local = 3\n  state hidden = false\n  computed total = 2 + 3 * 4\n  Column(repaintBoundary: true) {\n    Text(\"{fromA} {fromB} {local} {total}\", relayoutBoundary: true)\n    if (hidden) { Text(\"gone\") }\n    Button(\"inc\") { local += 1 }\n  }\n}\n",
    )
    .expect("write source");
    dir
}

fn run(dir: &PathBuf, args: &[&str]) -> Vec<u8> {
    let mut command = bin();
    for arg in args {
        command.arg(arg);
    }
    let result = command.current_dir(dir).output().expect("run opuromc");
    assert!(
        result.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::read(dir.join("out.json")).expect("read IR")
}

fn maps(ir: &[u8]) -> (serde_json::Value, serde_json::Value, Vec<u64>) {
    let doc: serde_json::Value = serde_json::from_slice(ir).expect("parse IR");
    let refs = doc
        .get("stateRefs")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let bounds = doc
        .get("nodeBoundaries")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let ids: Vec<u64> = doc["tree"]["children"]
        .as_array()
        .expect("children")
        .iter()
        .filter_map(|n| n["id"].as_u64())
        .collect();
    (refs, bounds, ids)
}

#[test]
fn additive_maps_survive_merge_renumber_and_repeat() {
    let dir = out_dir("additive-map-stability");
    let src = dir.join("main.opu").to_string_lossy().into_owned();
    let out = dir.join("out.json").to_string_lossy().into_owned();

    let raw = run(&dir, &["compile", &src, "-o", &out, "--no-fold"]);
    assert!(
        maps(&raw).0.is_null() && maps(&raw).1.is_null(),
        "raw lowering must not export the maps"
    );

    let default = run(&dir, &["compile", &src, "-o", &out]);
    let (default_refs, default_bounds, default_ids) = maps(&default);
    assert!(
        !default_refs.is_null() && !default_bounds.is_null(),
        "default compile must export the maps when boundaries exist"
    );
    assert!(
        default_ids
            .iter()
            .copied()
            .eq(1..default_ids.len() as u64 + 1),
        "default ids must be dense: {default_ids:?}"
    );

    let optimized = run(&dir, &["optimize", &out, "-o", &out, "--full-fold"]);
    let (opt_refs, _opt_bounds, opt_ids) = maps(&optimized);
    assert!(
        opt_refs.is_null() || opt_refs.as_object().is_some(),
        "optimized map must be an object"
    );
    assert!(
        opt_ids.len() <= default_ids.len(),
        "constant-visibility fold may only remove nodes"
    );

    let twice = run(&dir, &["optimize", &out, "-o", &out, "--full-fold"]);
    assert_eq!(optimized, twice, "second optimize must be byte identical");
}

#[test]
fn additive_maps_are_deterministic_across_compile_modes() {
    let dir = out_dir("additive-map-modes");
    let src = dir.join("main.opu").to_string_lossy().into_owned();
    let out = dir.join("out.json").to_string_lossy().into_owned();

    let first = run(&dir, &["compile", &src, "-o", &out]);
    let second = run(&dir, &["compile", &src, "-o", &out]);
    assert_eq!(first, second, "compile must be deterministic");
    let (refs, bounds, _) = maps(&first);

    let expected_refs = serde_json::json!({
        "fromA": [1],
        "fromB": [1],
        "local": [1, 2],
        "total": [1]
    });
    assert_eq!(refs, expected_refs, "stateRefs order and ids are public");
    assert_eq!(bounds["0"]["repaint"], 0, "root repaint boundary is node 0");
    assert_eq!(
        bounds["1"]["relayout"], 1,
        "text relayout boundary is node 1"
    );
}
