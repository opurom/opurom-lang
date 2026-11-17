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
            entry
                .get("path")
                .and_then(|p| p.as_str())
                .map(PathBuf::from)
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
                .filter_map(|p| {
                    p.get("d")
                        .and_then(|d| d.as_str())
                        .map(|d| Json::String(d.to_string()))
                })
                .collect();
            if data.is_empty() {
                continue;
            }
            let mut record = JsonMap::new();
            record.insert(
                "viewBox".to_string(),
                Json::String(
                    icon.get("viewBox")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0 0 256 256")
                        .to_string(),
                ),
            );
            record.insert("paths".to_string(), Json::Array(data));
            // Non-outline artwork stays in the in-memory table so lowering can
            // bake the requested style into `paths` without growing the IR.
            let mut styles = JsonMap::new();
            if let Some(defined) = icon.get("styles").and_then(Json::as_object) {
                for (style, entries) in defined {
                    if style == "outline" {
                        continue;
                    }
                    let ds: Vec<Json> = entries
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| {
                            p.get("d")
                                .and_then(|d| d.as_str())
                                .map(|d| Json::String(d.to_string()))
                        })
                        .collect();
                    if !ds.is_empty() {
                        styles.insert(style.clone(), Json::Array(ds));
                    }
                }
            }
            if !styles.is_empty() {
                record.insert("styles".to_string(), Json::Object(styles));
            }
            icons.insert(icon_name.to_string(), Json::Object(record));
        }
    }
    icons
}

/// Resolve one Icon use into the exact path array the IR will carry. Style and
/// RTL are baked at compile time so consumers keep their single name lookup.
/// A missing style degrades to outlined with a warning, never a build failure.
pub fn variant_paths(name: &str, record: &Json, style: &str, flip: bool) -> Vec<Json> {
    let outline = || {
        record
            .get("paths")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let paths = if style == "outlined" {
        outline()
    } else {
        match record
            .get("styles")
            .and_then(|s| s.get(style))
            .and_then(Json::as_array)
        {
            Some(found) => found.clone(),
            None => {
                eprintln!("warning: icon '{name}' has no {style} artwork; outlined is used");
                outline()
            }
        }
    };
    if !flip {
        return paths;
    }
    let width = record
        .get("viewBox")
        .and_then(Json::as_str)
        .and_then(|v| v.split_whitespace().nth(2))
        .and_then(|w| w.parse::<f64>().ok())
        .unwrap_or(24.0);
    paths
        .iter()
        .map(|p| match p.as_str() {
            Some(d) => Json::String(mirror_path_d(d, width)),
            None => p.clone(),
        })
        .collect()
}

/// Mirror an SVG path's x coordinates inside `width`. Only numbers are
/// rewritten: relative x deltas are negated, absolute ones reflected, and an
/// arc's sweep flag flips. The node tree is untouched.
pub fn mirror_path_d(d: &str, width: f64) -> String {
    let bytes = d.as_bytes();
    let mut tokens: Vec<(bool, f64, usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() || b == b',' {
            i += 1;
            continue;
        }
        if b.is_ascii_alphabetic() {
            tokens.push((false, 0.0, i, i + 1));
            i += 1;
            continue;
        }
        let start = i;
        if b == b'+' || b == b'-' {
            i += 1;
        }
        let mut seen = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            seen = true;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                seen = true;
            }
        }
        if seen && i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if !seen {
            return d.to_string();
        }
        let Ok(value) = d[start..i].parse::<f64>() else {
            return d.to_string();
        };
        tokens.push((true, value, start, i));
    }
    let mut out = String::with_capacity(d.len());
    let mut cursor = 0usize;
    let mut cmd = ' ';
    let mut arg = 0usize;
    for (is_number, value, start, end) in tokens {
        if !is_number {
            cmd = d[start..end].chars().next().unwrap_or(' ');
            arg = 0;
            continue;
        }
        let slot = arg;
        arg += 1;
        let lower = cmd.to_ascii_lowercase();
        let group = match lower {
            'm' | 'l' | 't' => 2,
            'h' | 'v' => 1,
            'c' => 6,
            's' | 'q' => 4,
            'a' => 7,
            _ => 0,
        };
        let at = if group == 0 { slot } else { slot % group };
        let x_slot = match lower {
            'm' | 'l' | 't' | 'h' => at == 0,
            'c' => at == 0 || at == 2 || at == 4,
            's' | 'q' => at == 0 || at == 2,
            'a' => at == 5,
            _ => false,
        };
        let replacement = if lower == 'a' && at == 4 {
            Some(if value == 0.0 { "1" } else { "0" }.to_string())
        } else if x_slot {
            let mapped = if cmd.is_ascii_uppercase() {
                width - value
            } else {
                -value
            };
            Some(format_coord(mapped))
        } else {
            None
        };
        if let Some(text) = replacement {
            out.push_str(&d[cursor..start]);
            // Two adjacent numbers may lose their sign separator when mirrored
            // (e.g. `7-7` -> `7 7`); keep them from fusing into `77`.
            if out.ends_with(|c: char| c.is_ascii_digit())
                && text.starts_with(|c: char| c.is_ascii_digit())
            {
                out.push(' ');
            }
            out.push_str(&text);
            cursor = end;
        }
    }
    out.push_str(&d[cursor..]);
    out
}

fn format_coord(value: f64) -> String {
    if value.abs() < 0.00005 {
        return "0".to_string();
    }
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_string()
}

/// The project root for a source file: the nearest directory with
/// `opurom.json`, walking up but stopping at a repository boundary, exactly
/// like the module resolver's own project discovery. The result is canonical
/// so containment checks against canonicalized package directories hold even
/// when the compiler was invoked with a relative source path.
pub fn project_root(source_path: &str) -> PathBuf {
    let start = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
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
    Diagnostic::new(
        path,
        pos.line,
        pos.col,
        format!("unknown icon name '{name}' on Icon"),
        help,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        fs::write(
            dir.join("outside/icons.json"),
            r##"{"icons":[{"name":"x","paths":[{"d":"M0 0"}]}]}"##,
        )
        .unwrap();
        // No lockfile: the exact-pin fallback builds registry/evil/1.0.0, which
        // does not exist, so nothing resolves and nothing escapes the root.
        assert!(resolve(&dir).is_empty());
    }

    #[test]
    fn mirroring_flips_x_and_arc_sweep_only() {
        assert_eq!(mirror_path_d("M3 12h18", 24.0), "M21 12h-18");
        assert_eq!(
            mirror_path_d("M2 12a10 10 0 1 0 20 0", 24.0),
            "M22 12a10 10 0 1 1 -20 0"
        );
        assert_eq!(mirror_path_d("M9 5l7 7-7 7", 24.0), "M15 5l-7 7 7 7");
    }

    #[test]
    fn a_missing_style_falls_back_to_outline() {
        let record = json!({
            "viewBox": "0 0 24 24",
            "paths": ["M3 12h18"],
            "styles": {"filled": ["M4 12h16"]}
        });
        assert_eq!(
            variant_paths("star", &record, "filled", false),
            vec![json!("M4 12h16")]
        );
        assert_eq!(
            variant_paths("star", &record, "rounded", false),
            vec![json!("M3 12h18")]
        );
    }
}
