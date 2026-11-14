use std::collections::{HashMap, HashSet};

use serde_json::{json, Value as Json};

use crate::catalog;
use crate::diag::{DResult, Diagnostic};
use crate::lower::{interp_parts, offset_pos, IndexExpr, Part, Ty, KINDS};
use crate::parser::{
    Arg, Block, Effect, Expr, ExprKind, ForIter, Lit, Node, NodeKind, Pos, Program, Stmt, StmtKind,
    Value as AstValue,
};

/// The value lattice of the language: `int`, `bool` and `string` are stored in
/// IR state, `list` is compile-time only and `number` is a literal-only type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueTy {
    Int,
    Bool,
    Str,
    Number,
    List,
}

impl ValueTy {
    pub fn name(self) -> &'static str {
        match self {
            ValueTy::Int => "int",
            ValueTy::Bool => "bool",
            ValueTy::Str => "string",
            ValueTy::Number => "number",
            ValueTy::List => "list",
        }
    }

    /// "an int" but "a bool" and "a string".
    pub fn article(self) -> &'static str {
        match self {
            ValueTy::Int => "an",
            _ => "a",
        }
    }

    fn is_scalar(self) -> bool {
        self != ValueTy::List
    }

    fn of_state(ty: Ty) -> Self {
        match ty {
            Ty::Int => ValueTy::Int,
            Ty::Bool => ValueTy::Bool,
            Ty::Str => ValueTy::Str,
            Ty::List => ValueTy::List,
        }
    }

    fn of_lit(lit: &Lit) -> Self {
        match lit {
            Lit::Int(_) => ValueTy::Int,
            Lit::Float(_) => ValueTy::Number,
            Lit::Bool(_) => ValueTy::Bool,
            Lit::Str(_) => ValueTy::Str,
            Lit::List(_) => ValueTy::List,
        }
    }
}

/// The inferred type environment of one program.
#[derive(Clone)]
pub struct Env {
    pub types: HashMap<String, Ty>,
    pub inits: HashMap<String, Json>,
    pub computed: HashSet<String>,
    pub order: Vec<String>,
}

impl Env {
    fn state_list(&self) -> String {
        if self.order.is_empty() {
            "(no states declared)".to_string()
        } else {
            self.order.join(", ")
        }
    }
}

pub fn state_ty(name: &str) -> Option<Ty> {
    match name {
        "int" => Some(Ty::Int),
        "bool" => Some(Ty::Bool),
        "string" => Some(Ty::Str),
        "list" => Some(Ty::List),
        _ => None,
    }
}

/// The type of one component parameter: declared in the source, inferred from
/// the first call site, or not yet known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamTy {
    Declared(Ty),
    Inferred(Ty),
    Unknown,
}

impl ParamTy {
    fn inferred(ty: Ty) -> Self {
        ParamTy::Inferred(ty)
    }
}

/// The parameter types of every declared component.
pub type ComponentTypes = HashMap<String, Vec<ParamTy>>;

/// Record the type of a component parameter at a call site and report a
/// conflicting call site.
pub fn check_component_param(
    types: &mut ComponentTypes,
    component: &str,
    index: usize,
    param: &str,
    got: Ty,
    path: &str,
    pos: Pos,
) -> DResult<()> {
    let declared = types
        .get_mut(component)
        .and_then(|params| params.get_mut(index))
        .expect("component types are initialized");
    let inferred = matches!(*declared, ParamTy::Inferred(_));
    let expected = match *declared {
        ParamTy::Unknown => {
            *declared = ParamTy::inferred(got);
            got
        }
        ParamTy::Declared(ty) | ParamTy::Inferred(ty) => ty,
    };
    if got == expected {
        return Ok(());
    }
    let help = if inferred {
        format!(
            "the type of '{param}' was inferred as {} from the first use of '{component}'; pass a {} literal",
            expected.name(),
            expected.name()
        )
    } else {
        format!("pass a {} literal to '{component}'", expected.name())
    };
    Err(Diagnostic::new(
        path,
        pos.line,
        pos.col,
        format!(
            "parameter '{param}' of component '{component}' expects {}, found {}",
            expected.name(),
            got.name()
        ),
        help,
    ))
}

