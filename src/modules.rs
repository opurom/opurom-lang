use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::diag::{DResult, Diagnostic};
use crate::lexer::lex;
use crate::parser::{self, ApiDecl, ImportDecl, ModelDecl, Pos, Program, StateDecl, ThemeDecl};

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn has_traversal(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| part == "..")
}

fn at_import(
    origin: &Path,
    import: &ImportDecl,
    message: impl Into<String>,
    help: impl Into<String>,
) -> Diagnostic {
    let origin_text = origin.display().to_string();
    Diagnostic::new(&origin_text, import.pos.line, import.pos.col, message, help)
}

fn module_error(diag: Diagnostic, origin: &Path, import: &ImportDecl) -> Diagnostic {
    at_import(
        origin,
        import,
        format!(
            "cannot import '{}' from {}: {}",
            import.path, diag.path, diag.message
        ),
        diag.help,
    )
}

fn project_root(main: &Path) -> DResult<(PathBuf, Option<PathBuf>)> {
    let start = main.parent().unwrap_or_else(|| Path::new("."));
    let root = canonical(start);
    for dir in root.ancestors() {
        let manifest = dir.join("opurom.json");
        if manifest.is_file() {
            return Ok((canonical(dir), Some(manifest)));
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    Ok((root, None))
}

fn resolution_roots(root: &Path, manifest: Option<&PathBuf>) -> DResult<Vec<PathBuf>> {
    let mut roots = vec![root.to_path_buf()];
    let Some(manifest) = manifest else {
        return Ok(roots);
    };
    let manifest_text = manifest.display().to_string();
    let text = fs::read_to_string(manifest).map_err(|e| {
        Diagnostic::file(
            &manifest_text,
            format!("cannot read project manifest: {e}"),
            "check the opurom.json path and permissions",
        )
    })?;
    let doc: Json = serde_json::from_str(&text).map_err(|e| {
        Diagnostic::file(
            &manifest_text,
            format!("invalid opurom.json: {e}"),
            "write moduleRoots as an array of relative path strings",
        )
    })?;
    if let Some(value) = doc.get("moduleRoots") {
        let list = value.as_array().ok_or_else(|| {
            Diagnostic::file(
                &manifest_text,
                "moduleRoots must be an array of strings",
                "write \"moduleRoots\": [\"modules\"]",
            )
        })?;
        for entry in list {
            let root_name = entry.as_str().ok_or_else(|| {
                Diagnostic::file(
                    &manifest_text,
                    "moduleRoots must be an array of strings",
                    "write each root as a relative path string",
                )
            })?;
            if Path::new(root_name).is_absolute() || has_traversal(root_name) {
                return Err(Diagnostic::file(
                    &manifest_text,
                    format!("moduleRoots entry '{root_name}' may not be absolute or contain '..'"),
                    "keep every module root inside the project directory",
                ));
            }
            let candidate = root.join(root_name);
            if let Ok(found) = candidate.canonicalize() {
                if !found.starts_with(root) {
                    return Err(Diagnostic::file(
                        &manifest_text,
                        format!("moduleRoots entry '{root_name}' escapes the project root"),
                        "keep every module root inside the project directory",
                    ));
                }
                roots.push(found);
            }
        }
    }
    roots.extend(package_roots(root, manifest, &doc)?);
    Ok(roots)
}

/// Package module roots from `opurom.lock`. Every dependency pinned in the
/// lockfile contributes its installed `registry/<name>/<version>` directory, so
/// an import inside an external package resolves by file name exactly like a
/// project module. The lockfile is the only version source: `opurom-pkg` owns
/// SemVer range selection, and the compiler never resolves ranges itself. A
/// missing or malformed lock yields no package roots; moduleRoots keep working.
fn package_roots(root: &Path, manifest: &Path, doc: &Json) -> DResult<Vec<PathBuf>> {
    let Some(dependencies) = doc.get("dependencies").and_then(|d| d.as_object()) else {
        return Ok(Vec::new());
    };
    if dependencies.is_empty() {
        return Ok(Vec::new());
    }
    let lock_path = manifest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("opurom.lock");
    // A missing or malformed lockfile is not fatal: exact version pins in the
    // manifest still resolve, so a project without `opurom-pkg` can import
    // packages; only range dependencies then stay unresolved.
    let lock_text = fs::read_to_string(&lock_path).unwrap_or_default();
    let lock: Json = serde_json::from_str(&lock_text).unwrap_or(Json::Null);
    let packages = lock
        .get("packages")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let mut roots = Vec::new();
    for (name, contract) in dependencies {
        let pinned = contract.as_str().unwrap_or_default();
        let mut found_dir: Option<String> = None;
        if let Some(entry) = packages
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name.as_str()))
        {
            found_dir = entry
                .get("path")
                .and_then(|p| p.as_str())
                .map(|p| p.to_string());
        } else if pinned.split('.').count() == 3
            && pinned.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            // No lockfile entry: an exact x.y.z pin resolves directly, so a
            // project without `opurom-pkg` can still import packages. Ranges
            // never resolve here; `opurom-pkg` owns SemVer selection.
            found_dir = Some(format!("registry/{name}/{pinned}"));
        }
        let Some(relative) = found_dir else {
            continue;
        };
        if Path::new(&relative).is_absolute() || has_traversal(&relative) {
            return Err(Diagnostic::file(
                &lock_text,
                format!("lock entry '{name}' may not be absolute or contain '..'"),
                "regenerate opurom.lock with 'opurom-pkg lock'",
            ));
        }
        let candidate = root.join(relative);
        let Ok(found) = candidate.canonicalize() else {
            continue;
        };
        if !found.starts_with(root) {
            return Err(Diagnostic::file(
                &lock_text,
                format!("lock entry '{name}' escapes the project root"),
                "install packages inside the project directory",
            ));
        }
        if !roots.contains(&found) {
            roots.push(found);
        }
    }
    Ok(roots)
}

