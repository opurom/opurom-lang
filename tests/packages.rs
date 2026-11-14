//! External package imports: `opurom.lock` entries contribute module roots, so
//! `import "theme/default.opu"` resolves inside an installed package without
//! any new language syntax. The golden fixtures never have a lockfile, so the
//! default path is unchanged.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const OpuromC: &str = env!("CARGO_BIN_EXE_opuromc");

fn unique(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opurom-packages-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn compile(dir: &Path, file: &str) -> Result<Value, String> {
    let out = dir.join("out.opuir.json");
    let result = Command::new(OpuromC)
        .args(["compile", file, "-o", out.to_str().unwrap()])
        .current_dir(dir)
        .output()
        .unwrap();
    if result.status.success() {
        Ok(serde_json::from_slice(&result.stdout).unwrap_or_else(|_| {
            serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap()
        }))
    } else {
        Err(String::from_utf8_lossy(&result.stderr).to_string())
    }
}

const MANIFEST: &str = r##"{
  "name": "sample-app",
  "version": "0.1.0",
  "moduleRoots": ["registry"],
  "dependencies": {
    "opurom-ui": "0.1.0"
  }
}"##;

const LOCK: &str = r##"{
  "version": 1,
  "manifestSha256": "0",
  "packages": [
    {
      "name": "opurom-ui",
      "version": "0.1.0",
      "path": "registry/opurom-ui/0.1.0",
      "sha256": "0",
      "files": [
        {
          "path": "registry/opurom-ui/0.1.0/opurom-pkg.json",
          "sha256": "0"
        },
        {
          "path": "registry/opurom-ui/0.1.0/theme/default.opu",
          "sha256": "0"
        }
      ]
    }
  ]
}"##;

const THEME_MODULE: &str = r##"theme {
  mode: light,
  accent: "#007AFF",
  radius: 12,
  fontScale: 1.0,
  surface: "#FFFFFF",
  onSurface: "#111827",
  stage: "#F9FAFB",
}
"##;

fn package_project(tag: &str, main: &str) -> PathBuf {
    let dir = unique(tag);
    write(&dir.join("opurom.json"), MANIFEST);
    write(&dir.join("opurom.lock"), LOCK);
    write(
        &dir.join("registry/opurom-ui/0.1.0/opurom-pkg.json"),
        r##"{"name":"opurom-ui","version":"0.1.0","files":["opurom-pkg.json","theme/default.opu"]}"##,
    );
    write(&dir.join("registry/opurom-ui/0.1.0/theme/default.opu"), THEME_MODULE);
    write(&dir.join("main.opu"), main);
    dir
}

#[test]
fn a_locked_package_contributes_module_roots() {
    let dir = package_project(
        "roots",
        r#"import "theme/default.opu";

app Packages {
  Text("hello")
}
"#,
    );
    let ir = compile(&dir, "main.opu").expect("package import must compile");
    assert_eq!(
        ir["theme"],
        json!({
            "mode": "light",
            "accent": "#007AFF",
            "radius": 12,
            "fontScale": 1.0,
            "tokens": {
                "surface": "#FFFFFF",
                "onSurface": "#111827",
                "stage": "#F9FAFB",
            },
        })
    );
}

#[test]
fn a_missing_lockfile_leaves_plain_module_roots_working() {
    let dir = unique("no-lock");
    write(&dir.join("opurom.json"), MANIFEST);
    write(&dir.join("registry/shared.opu"), "state shared = \"from-root\"\n");
    write(
        &dir.join("main.opu"),
        r#"import "registry/shared.opu";

app NoLock {
  Text("{shared}")
}
"#,
    );
    let ir = compile(&dir, "main.opu").expect("moduleRoots must keep working without a lock");
    assert_eq!(state_value(&ir, "shared"), json!("from-root"));
}

#[test]
fn an_uninstalled_dependency_is_not_a_module_root() {
    let dir = package_project(
        "uninstalled",
        r#"import "theme/default.opu";

app Missing {
  Text("hi")
}
"#,
    );
    // The lockfile lists opurom-ui, but the registry directory is removed.
    fs::remove_dir_all(dir.join("registry")).unwrap();
    let error = compile(&dir, "main.opu").expect_err("the import must fail cleanly");
    assert!(error.contains("cannot read imported module"), "{error}");
}

#[test]
fn a_lock_entry_escaping_the_project_root_is_rejected() {
    let dir = package_project(
        "escape",
        r#"import "theme/default.opu";

app Escape {
  Text("hi")
}
"#,
    );
    let lock = dir.join("opurom.lock");
    fs::write(&lock, LOCK.replace("registry/opurom-ui/0.1.0", "../../escape-pkg")).unwrap();
    let error = compile(&dir, "main.opu").expect_err("an escaping lock path must be rejected");
    // The traversal guard fires before the containment check, so the
    // diagnostic names the unsafe path form either way.
    assert!(error.contains("may not be absolute or contain"), "{error}");
}

fn state_value(ir: &Value, name: &str) -> Value {
    ir["state"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .map(|s| s["init"].clone())
        .unwrap_or(Value::Null)
}