fn article_of(ty: Ty) -> &'static str {
    if ty == Ty::Int {
        "an"
    } else {
        "a"
    }
}

/// A type diagnostic names the expected type, the found type and the position.
pub fn type_error(
    path: &str,
    pos: Pos,
    message: impl Into<String>,
    expected: impl Into<String>,
    found: impl Into<String>,
    help: impl Into<String>,
) -> Diagnostic {
    Diagnostic::type_error(path, pos.line, pos.col, message, expected, found, help)
}

// ------------------------------------------------------------- inference ---

/// Infer the type of every state declaration from its initializer and check an
/// optional `state name: type` annotation against it.
pub fn infer_states(
    program: &Program,
    path: &str,
) -> DResult<(HashMap<String, Ty>, HashMap<String, Json>)> {
    let mut types = HashMap::new();
    let mut inits = HashMap::new();
    let mut order = Vec::new();
    for decl in &program.states {
        if types.contains_key(&decl.name) {
            return Err(Diagnostic::new(
                path,
                decl.pos.line,
                decl.pos.col,
                format!("duplicate state '{}'", decl.name),
                format!(
                    "rename the second declaration; declared states are: {}",
                    order.join(", ")
                ),
            ));
        }
        let (ty, init) = match &decl.init {
            Lit::Int(n) => (Ty::Int, json!(*n)),
            Lit::Bool(b) => (Ty::Bool, json!(*b)),
            Lit::Str(v) => (Ty::Str, json!(v)),
            Lit::List(values) => (Ty::List, json!(values)),
            Lit::Float(_) => {
                return Err(Diagnostic::new(
                    path,
                    decl.pos.line,
                    decl.pos.col,
                    format!("state '{}' cannot be a decimal", decl.name),
                    "declare numbers as integers, such as 'state count = 0'".to_string(),
                ))
            }
        };
        if let Some((annotation, pos)) = &decl.annotation {
            let declared = state_ty(annotation).ok_or_else(|| {
                Diagnostic::new(
                    path,
                    pos.line,
                    pos.col,
                    format!("unknown state type '{annotation}'"),
                    "state types are int, bool, string or list".to_string(),
                )
            })?;
            if declared != ty {
                return Err(type_error(
                    path,
                    decl.init_pos,
                    format!("state '{}' does not match its declared type", decl.name),
                    declared.name(),
                    ty.name(),
                    format!(
                        "write {} {} initializer for 'state {}: {} = ...'",
                        article_of(declared),
                        declared.name(),
                        decl.name,
                        declared.name()
                    ),
                ));
            }
        }
        types.insert(decl.name.clone(), ty);
        inits.insert(decl.name.clone(), init);
        order.push(decl.name.clone());
    }
    Ok((types, inits))
}

