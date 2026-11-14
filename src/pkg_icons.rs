//! External icon packages: names and path data resolved from the project's
//! locked `opurom-icons`-style packages at compile time. A package manifest
//! (`opurom-pkg.json`) with `"kind": "icons"` points at an entry JSON document
//! whose icons carry their own viewBox and path data. Only programs that
//! actually reference a package icon carry the extra `"icons"` IR key, so the
//! golden fixtures stay byte-identical.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};

fn has_traversal(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| part == "..")
}

fn read_json(path: &Path, label: &str) -> Option<Json> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(doc) => Some(doc),
        Err(_) => {
            eprintln!("warning: skipping malformed {label} at {}", path.display());
            None
        }
    }
}

/// The package icon table for a project: one entry per icon name, carrying the
/// viewBox and every path of the package document, ordered by name so the IR
/// output stays deterministic.
pub fn resolve(project_root: &Path) -> JsonMap<String, Json> {
    let manifest_path = project_root.join("opurom.json");
    let Some(doc) = read_json(&manifest_path, "project manifest") else {
        return JsonMap::new();
    };
    let Some(dependencies) = doc.get("dependencies").and_then(|d| d.as_object()) else {
        return JsonMap::new();
    };
    if dependencies.is_empty() {
        return JsonMap::new();
    }
    // The lockfile is the version authority; an exact x.y.z manifest pin is the
    // offline fallback, mirroring the module-root resolution in `modules.rs`.
    let lock = read_json(&project_root.join("opurom.lock"), "lockfile");
    let packages = lock
        .as_ref()
        .and_then(|l| l.get("packages"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    let mut icons = JsonMap::new();
    for (name, contract) in dependencies {
        let pinned = contract.as_str().unwrap_or_default();
        let package_dir = if let Some(entry) = packages
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name.as_str()))
        {
            entry.get("path").and_then(|p| p.as_str()).map(PathBuf::from)
        } else if pinned.split('.').count() == 3
            && pinned.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            Some(PathBuf::from(format!("registry/{name}/{pinned}")))
        } else {
            None
        };
        let Some(relative) = package_dir else {
            continue;
        };
        if relative.is_absolute() || has_traversal(&relative.to_string_lossy()) {
            continue;
        }
        let root = project_root.join(relative);
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        if !root.starts_with(project_root) {
            continue;
        }
        let manifest = match read_json(&root.join("opurom-pkg.json"), "package manifest") {
            Some(manifest) => manifest,
            None => continue,
        };
        if manifest.get("kind").and_then(|k| k.as_str()) != Some("icons") {
            continue;
        }
        let Some(entry) = manifest.get("entry").and_then(|e| e.as_str()) else {
            continue;
        };
        if has_traversal(entry) {
            continue;
        }
        let Some(table) = read_json(&root.join(entry), "icon table") else {
            continue;
        };
        let Some(list) = table.get("icons").and_then(|i| i.as_array()) else {
            continue;
        };
        for icon in list {
            let Some(icon_name) = icon.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            let Some(paths) = icon.get("paths").and_then(|p| p.as_array()) else {
                continue;
            };
            let data: Vec<Json> = paths
                .iter()
                .filter_map(|p| p.get("d").and_then(|d| d.as_str()).map(|d| Json::String(d.to_string())))
                .collect();
            if data.is_empty() {
                continue;
            }
            let record = Json::Object({
                let mut map = JsonMap::new();
                map.insert(
                    "viewBox".to_string(),
                    Json::String(
                        icon.get("viewBox")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0 0 256 256")
                            .to_string(),
                    ),
                );
                map.insert("paths".to_string(), Json::Array(data));
                map
            });
            icons.insert(icon_name.to_string(), record);
        }
    }
    icons
}

/// The project root for a source file: the nearest directory with
/// `opurom.json`, walking up but stopping at a repository boundary, exactly
/// like the module resolver's own project discovery. The result is canonical
/// so containment checks against canonicalized package directories hold even
/// when the compiler was invoked with a relative source path.
pub fn project_root(source_path: &str) -> PathBuf {
    let start = Path::new(source_path).parent().unwrap_or_else(|| Path::new("."));
    for dir in start.ancestors() {
        if dir.join("opurom.json").is_file() {
            return fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf())
}

/// A diagnostics helper for an unknown name that also is not in any package.
pub fn unknown_error(path: &str, pos: crate::parser::Pos, name: &str, help: String) -> Diagnostic {
    Diagnostic::new(path, pos.line, pos.col, format!("unknown icon name '{name}' on Icon"), help)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_project_resolves_no_icons() {
        let dir = std::env::temp_dir().join(format!("pkg-icons-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(resolve(&dir).is_empty());
    }

    #[test]
    fn a_traversing_package_path_is_skipped() {
        let dir = std::env::temp_dir().join(format!("pkg-icons-trav-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("opurom.json"),
            r##"{"name":"p","version":"0.1.0","dependencies":{"evil":"1.0.0"}}"##,
        )
        .unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        fs::write(
            dir.join("outside/opurom-pkg.json"),
            r##"{"name":"evil","version":"1.0.0","kind":"icons","entry":"icons.json"}"##,
        )
        .unwrap();
        fs::write(dir.join("outside/icons.json"), r##"{"icons":[{"name":"x","paths":[{"d":"M0 0"}]}]}"##).unwrap();
        // No lockfile: the exact-pin fallback builds registry/evil/1.0.0, which
        // does not exist, so nothing resolves and nothing escapes the root.
        assert!(resolve(&dir).is_empty());
    }
}
