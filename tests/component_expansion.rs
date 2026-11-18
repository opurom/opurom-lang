//! Component expansion depth and cross-module determinism gate.
//!
//! Flutter elements mount recursively with a parent pointer and traverse
//! children in tree order (`packages/flutter_tools/lib/src/widgets/framework.dart:4346-4354`,
//! `:3905-3911`, Flutter 3.50.0). Opurom expands components at compile time,
//! so the same tree must be produced regardless of module import order. This
//! gate pins the self-reference rejection, an acyclic chain's deterministic
//! expansion, and the practical recursion boundary observed when a chain of
//! component declarations grows without a cycle.

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
    dir
}

fn check(src: &PathBuf) -> String {
    let result = bin()
        .arg("check")
        .arg(src)
        .output()
        .expect("run opuromc check");
    assert_eq!(result.status.code(), Some(1), "source must be rejected");
    String::from_utf8_lossy(&result.stderr).into_owned()
}

fn compile(src: &PathBuf, out: &PathBuf) -> bool {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
        .status
        .success()
}

#[test]
fn self_reference_and_mutual_cycles_are_rejected() {
    let dir = out_dir("component-cycle");
    let self_ref = dir.join("self.opu");
    let self_ir = dir.join("self.opuir.json");
    fs::write(
        &self_ref,
        "component SelfRef(x: string) {\n  SelfRef(x)\n}\napp Bad { SelfRef(\"x\") }\n",
    )
    .expect("write self reference");
    let stderr = check(&self_ref);
    assert!(
        stderr.contains("component cycle detected: SelfRef -> SelfRef")
            && stderr.contains("components expand at compile time"),
        "self-reference diagnostic: {stderr}"
    );
    assert!(!self_ir.exists(), "rejected cycle must not write IR");

    let mutual = dir.join("mutual.opu");
    let mutual_ir = dir.join("mutual.opuir.json");
    fs::write(
        &mutual,
        "component Ping(x: string) {\n  Pong(x)\n}\ncomponent Pong(x: string) {\n  Ping(x)\n}\napp Bad { Ping(\"x\") }\n",
    )
    .expect("write mutual reference");
    let stderr = check(&mutual);
    assert!(
        stderr.contains("component cycle detected: Ping -> Pong -> Ping"),
        "mutual-cycle diagnostic: {stderr}"
    );
    assert!(
        !mutual_ir.exists(),
        "rejected mutual cycle must not write IR"
    );
}

#[test]
fn deep_acyclic_chain_expands_deterministically() {
    let dir = out_dir("component-depth");
    let src = dir.join("chain.opu");
    let first = dir.join("first.opuir.json");
    let second = dir.join("second.opuir.json");
    const DEPTH: usize = 200;
    let mut lines = Vec::new();
    for i in 0..DEPTH {
        lines.push(format!("component C{i}(x: string) {{"));
        if i + 1 < DEPTH {
            lines.push(format!("  C{}(x)", i + 1));
        } else {
            lines.push("  Text(x)".to_string());
        }
        lines.push("}".to_string());
    }
    lines.push("app Deep { C0(\"leaf\") }".to_string());
    fs::write(&src, format!("{}\n", lines.join("\n"))).expect("write chain");
    for out in [&first, &second] {
        assert!(
            compile(&src, out),
            "deep acyclic chain must compile: {}",
            String::from_utf8_lossy(&fs::read(out).unwrap_or_default())
        );
    }
    assert_eq!(
        fs::read(&first).expect("read first IR"),
        fs::read(&second).expect("read second IR"),
        "component expansion must be deterministic"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&first).expect("read IR")).expect("parse IR");
    assert_eq!(
        doc["tree"]["kind"], "Text",
        "single-child expansion keeps the primitive at the root"
    );
    let leaf = doc["tree"]["props"]["value"].as_str().expect("leaf text");
    assert_eq!(leaf, "leaf", "parameter substitution must reach the leaf");
}

#[test]
fn cross_module_component_expansion_follows_import_order() {
    let dir = out_dir("component-modules");
    fs::create_dir_all(dir.join("modules")).expect("create module root");
    fs::write(
        dir.join("opurom.json"),
        "{\"name\":\"component-fixture\",\"version\":\"0.1.0\",\"moduleRoots\":[\"modules\"],\"dependencies\":{}}",
    )
    .expect("write manifest");
    fs::write(
        dir.join("a.opu"),
        "component Leaf(text: string) {\n  Text(text)\n}\n",
    )
    .expect("write module a");
    fs::write(
        dir.join("b.opu"),
        "component Mid(text: string) {\n  Column {\n    Leaf(text)\n  }\n}\n",
    )
    .expect("write module b");
    fs::write(
        dir.join("main.opu"),
        "import \"b.opu\"\nimport \"a.opu\"\n\napp Modules {\n  Mid(\"hello\")\n}\n",
    )
    .expect("write source");
    let first = dir.join("first.opuir.json");
    let second = dir.join("second.opuir.json");
    for out in [&first, &second] {
        assert!(
            compile(&dir.join("main.opu"), out),
            "cross-module component must compile: {}",
            String::from_utf8_lossy(&fs::read(out).unwrap_or_default())
        );
    }
    assert_eq!(
        fs::read(&first).expect("read first IR"),
        fs::read(&second).expect("read second IR"),
        "cross-module expansion must be deterministic"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&first).expect("read IR")).expect("parse IR");
    assert_eq!(doc["tree"]["kind"], "Column");
    assert_eq!(doc["tree"]["children"][0]["kind"], "Text");
    assert_eq!(doc["tree"]["children"][0]["props"]["value"], "hello");
}

#[test]
fn extreme_chain_boundary_is_replayable() {
    let dir = out_dir("component-depth-boundary");
    for depth in [200, 1000] {
        let src = dir.join(format!("chain-{depth}.opu"));
        let out = dir.join(format!("chain-{depth}.opuir.json"));
        let mut lines = Vec::new();
        for i in 0..depth {
            lines.push(format!("component C{i}(x: string) {{"));
            if i + 1 < depth {
                lines.push(format!("  C{}(x)", i + 1));
            } else {
                lines.push("  Text(x)".to_string());
            }
            lines.push("}".to_string());
        }
        lines.push(format!("app Deep {{ C0(\"leaf\") }}"));
        fs::write(&src, format!("{}\n", lines.join("\n"))).expect("write boundary source");
        let result = bin()
            .arg("compile")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run opuromc compile");
        let stderr = String::from_utf8_lossy(&result.stderr);
        if depth <= 256 {
            assert!(
                result.status.success(),
                "depth {depth} must stay inside the guard: {stderr}"
            );
        } else {
            assert_eq!(
                result.status.code(),
                Some(1),
                "depth {depth} must fail with a diagnostic"
            );
            assert!(
                stderr.contains("component expansion depth exceeds 256"),
                "depth diagnostic: {stderr}"
            );
            assert!(!out.exists(), "depth {depth} must not write IR");
        }
    }
}