/// An operand of the same sign that keeps `lhs op rhs` inside i64, for the hint.
fn safe_operand(op: &str, lhs: i64, rhs: i64) -> i64 {
    let a = lhs as i128;
    let safe = match op {
        "+" if rhs > 0 => i64::MAX as i128 - a,
        "+" => i64::MIN as i128 - a,
        "-" if rhs > 0 => a - i64::MIN as i128,
        "-" => a - i64::MAX as i128,
        _ => {
            if a == 0 {
                return 0;
            }
            let bound = if (lhs > 0) == (rhs > 0) {
                i64::MAX
            } else {
                i64::MIN
            };
            bound as i128 / a
        }
    };
    safe.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Infer the type and the compile-time value of a computed expression.
pub fn infer_computed(
    expr: &Expr,
    env: &Env,
    computed_names: &HashSet<String>,
    path: &str,
) -> DResult<(Ty, Json)> {
    match &expr.kind {
        ExprKind::Int(n) => Ok((Ty::Int, json!(*n))),
        ExprKind::State(name) => {
            if computed_names.contains(name) {
                return Err(Diagnostic::new(
                    path,
                    expr.pos.line,
                    expr.pos.col,
                    format!("computed expression cannot read computed value '{name}'"),
                    "read a declared state; computed-to-computed references are not supported yet",
                ));
            }
            let ty = env.types.get(name).copied().ok_or_else(|| {
                type_error(
                    path,
                    expr.pos,
                    format!("unknown state '{name}' in computed expression"),
                    "int, bool or string",
                    "unknown",
                    format!("declare '{name}' or use one of: {}", env.state_list()),
                )
            })?;
            if ty == Ty::List {
                return Err(type_error(
                    path,
                    expr.pos,
                    format!("list state '{name}' is compile-time only"),
                    "int, bool or string",
                    "list",
                    "use a declared int, bool or string state in a computed expression",
                ));
            }
            Ok((ty, env.inits.get(name).cloned().unwrap_or(Json::Null)))
        }
        ExprKind::Add(lhs, rhs) | ExprKind::Sub(lhs, rhs) | ExprKind::Mul(lhs, rhs) => {
            let op = match &expr.kind {
                ExprKind::Add(..) => "+",
                ExprKind::Sub(..) => "-",
                ExprKind::Mul(..) => "*",
                _ => unreachable!("binary expression variant"),
            };
            let (lhs_ty, lhs_value) = infer_computed(lhs, env, computed_names, path)?;
            let (rhs_ty, rhs_value) = infer_computed(rhs, env, computed_names, path)?;
            if lhs_ty != Ty::Int || rhs_ty != Ty::Int {
                let found = if lhs_ty != Ty::Int { lhs_ty } else { rhs_ty };
                return Err(type_error(
                    path,
                    expr.pos,
                    format!("computed expression '{op}' requires int operands"),
                    "int",
                    found.name(),
                    "use only int states and integer literals with +, - and *",
                ));
            }
            let a = lhs_value.as_i64().unwrap_or(0);
            let b = rhs_value.as_i64().unwrap_or(0);
            let value = match &expr.kind {
                ExprKind::Add(..) => a.checked_add(b),
                ExprKind::Sub(..) => a.checked_sub(b),
                ExprKind::Mul(..) => a.checked_mul(b),
                _ => unreachable!("binary expression variant"),
            };
            let value = value.ok_or_else(|| {
                Diagnostic::arithmetic_overflow(
                    path,
                    expr.pos.line,
                    expr.pos.col,
                    op,
                    a,
                    b,
                    safe_operand(op, a, b),
                )
            })?;
            Ok((Ty::Int, json!(value)))
        }
    }
}

// --------------------------------------------------------------- checker ---

struct Checker<'a> {
    path: &'a str,
    env: Env,
    locals: Vec<HashMap<String, ValueTy>>,
    conditions: HashMap<String, Pos>,
}

impl<'a> Checker<'a> {
    fn err(
        &self,
        pos: Pos,
        message: impl Into<String>,
        expected: impl Into<String>,
        found: impl Into<String>,
        help: impl Into<String>,
    ) -> Diagnostic {
        type_error(self.path, pos, message, expected, found, help)
    }

    fn unknown_state(&self, name: &str, context: &str, pos: Pos, expected: &str) -> Diagnostic {
        self.err(
            pos,
            format!("unknown state '{name}' {context}"),
            expected,
            "unknown",
            format!("declare '{name}' or use one of: {}", self.env.state_list()),
        )
    }

    /// The legacy wording of the event-statement diagnostic.
    fn unknown_event_state(&self, name: &str, pos: Pos) -> Diagnostic {
        self.err(
            pos,
            format!("unknown state '{name}' in event statement"),
            "a declared mutable state",
            "unknown",
            format!("declare '{name}' or use one of: {}", self.env.state_list()),
        )
    }

    fn readonly(&self, name: &str, ty: ValueTy, pos: Pos) -> Diagnostic {
        self.err(
            pos,
            format!("computed value '{name}' is read-only"),
            "a mutable state",
            format!("computed {}", ty.name()),
            "change a declared state instead; computed values are recalculated from state",
        )
    }

    fn list_only(&self, name: &str, pos: Pos) -> Diagnostic {
        self.err(
            pos,
            format!("list state '{name}' is compile-time only"),
            "int, bool or string",
            "list",
            "use 'for (item in items)' to unroll list values at compile time",
        )
    }

