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

struct Merge {
    states: Vec<StateDecl>,
    models: Vec<ModelDecl>,
    apis: Vec<ApiDecl>,
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
        Self {
            states: Vec::new(),
            models: Vec::new(),
            apis: Vec::new(),
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
            nodes,
            theme,
            models,
            apis,
        } = main;
        self.states.extend(states);
        self.models.extend(models);
        self.apis.extend(apis);
        Program {
            app,
            imports,
            states: self.states,
            computed,
            nodes,
            theme: self.theme.or(theme),
            models: self.models,
            apis: self.apis,
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
    Ok(resolver.merge.finish(main))
}