fn duplicate_name(module_path: &str, name: &str, pos: Pos) -> Diagnostic {
    Diagnostic::new(
        module_path,
        pos.line,
        pos.col,
        format!("duplicate imported name '{name}'"),
        "rename one declaration or remove the duplicate import",
    )
}

fn duplicate_component(module_path: &str, decl: &crate::parser::ComponentDecl) -> Diagnostic {
    Diagnostic::new(
        module_path,
        decl.pos.line,
        decl.pos.col,
        format!("duplicate component '{}'", decl.name),
        "declare every component once across the app and its imports",
    )
}

struct Merge {
    states: Vec<StateDecl>,
    models: Vec<ModelDecl>,
    apis: Vec<ApiDecl>,
    components: Vec<crate::parser::ComponentDecl>,
    component_names: HashSet<String>,
    theme: Option<ThemeDecl>,
    theme_seen: bool,
    names: HashSet<String>,
    api_paths: HashSet<String>,
}

impl Merge {
    fn new(main: &Program) -> Self {
        let mut names = HashSet::new();
        for state in &main.states {
            names.insert(state.name.clone());
        }
        for computed in &main.computed {
            names.insert(computed.name.clone());
        }
        for model in &main.models {
            names.insert(model.name.clone());
        }
        let mut api_paths = HashSet::new();
        for api in &main.apis {
            api_paths.insert(api.path.clone());
        }
        // The main program's own components already live in `Program`; seed the
        // name set so an imported module cannot redefine one of them.
        let component_names: HashSet<String> =
            main.components.iter().map(|c| c.name.clone()).collect();
        Self {
            states: Vec::new(),
            models: Vec::new(),
            apis: Vec::new(),
            components: Vec::new(),
            component_names,
            theme: None,
            theme_seen: main.theme.is_some(),
            names,
            api_paths,
        }
    }

    fn add_name(&mut self, module_path: &str, name: &str, pos: Pos) -> DResult<()> {
        if !self.names.insert(name.to_string()) {
            return Err(duplicate_name(module_path, name, pos));
        }
        Ok(())
    }

    fn add_module(&mut self, module: Program, module_path: &str) -> DResult<()> {
        for state in module.states {
            self.add_name(module_path, &state.name, state.pos)?;
            self.states.push(state);
        }
        for model in module.models {
            self.add_name(module_path, &model.name, model.pos)?;
            self.models.push(model);
        }
        for api in module.apis {
            if !self.api_paths.insert(api.path.clone()) {
                return Err(duplicate_name(module_path, &api.path, api.pos));
            }
            self.apis.push(api);
        }
        for component in module.components {
            if !self.component_names.insert(component.name.clone()) {
                return Err(duplicate_component(module_path, &component));
            }
            self.components.push(component);
        }
        if let Some(theme) = module.theme {
            if self.theme_seen {
                return Err(duplicate_name(module_path, "theme", theme.pos));
            }
            self.theme_seen = true;
            self.theme = Some(theme);
        }
        Ok(())
    }

    fn finish(mut self, main: Program) -> Program {
        let Program {
            app,
            imports,
            states,
            computed,
            components,
            nodes,
            theme,
            models,
            apis,
            pages,
            routes,
            locale,
            directionality,
            catalogs,
            quality,
        } = main;
        self.states.extend(states);
        self.models.extend(models);
        self.apis.extend(apis);
        // Main components come first, then imported ones in merge order.
        let mut components = components;
        components.extend(self.components);
        Program {
            app,
            imports,
            states: self.states,
            computed,
            components,
            nodes,
            theme: self.theme.or(theme),
            models: self.models,
            apis: self.apis,
            pages,
            routes,
            locale,
            directionality,
            catalogs,
            quality,
        }
    }
}