    /// Statement targets, conditions and bindings always name a declared state.
    fn state_of(&self, name: &str) -> Option<ValueTy> {
        self.env.types.get(name).copied().map(ValueTy::of_state)
    }

    /// String interpolation resolves a loop variable first, then a state.
    fn interpolation_ty(&self, name: &str) -> Option<ValueTy> {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(*ty);
            }
        }
        self.state_of(name)
    }

    fn value_ty(
        &self,
        value: &AstValue,
        pos: Pos,
        context: &str,
        expected: &str,
    ) -> DResult<ValueTy> {
        match value {
            AstValue::Lit(lit) => Ok(ValueTy::of_lit(lit)),
            AstValue::Ident(name) => self
                .state_of(name)
                .ok_or_else(|| self.unknown_state(name, context, pos, expected)),
        }
    }

    fn statement(&self, stmt: &Stmt) -> DResult<()> {
        // `push:` targets a declared page, not a state; the page set is checked
        // during lowering. `form.validate()` targets a built-in form receiver.
        if matches!(stmt.kind, StmtKind::Push | StmtKind::Validate) {
            return Ok(());
        }
        let target = self
            .state_of(&stmt.target)
            .ok_or_else(|| self.unknown_event_state(&stmt.target, stmt.pos))?;
        if self.env.computed.contains(&stmt.target) {
            return Err(self.readonly(&stmt.target, target, stmt.pos));
        }
        if target == ValueTy::List {
            return Err(self.list_only(&stmt.target, stmt.pos));
        }
        match &stmt.kind {
            StmtKind::Add(value, pos) | StmtKind::Sub(value, pos) => {
                if target != ValueTy::Int {
                    return Err(self.err(
                        stmt.pos,
                        format!(
                            "'+=' / '-=' requires an int state, found {} state '{}'",
                            target.name(),
                            stmt.target
                        ),
                        "int",
                        target.name(),
                        format!(
                            "use '=' for {} {} state or declare '{}' as int",
                            target.article(),
                            target.name(),
                            stmt.target
                        ),
                    ));
                }
                let found = self.value_ty(value, *pos, "in event expression", "int")?;
                if found != ValueTy::Int {
                    let message = match value {
                        AstValue::Ident(name) => format!(
                            "'+=' / '-=' requires an int state, found {} state '{}'",
                            found.name(),
                            name
                        ),
                        _ => format!(
                            "'+=' / '-=' requires an int literal, found {}",
                            found.name()
                        ),
                    };
                    return Err(self.err(
                        *pos,
                        message,
                        "int",
                        found.name(),
                        "use an int literal here or read an int state",
                    ));
                }
            }
            StmtKind::Set(value, pos) => {
                let found = self.value_ty(value, *pos, "in event expression", target.name())?;
                if found != target {
                    let help = if matches!(value, AstValue::Ident(_)) {
                        format!(
                            "read {} {} state here or change the state initializer",
                            target.article(),
                            target.name()
                        )
                    } else {
                        format!(
                            "use {} {} literal or change the state initializer",
                            target.article(),
                            target.name()
                        )
                    };
                    return Err(self.err(
                        *pos,
                        format!(
                            "cannot assign {} to {} state '{}'",
                            found.name(),
                            target.name(),
                            stmt.target
                        ),
                        target.name(),
                        found.name(),
                        help,
                    ));
                }
            }
            StmtKind::Toggle => {
                if target != ValueTy::Bool {
                    return Err(self.err(
                        stmt.pos,
                        format!(
                            "'toggle()' requires a bool state, found {} state '{}'",
                            target.name(),
                            stmt.target
                        ),
                        "bool",
                        target.name(),
                        "declare the state with true/false or use '+=' / '=' instead",
                    ));
                }
            }
            StmtKind::Push | StmtKind::Validate => {}
        }
        Ok(())
    }

    fn effect(&self, effect: &Effect) -> DResult<()> {
        if let Effect::Fetch { into, pos, .. } = effect {
            let ty = self.state_of(into).ok_or_else(|| {
                self.unknown_state(into, "in fetch target", *pos, "a declared string state")
            })?;
            if self.env.computed.contains(into) {
                return Err(self.readonly(into, ty, *pos));
            }
            if ty != ValueTy::Str {
                return Err(self.err(
                    *pos,
                    format!(
                        "'fetch' requires a string state, found {} state '{}'",
                        ty.name(),
                        into
                    ),
                    "string",
                    ty.name(),
                    format!("declare '{into}' with a string initializer"),
                ));
            }
        }
        Ok(())
    }

    fn block(&mut self, block: &Block) -> DResult<()> {
        for stmt in &block.stmts {
            self.statement(stmt)?;
        }
        for effect in &block.effects {
            self.effect(effect)?;
        }
        self.nodes(&block.nodes)
    }

    fn bind(&self, kind: &str, arg: &Arg) -> DResult<()> {
        let expected = match kind {
            "Switch" | "Checkbox" => ValueTy::Bool,
            "Slider" => ValueTy::Int,
            _ => ValueTy::Str,
        };
        if let AstValue::Ident(name) = &arg.value {
            let found = self.state_of(name).ok_or_else(|| {
                self.unknown_state(
                    name,
                    "in 'bind'",
                    arg.pos,
                    &format!("a declared {} state", expected.name()),
                )
            })?;
            if self.env.computed.contains(name) {
                return Err(self.readonly(name, found, arg.pos));
            }
            if found == ValueTy::List {
                return Err(self.list_only(name, arg.pos));
            }
            if found != expected {
                return Err(self.err(
                    arg.pos,
                    format!(
                        "'bind' requires a {} state, found {} state '{}'",
                        expected.name(),
                        found.name(),
                        name
                    ),
                    expected.name(),
                    found.name(),
                    format!(
                        "declare the state with {} {} initializer or remove 'bind'",
                        expected.article(),
                        expected.name()
                    ),
                ));
            }
        }
        Ok(())
    }

    fn interpolation(&self, text: &str, base: Pos) -> DResult<()> {
        let parts = interp_parts(text).map_err(|(off, message)| {
            Diagnostic::new(
                self.path,
                offset_pos(base, text, off).line,
                offset_pos(base, text, off).col,
                message,
                "close it with '}' or write '{{' for a literal '{'",
            )
        })?;
        for part in parts {
            match part {
                Part::Var(name, off) => {
                    let pos = offset_pos(base, text, off);
                    let ty = self.interpolation_ty(&name).ok_or_else(|| {
                        self.err(
                            pos,
                            format!("unknown state '{name}' in Text interpolation"),
                            "a declared scalar state or loop variable",
                            "unknown",
                            format!("declare '{name}' or use one of: {}", self.env.state_list()),
                        )
                    })?;
                    if !ty.is_scalar() {
                        return Err(self.list_only(&name, pos));
                    }
                }
                Part::Index { list, index, off } => {
                    let pos = offset_pos(base, text, off);
                    self.list_index(&list, &index, pos)?;
                }
                Part::Lit(_) => {}
            }
        }
        Ok(())
    }

    /// `{items[i]}` reads one element: the target must be a list state and the
    /// subscript must resolve to an integer. Bounds for a literal are checked
    /// here; a symbolic subscript is checked when lowering knows its value.
    fn list_index(&self, list: &str, index: &IndexExpr, pos: Pos) -> DResult<()> {
        let ty = self.state_of(list).ok_or_else(|| {
            self.err(
                pos,
                format!("unknown list state '{list}' in Text interpolation"),
                "a declared list state",
                "unknown",
                format!("declare '{list}' or use one of: {}", self.env.state_list()),
            )
        })?;
        if ty != ValueTy::List {
            return Err(self.err(
                pos,
                format!(
                    "'{}[...]' reads a list, found {} state '{list}'",
                    list,
                    ty.name()
                ),
                "list",
                ty.name(),
                "declare the state as a list or drop the subscript",
            ));
        }
        let n = match index {
            IndexExpr::Int(n) => *n,
            IndexExpr::Var(name) => {
                return match self.interpolation_ty(name) {
                    Some(ValueTy::Int) => Ok(()),
                    Some(other) => Err(self.err(
                        pos,
                        format!("list index '{name}' must be an int, found {}", other.name()),
                        "int",
                        other.name(),
                        "index with an integer literal or a 'for' count loop variable".to_string(),
                    )),
                    None => Err(self.err(
                        pos,
                        format!("unknown list index '{name}'"),
                        "an integer loop variable",
                        "unknown",
                        "index with an integer literal or a 'for' count loop variable".to_string(),
                    )),
                };
            }
        };
        let len = self
            .env
            .inits
            .get(list)
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        if n < 0 || n as usize >= len {
            return Err(self.err(
                pos,
                format!("list index {n} is out of range for '{list}' with {len} values"),
                "an index inside the list",
                "out-of-range index",
                format!("use an index from 0 to {}", len.saturating_sub(1)),
            ));
        }
        Ok(())
    }

    fn args(&self, source_kind: &str, args: &[Arg]) -> DResult<()> {
        let Some(kind) = primitive_kind(source_kind)? else {
            return Ok(());
        };
        for (idx, arg) in args.iter().enumerate() {
            let key = match &arg.name {
                Some(name) => name.as_str(),
                None if idx == 0 && kind == "Text" => "value",
                None => "",
            };
            if key == "bind" {
                self.bind(kind, arg)?;
            }
            if kind == "Text" && key == "value" {
                if let AstValue::Lit(Lit::Str(text)) = &arg.value {
                    self.interpolation(text, arg.pos)?;
                }
            }
        }
        Ok(())
    }

    fn condition(&mut self, name: &str, pos: Pos) -> DResult<()> {
        let ty = self
            .state_of(name)
            .ok_or_else(|| self.unknown_state(name, "in 'if'", pos, "a declared bool state"))?;
        if ty != ValueTy::Bool {
            return Err(self.err(
                pos,
                format!(
                    "'if' requires a bool state, found {} state '{}'",
                    ty.name(),
                    name
                ),
                "bool",
                ty.name(),
                "use a bool state or computed bool value for the condition",
            ));
        }
        self.conditions.entry(name.to_string()).or_insert(pos);
        Ok(())
    }

    fn nodes(&mut self, nodes: &[Node]) -> DResult<()> {
        for node in nodes {
            match &node.kind {
                NodeKind::Widget(kind) => self.args(kind, &node.args)?,
                NodeKind::If { cond, cond_pos, .. } => self.condition(cond, *cond_pos)?,
                NodeKind::For { var, iter } => {
                    let local = match iter {
                        ForIter::Count { .. } => ValueTy::Int,
                        ForIter::List { name, pos } => {
                            let ty = self.state_of(name).ok_or_else(|| {
                                self.err(
                                    *pos,
                                    format!("unknown list state '{name}' in 'for'"),
                                    "list",
                                    "unknown",
                                    format!(
                                        "declare '{name}' with a string list or use one of: {}",
                                        self.env.state_list()
                                    ),
                                )
                            })?;
                            if ty != ValueTy::List {
                                return Err(self.err(
                                    *pos,
                                    format!(
                                        "'for' requires a list state, found {} state '{}'",
                                        ty.name(),
                                        name
                                    ),
                                    "list",
                                    ty.name(),
                                    "declare the state with a string list or use an integer count",
                                ));
                            }
                            ValueTy::Str
                        }
                    };
                    let mut scope = HashMap::new();
                    scope.insert(var.clone(), local);
                    self.locals.push(scope);
                    let result = node
                        .block
                        .as_ref()
                        .map_or(Ok(()), |block| self.block(block));
                    self.locals.pop();
                    result?;
                    continue;
                }
                NodeKind::Repeat {
                    var,
                    list,
                    list_pos,
                } => {
                    let ty = self.state_of(list).ok_or_else(|| {
                        self.err(
                            *list_pos,
                            format!("unknown list state '{list}' in 'for'"),
                            "list",
                            "unknown",
                            format!(
                                "declare '{list}' with a string list or use one of: {}",
                                self.env.state_list()
                            ),
                        )
                    })?;
                    if ty != ValueTy::List {
                        return Err(self.err(
                            *list_pos,
                            format!(
                                "'for' requires a list state, found {} state '{}'",
                                ty.name(),
                                list
                            ),
                            "list",
                            ty.name(),
                            "declare the state with a string list or use 'for (i in 3) { ... }'",
                        ));
                    }
                    let mut scope = HashMap::new();
                    scope.insert(var.clone(), ValueTy::Str);
                    self.locals.push(scope);
                    let result = node
                        .block
                        .as_ref()
                        .map_or(Ok(()), |block| self.block(block));
                    self.locals.pop();
                    result?;
                    continue;
                }
            }
            if let Some(block) = &node.block {
                self.block(block)?;
            }
            if let NodeKind::If {
                else_block: Some(else_block),
                ..
            } = &node.kind
            {
                self.block(else_block)?;
            }
        }
        Ok(())
    }
}

