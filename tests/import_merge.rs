//! Cross-module import merge gate.
//!
//! Flutter's dependency parser deduplicates path lists before building a depfile
//! (`packages/flutter_tools/lib/src/build_system/depfile.dart:116-118`,
//! Flutter 3.50.0). Opurom goes further for declarations: the same module may be
//! imported twice, but two different modules cannot declare the same state,
//! model, API path, component or theme. This gate pins the merge order, the
//! conflict diagnostics and the fact that every rejected program writes no IR.

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

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = out_dir(label);
        fs::create_dir_all(&root).expect("create project dir");
        fs::write(
            root.join("opurom.json"),
            "{\"name\":\"merge-fixture\",\"version\":\"0.1.0\",\"moduleRoots\":[\"modules\"],\"dependencies\":{}}",
        )
        .expect("write manifest");
        fs::create_dir_all(root.join("modules")).expect("create module root");
        Self { root }
    }

    fn write(&self, name: &str, source: &str) {
        fs::write(self.root.join(name), source).expect("write module source");
    }

    fn compile(&self, name: &str, out: &PathBuf) -> std::process::Output {
        bin()
            .arg("compile")
            .arg(self.root.join(name))
            .arg("-o")
            .arg(out)
            .output()
            .expect("run opuromc compile")
    }

    fn check(&self, name: &str) -> String {
        let result = bin()
            .arg("check")
            .arg(self.root.join(name))
            .output()
            .expect("run opuromc check");
        assert_eq!(result.status.code(), Some(1), "source must be rejected");
        String::from_utf8_lossy(&result.stderr).into_owned()
    }
}

#[test]
fn module_merge_order_is_import_order_with_main_states_last() {
    let project = Project::new("import-merge-ok");
    project.write(
        "one.opu",
        "state a = 1\nmodel M1 {\n  id: int primary\n}\napi \"/one\" {\n  list M1\n}\ncomponent CardOne(title: string) {\n  Text(title)\n}\n",
    );
    project.write(
        "two.opu",
        "state b = 2\nmodel M2 {\n  id: int primary\n}\napi \"/two\" {\n  list M2\n}\ncomponent CardTwo(title: string) {\n  Text(title)\n}\n",
    );
    project.write(
        "main.opu",
        "import \"one.opu\"\nimport \"two.opu\"\n\napp Two {\n  state local = 3\n  Text(\"{a} {b} {local}\")\n}\n",
    );
    let out = project.root.join("two.opuir.json");
    let result = project.compile("main.opu", &out);
    assert!(
        result.status.success(),
        "multi-module compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
    let states: Vec<&str> = doc["state"]
        .as_array()
        .expect("state array")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(states, vec!["a", "b", "local"]);
    let models: Vec<&str> = doc["models"]
        .as_array()
        .expect("models array")
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert_eq!(models, vec!["M1", "M2"]);
    let apis: Vec<&str> = doc["api"]
        .as_array()
        .expect("api array")
        .iter()
        .filter_map(|a| a["path"].as_str())
        .collect();
    assert_eq!(apis, vec!["/one", "/two"]);
}

#[test]
fn repeated_import_is_idempotent() {
    let project = Project::new("import-repeat");
    project.write("dup.opu", "state dup = 1\n");
    project.write(
        "main.opu",
        "import \"dup.opu\"\nimport \"dup.opu\"\n\napp Dup {\n  Text(\"{dup}\")\n}\n",
    );
    let out = project.root.join("dup.opuir.json");
    let result = project.compile("main.opu", &out);
    assert!(
        result.status.success(),
        "repeated import failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
    let states: Vec<&str> = doc["state"]
        .as_array()
        .expect("state array")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(states, vec!["dup"]);
}

#[test]
fn duplicate_names_conflict_across_modules_and_with_main() {
    let project = Project::new("import-duplicates");
    project.write("shared.opu", "state shared = 0\n");
    project.write("other.opu", "state shared = 0\n");
    project.write(
        "cross.opu",
        "import \"shared.opu\"\nimport \"other.opu\"\n\napp Cross {\n  Text(\"{shared}\")\n}\n",
    );
    let stderr = project.check("cross.opu");
    assert!(
        stderr.contains("duplicate imported name 'shared'"),
        "cross-module conflict: {stderr}"
    );

    project.write("local.opu", "state local = 9\n");
    project.write(
        "main.opu",
        "import \"local.opu\"\n\napp Main {\n  state local = 1\n  Text(\"{local}\")\n}\n",
    );
    let stderr = project.check("main.opu");
    assert!(
        stderr.contains("duplicate imported name 'local'"),
        "main conflict: {stderr}"
    );
}

#[test]
fn duplicate_models_apis_and_components_conflict() {
    let project = Project::new("import-conflicts");
    project.write(
        "model_a.opu",
        "model M {\n  id: int primary\n}\napi \"/same\" {\n  list M\n}\ncomponent Card(text: string) {\n  Text(text)\n}\n",
    );
    project.write(
        "model_b.opu",
        "model M {\n  id: int primary\n}\napi \"/same\" {\n  list M\n}\ncomponent Card(text: string) {\n  Text(text)\n}\n",
    );
    project.write(
        "main.opu",
        "import \"model_a.opu\"\nimport \"model_b.opu\"\n\napp Conflicts {\n  Column {\n    Card(\"x\")\n  }\n}\n",
    );
    // The merge stops at the first conflict: a duplicate model prevents the
    // later API and component comparisons from running, so the single
    // positioned diagnostic is the stable contract.
    let stderr = project.check("main.opu");
    assert!(
        stderr.contains("duplicate imported name 'M'"),
        "model conflict must be reported: {stderr}"
    );
    assert!(
        !stderr.contains("duplicate imported name '/same'"),
        "merge must stop at the first conflict: {stderr}"
    );
}

#[test]
fn theme_is_single_source_across_modules_and_main() {
    let project = Project::new("import-theme");
    project.write("theme_a.opu", "theme { mode: dark }\n");
    project.write("theme_b.opu", "theme { mode: light }\n");
    project.write(
        "multi.opu",
        "import \"theme_a.opu\"\nimport \"theme_b.opu\"\n\napp Themes {\n  Text(\"x\")\n}\n",
    );
    let stderr = project.check("multi.opu");
    assert!(
        stderr.contains("duplicate imported name 'theme'"),
        "multi-theme conflict: {stderr}"
    );

    project.write(
        "main.opu",
        "import \"theme_a.opu\"\n\napp Main {\n  theme { mode: light }\n  Text(\"x\")\n}\n",
    );
    let stderr = project.check("main.opu");
    assert!(
        stderr.contains("duplicate imported name 'theme'"),
        "main-theme conflict: {stderr}"
    );
}

#[test]
fn conflicting_imports_write_no_ir() {
    let project = Project::new("import-no-artifact");
    project.write("a.opu", "state shared = 0\n");
    project.write("b.opu", "state shared = 0\n");
    project.write(
        "main.opu",
        "import \"a.opu\"\nimport \"b.opu\"\n\napp Bad {\n  Text(\"{shared}\")\n}\n",
    );
    let out = project.root.join("bad.opuir.json");
    let result = project.compile("main.opu", &out);
    assert_eq!(result.status.code(), Some(1), "conflict must fail");
    assert!(!out.exists(), "rejected merge must not write IR");
}