struct Resolver {
    root: PathBuf,
    roots: Vec<PathBuf>,
    stack: Vec<PathBuf>,
    visited: HashSet<PathBuf>,
    merge: Merge,
}

impl Resolver {
    fn short(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn roots_help(&self) -> String {
        let names = self
            .roots
            .iter()
            .map(|p| self.short(p))
            .collect::<Vec<_>>()
            .join(", ");
        format!("paths are relative to the importing file or one of: {names}")
    }

    fn validate_import(&self, import: &ImportDecl, origin: &Path) -> DResult<()> {
        if !import.path.ends_with(".opu") {
            return Err(at_import(
                origin,
                import,
                format!("import path '{}' must end in '.opu'", import.path),
                "write a relative path such as 'models/todo.opu'",
            ));
        }
        if Path::new(&import.path).is_absolute() || has_traversal(&import.path) {
            return Err(at_import(
                origin,
                import,
                "import path may not be absolute or contain '..'",
                "keep imports inside the project root; use moduleRoots for shared directories",
            ));
        }
        Ok(())
    }

    fn resolve_import(&mut self, import: &ImportDecl, origin: &Path) -> DResult<()> {
        self.validate_import(import, origin)?;
        let relative = Path::new(&import.path);
        let mut candidates = Vec::new();
        if let Some(parent) = origin.parent() {
            candidates.push(parent.join(relative));
        }
        for root in &self.roots {
            candidates.push(root.join(relative));
        }
        let found = candidates
            .iter()
            .find(|p| p.is_file())
            .cloned()
            .ok_or_else(|| {
                at_import(
                    origin,
                    import,
                    format!(
                        "cannot read imported module '{}': file not found",
                        import.path
                    ),
                    self.roots_help(),
                )
            })?;
        let module_path = found.canonicalize().map_err(|e| {
            at_import(
                origin,
                import,
                format!("cannot resolve imported module '{}': {e}", import.path),
                self.roots_help(),
            )
        })?;
        if !module_path.starts_with(&self.root) {
            return Err(at_import(
                origin,
                import,
                format!("import path '{}' escapes the project root", import.path),
                "keep imports and symlink targets inside the project root",
            ));
        }
        if self.stack.contains(&module_path) {
            let chain = self
                .stack
                .iter()
                .chain(std::iter::once(&module_path))
                .map(|p| self.short(p))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(at_import(
                origin,
                import,
                format!("import cycle detected: {chain}"),
                "remove the import that closes the cycle",
            ));
        }
        if !self.visited.insert(module_path.clone()) {
            return Ok(());
        }
        let module_text = module_path.display().to_string();
        let source = fs::read_to_string(&found).map_err(|e| {
            at_import(
                origin,
                import,
                format!("cannot read imported module '{}': {e}", import.path),
                self.roots_help(),
            )
        })?;
        let toks = lex(&source, &module_text).map_err(|diag| module_error(diag, origin, import))?;
        let module = parser::parse_module(toks, &module_text)
            .map_err(|diag| module_error(diag, origin, import))?;
        self.stack.push(module_path.clone());
        for nested in &module.imports {
            self.resolve_import(nested, &module_path)?;
        }
        self.stack.pop();
        self.merge.add_module(module, &module_text)?;
        Ok(())
    }
}

/// Diagnostic at a token-import source position.
fn token_err(
    origin: &str,
    pos: Pos,
    msg: impl Into<String>,
    help: impl Into<String>,
) -> Diagnostic {
    Diagnostic::new(origin, pos.line, pos.col, msg, help)
}

/// A JSON number as a plain decimal string, never in exponent notation.
fn number_to_decimal(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    if let Some(f) = n.as_f64() {
        return format!("{f}");
    }
    n.to_string()
}

/// Flatten a JSON token object into `(key, string value)` leaves. Keys join the
/// full path with `-`, and `.` inside a segment is rewritten to `-`.
fn flatten_token_object(
    obj: &serde_json::Map<String, Json>,
    prefix: &str,
    out: &mut Vec<(String, String)>,
    origin: &str,
    pos: Pos,
) -> DResult<()> {
    for (key, value) in obj {
        let segment = key.replace('.', "-");
        let full = if prefix.is_empty() {
            segment
        } else {
            format!("{prefix}-{segment}")
        };
        match value {
            Json::Object(child) => flatten_token_object(child, &full, out, origin, pos)?,
            Json::String(text) => out.push((full, text.clone())),
            Json::Number(n) => out.push((full, number_to_decimal(n))),
            _ => {
                return Err(token_err(
                    origin,
                    pos,
                    format!("token '{full}' must hold a string or number"),
                    "write a string or number leaf in the token file",
                ))
            }
        }
    }
    Ok(())
}

/// Expand opt-in JSON token imports into flattened `md-*` strings on the theme.
fn expand_token_imports(
    theme: &mut ThemeDecl,
    origin: &Path,
    root: &Path,
    roots: &[PathBuf],
) -> DResult<()> {
    if theme.token_imports.is_empty() {
        return Ok(());
    }
    let origin_text = origin.display().to_string();
    let mut seen: HashSet<String> = theme.tokens.iter().map(|(k, _)| k.clone()).collect();
    let mut flat: Vec<(String, String)> = Vec::new();
    for (rel, pos) in &theme.token_imports {
        if !rel.ends_with(".json") {
            return Err(token_err(
                &origin_text,
                *pos,
                format!("token import '{rel}' must end in '.json'"),
                "write a relative path such as 'theme/tokens.json'",
            ));
        }
        if Path::new(rel).is_absolute() || has_traversal(rel) {
            return Err(token_err(
                &origin_text,
                *pos,
                "token import may not be absolute or contain '..'",
                "keep token files inside the project root",
            ));
        }
        let relative = Path::new(rel);
        let mut candidates = Vec::new();
        if let Some(parent) = origin.parent() {
            candidates.push(parent.join(relative));
        }
        for r in roots {
            candidates.push(r.join(relative));
        }
        let found = candidates
            .iter()
            .find(|p| p.is_file())
            .cloned()
            .ok_or_else(|| {
                token_err(
                    &origin_text,
                    *pos,
                    format!("cannot read token import '{rel}': file not found"),
                    "paths are relative to the source or one of the module roots",
                )
            })?;
        let resolved = found.canonicalize().map_err(|e| {
            token_err(
                &origin_text,
                *pos,
                format!("cannot resolve token import '{rel}': {e}"),
                "check the token file path",
            )
        })?;
        if !resolved.starts_with(root) {
            return Err(token_err(
                &origin_text,
                *pos,
                format!("token import '{rel}' escapes the project root"),
                "keep token files inside the project root",
            ));
        }
        let text = fs::read_to_string(&found).map_err(|e| {
            token_err(
                &origin_text,
                *pos,
                format!("cannot read token import '{rel}': {e}"),
                "check the file path and permissions",
            )
        })?;
        let doc: Json = serde_json::from_str(&text).map_err(|e| {
            token_err(
                &origin_text,
                *pos,
                format!("invalid token import '{rel}': {e}"),
                "write a JSON object of token names",
            )
        })?;
        let obj = doc.as_object().ok_or_else(|| {
            token_err(
                &origin_text,
                *pos,
                format!("token import '{rel}' must hold a JSON object"),
                "write an object of token names to values",
            )
        })?;
        let mut entries = Vec::new();
        flatten_token_object(obj, "", &mut entries, &origin_text, *pos)?;
        for (name, value) in entries {
            if !name.starts_with("md-") {
                return Err(token_err(
                    &origin_text,
                    *pos,
                    format!("imported token '{name}' must use the 'md-' namespace"),
                    format!("rename '{name}' with an 'md-' prefix"),
                ));
            }
            if seen.contains(&name) {
                return Err(token_err(
                    &origin_text,
                    *pos,
                    format!("duplicate token '{name}'"),
                    "remove the duplicate token key",
                ));
            }
            seen.insert(name.clone());
            flat.push((name, value));
        }
    }
    flat.sort_by(|a, b| a.0.cmp(&b.0));
    theme.imported_tokens = flat;
    Ok(())
}

/// Merge declarations from `.opu` imports into the main program before lowering.
pub fn resolve_imports(main: Program, source_path: &str) -> DResult<Program> {
    let main_abs = absolute_path(Path::new(source_path));
    let main_path = canonical(&main_abs);
    let (root, manifest) = project_root(&main_path)?;
    let roots = resolution_roots(&root, manifest.as_ref())?;
    let mut visited = HashSet::new();
    visited.insert(main_path.clone());
    let mut resolver = Resolver {
        root,
        roots,
        stack: vec![main_path.clone()],
        visited,
        merge: Merge::new(&main),
    };
    for import in &main.imports {
        resolver.resolve_import(import, &main_path)?;
    }
    let mut program = resolver.merge.finish(main);
    if let Some(theme) = program.theme.as_mut() {
        expand_token_imports(theme, &main_path, &resolver.root, &resolver.roots)?;
    }
    Ok(program)
}