fn primitive_kind<'a>(source: &'a str) -> DResult<Option<&'a str>> {
    if KINDS.contains(&source) {
        return Ok(Some(source));
    }
    Ok(catalog::catalog()?
        .get(source)
        .map(|component| component.primitive.as_str()))
}

/// Check every node of a program against the inferred types and report the
/// position of every boolean condition used by `when` and `unless`.
pub fn check_nodes(nodes: &[Node], env: Env, path: &str) -> DResult<HashMap<String, Pos>> {
    let mut checker = Checker {
        path,
        env,
        locals: Vec::new(),
        conditions: HashMap::new(),
    };
    checker.nodes(nodes)?;
    Ok(checker.conditions)
}

/// `when` and `unless` are IR annotations; verify that every generated field
/// still points to a bool state before an IR file is written.
pub fn check_ir_conditions(
    tree: &Json,
    env: Env,
    conditions: &HashMap<String, Pos>,
    path: &str,
) -> DResult<()> {
    fn walk(node: &Json, env: &Env, conditions: &HashMap<String, Pos>, path: &str) -> DResult<()> {
        for field in ["when", "unless"] {
            if let Some(name) = node.get(field).and_then(Json::as_str) {
                let ty = env.types.get(name).copied();
                if ty != Some(Ty::Bool) {
                    let pos = conditions
                        .get(name)
                        .copied()
                        .unwrap_or(Pos { line: 1, col: 1 });
                    let found = ty.map_or("unknown", Ty::name);
                    return Err(type_error(
                        path,
                        pos,
                        format!("'{field}' condition on '{name}' requires a bool state"),
                        "bool",
                        found,
                        format!("use a bool state or computed bool value for '{field}'"),
                    ));
                }
            }
        }
        for child in node
            .get("children")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            walk(child, &env, conditions, path)?;
        }
        Ok(())
    }
    walk(tree, &env, conditions, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, Ty)]) -> (HashMap<String, Ty>, HashMap<String, Json>, Vec<String>) {
        let mut types = HashMap::new();
        let mut inits = HashMap::new();
        let mut order = Vec::new();
        for (name, ty) in pairs {
            types.insert(name.to_string(), *ty);
            inits.insert(name.to_string(), Json::Null);
            order.push(name.to_string());
        }
        (types, inits, order)
    }

    #[test]
    fn ir_conditions_require_bool_states() {
        let (types, inits, order) = env_of(&[("count", Ty::Int), ("dark", Ty::Bool)]);
        let env = Env {
            types,
            inits,
            computed: HashSet::new(),
            order,
        };
        let mut conditions = HashMap::new();
        conditions.insert("count".to_string(), Pos { line: 3, col: 7 });
        let when = json!({"when": "count", "children": []});
        let err =
            check_ir_conditions(&when, env.clone(), &conditions, "ir.opuir.json").unwrap_err();
        assert_eq!(
            err.render(),
            "ir.opuir.json:3:7: error: 'when' condition on 'count' requires a bool state: expected bool, found int\n  help: use a bool state or computed bool value for 'when'\n"
        );
        let unless = json!({"unless": "count", "children": []});
        let err =
            check_ir_conditions(&unless, env.clone(), &conditions, "ir.opuir.json").unwrap_err();
        assert!(err.render().contains("'unless' condition"));
        let good = json!({"when": "dark", "unless": "dark", "children": []});
        assert!(check_ir_conditions(&good, env, &conditions, "ir.opuir.json").is_ok());
    }
}
