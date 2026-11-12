use crate::diag::{DResult, Diagnostic};
use crate::lexer::{Tok, Token};

#[derive(Clone, Copy, Debug)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

#[derive(Clone, Debug)]
pub enum Value {
    Lit(Lit),
    Ident(String),
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Value,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Add(Value, Pos),
    Sub(Value, Pos),
    Set(Value, Pos),
    Toggle,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub target: String,
    pub pos: Pos,
    pub kind: StmtKind,
}

/// A compile-time-lowered `await` effect. Effects run in source order before actions.
#[derive(Clone, Debug)]
pub enum Effect {
    Delay {
        ms: i64,
        pos: Pos,
    },
    Fetch {
        path: String,
        into: String,
        pos: Pos,
    },
}

impl Effect {
    pub fn pos(&self) -> Pos {
        match self {
            Effect::Delay { pos, .. } | Effect::Fetch { pos, .. } => *pos,
        }
    }
}

/// A node in the UI tree. `If` and `For` are compile-time forms that are unrolled before lowering.
#[derive(Clone, Debug)]
pub enum NodeKind {
    Widget(String),
    If {
        cond: String,
        cond_pos: Pos,
    },
    For {
        var: String,
        count: usize,
        count_pos: Pos,
    },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub pos: Pos,
    pub args: Vec<Arg>,
    pub block: Option<Block>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub pos: Pos,
    pub is_async: bool,
    pub nodes: Vec<Node>,
    pub stmts: Vec<Stmt>,
    pub effects: Vec<Effect>,
}

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub path: String,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct StateDecl {
    pub name: String,
    pub init: Lit,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct ThemeDecl {
    pub mode: Option<String>,
    pub accent: Option<String>,
    pub radius: Option<i64>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct FieldDecl {
    pub name: String,
    pub ty: String,
    pub primary: bool,
    pub default: Option<Lit>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct ModelDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct HandlerDecl {
    pub verb: String,
    pub model: String,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct ApiDecl {
    pub path: String,
    pub handlers: Vec<HandlerDecl>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub app: String,
    pub imports: Vec<ImportDecl>,
    pub states: Vec<StateDecl>,
    pub nodes: Vec<Node>,
    pub theme: Option<ThemeDecl>,
    pub models: Vec<ModelDecl>,
    pub apis: Vec<ApiDecl>,
}

fn is_kind(t: &Token) -> bool {
    matches!(&t.tok, Tok::Ident(s) if s.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
}

struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    path: &'a str,
    allow_nodes: bool,
    allow_imports_in_body: bool,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn pos(&self) -> Pos {
        let t = self.peek();
        Pos {
            line: t.line,
            col: t.col,
        }
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn sym(&self, s: &str) -> bool {
        matches!(&self.peek().tok, Tok::Sym(x) if x == s)
    }

    fn word(&self, s: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(x) if x == s)
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if self.sym(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn err(&self, message: impl Into<String>, help: impl Into<String>) -> Diagnostic {
        let p = self.pos();
        Diagnostic::new(self.path, p.line, p.col, message, help)
    }

    fn expect_sym(&mut self, s: &str, help: &str) -> DResult<Pos> {
        let t = self.peek().clone();
        if self.eat_sym(s) {
            Ok(Pos {
                line: t.line,
                col: t.col,
            })
        } else {
            Err(Diagnostic::new(
                self.path,
                t.line,
                t.col,
                format!("expected '{s}', found {}", t.describe()),
                help,
            ))
        }
    }

    fn expect_ident(&mut self, what: &str) -> DResult<(String, Pos)> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Ident(s) => {
                let p = Pos {
                    line: t.line,
                    col: t.col,
                };
                self.bump();
                Ok((s, p))
            }
            _ => Err(Diagnostic::new(
                self.path,
                t.line,
                t.col,
                format!("expected {what}, found {}", t.describe()),
                "identifiers start with a letter or '_' and contain letters, digits or '_'",
            )),
        }
    }

    fn value(&mut self) -> DResult<(Value, Pos)> {
        let start = self.pos();
        if self.sym("-") {
            self.bump();
            let t = self.peek().clone();
            if let Tok::Int(n) = t.tok {
                self.bump();
                return Ok((Value::Lit(Lit::Int(-n)), start));
            }
            if let Tok::Float(n) = t.tok {
                self.bump();
                return Ok((Value::Lit(Lit::Float(-n)), start));
            }
            return Err(Diagnostic::new(
                self.path,
                t.line,
                t.col,
                format!("expected a number after '-', found {}", t.describe()),
                "negative numbers are written as '-1', not as a separate expression",
            ));
        }
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(n) => {
                self.bump();
                Ok((Value::Lit(Lit::Int(n)), start))
            }
            Tok::Float(n) => {
                self.bump();
                Ok((Value::Lit(Lit::Float(n)), start))
            }
            Tok::Str(s) => {
                self.bump();
                Ok((Value::Lit(Lit::Str(s)), start))
            }
            Tok::Bool(b) => {
                self.bump();
                Ok((Value::Lit(Lit::Bool(b)), start))
            }
            Tok::Ident(s) => {
                self.bump();
                Ok((Value::Ident(s), start))
            }
            _ => Err(Diagnostic::new(
                self.path,
                t.line,
                t.col,
                format!("expected a literal or identifier, found {}", t.describe()),
                "values are integers, decimals, strings, true/false or state identifiers",
            )),
        }
    }

    fn arg(&mut self) -> DResult<Arg> {
        if let Tok::Ident(name) = self.peek().tok.clone() {
            let colon =
                matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Sym(s)) if s == ":");
            if colon {
                self.bump();
                self.bump();
                let (value, pos) = self.value()?;
                return Ok(Arg {
                    name: Some(name),
                    value,
                    pos,
                });
            }
        }
        let (value, pos) = self.value()?;
        Ok(Arg {
            name: None,
            value,
            pos,
        })
    }

    fn statement(&mut self) -> DResult<Stmt> {
        let start = self.pos();
        let (target, _) = self.expect_ident("a state name")?;
        let kind = if self.eat_sym("+=") {
            let (v, p) = self.value()?;
            StmtKind::Add(v, p)
        } else if self.eat_sym("-=") {
            let (v, p) = self.value()?;
            StmtKind::Sub(v, p)
        } else if self.eat_sym("=") {
            let (v, p) = self.value()?;
            StmtKind::Set(v, p)
        } else if self.eat_sym(".") {
            let (method, mp) = self.expect_ident("'toggle'")?;
            if method != "toggle" {
                return Err(Diagnostic::new(
                    self.path,
                    mp.line,
                    mp.col,
                    format!("unknown method '{method}', expected 'toggle'"),
                    "only 'name.toggle()' on a bool state is supported",
                ));
            }
            self.expect_sym("(", "toggle() needs an opening '('")?;
            self.expect_sym(")", "toggle() needs a closing ')'")?;
            StmtKind::Toggle
        } else {
            return Err(self.err(
                format!(
                    "expected '+=', '-=', '=' or '.toggle()' after '{target}', found {}",
                    self.peek().describe()
                ),
                "statements are 'x += 1', 'x -= 1', 'x = expr', 'x.toggle()'",
            ));
        };
        self.eat_sym(";");
        Ok(Stmt {
            target,
            pos: start,
            kind,
        })
    }

    fn import_decl(&mut self) -> DResult<ImportDecl> {
        let pos = self.pos();
        self.bump();
        let t = self.peek().clone();
        let path = match t.tok {
            Tok::Str(s) => {
                self.bump();
                s
            }
            _ => {
                return Err(Diagnostic::new(
                    self.path,
                    t.line,
                    t.col,
                    format!("expected an import path string, found {}", t.describe()),
                    "write 'import \"models/todo.opu\"' before the app block",
                ));
            }
        };
        self.eat_sym(";");
        Ok(ImportDecl { path, pos })
    }

    fn await_effect(&mut self) -> DResult<Effect> {
        let start = self.pos();
        self.bump();
        let (name, np) = self.expect_ident("'delay' or 'fetch'")?;
        if !matches!(name.as_str(), "delay" | "fetch") {
            return Err(Diagnostic::new(
                self.path,
                np.line,
                np.col,
                format!("unknown effect '{name}'"),
                "write 'await delay(INT)' or 'await fetch(STRING, IDENT)'",
            ));
        }
        self.expect_sym("(", "an effect call needs an opening '('")?;
        let effect = if name == "delay" {
            let negative = self.eat_sym("-");
            let t = self.peek().clone();
            let n = match t.tok {
                Tok::Int(n) => {
                    self.bump();
                    n
                }
                _ => {
                    return Err(Diagnostic::new(
                        self.path,
                        t.line,
                        t.col,
                        format!("'delay' expects an integer, found {}", t.describe()),
                        "write 'await delay(50)' with a millisecond count",
                    ));
                }
            };
            let ms = if negative { -n } else { n };
            if !(0..=60000).contains(&ms) {
                return Err(Diagnostic::new(
                    self.path,
                    start.line,
                    start.col,
                    format!("delay must be between 0 and 60000 ms, found {ms}"),
                    "write a delay from 0 to 60000 milliseconds",
                ));
            }
            self.expect_sym(")", "'delay' needs a closing ')'")?;
            Effect::Delay { ms, pos: start }
        } else {
            let t = self.peek().clone();
            let path = match t.tok {
                Tok::Str(s) => {
                    self.bump();
                    s
                }
                _ => {
                    return Err(Diagnostic::new(
                        self.path,
                        t.line,
                        t.col,
                        format!("'fetch' expects a path string, found {}", t.describe()),
                        "write 'await fetch(\"/todos\", payload)'",
                    ));
                }
            };
            self.expect_sym(",", "'fetch' needs a comma before the target state")?;
            let (into, _) = self.expect_ident("a string state name")?;
            self.expect_sym(")", "'fetch' needs a closing ')'")?;
            Effect::Fetch {
                path,
                into,
                pos: start,
            }
        };
        self.eat_sym(";");
        Ok(effect)
    }

    fn block(&mut self, is_async: bool) -> DResult<Block> {
        let start = self.pos();
        self.bump();
        let mut nodes = Vec::new();
        let mut stmts = Vec::new();
        let mut effects = Vec::new();
        loop {
            self.eat_sym(";");
            if self.eat_sym("}") {
                break;
            }
            if matches!(self.peek().tok, Tok::Eof) {
                return Err(self.err(
                    "unexpected end of file, expected '}'",
                    "close every '{' with a matching '}'",
                ));
            }
            if self.word("state") || self.word("app") {
                return Err(self.err(
                    "state declarations belong at the top level of the app",
                    "move the declaration directly inside 'app Name { ... }'",
                ));
            }
            if self.word("import") {
                return Err(self.err(
                    "'import' is not yet supported in v0.1",
                    "move imports before the 'app' block",
                ));
            }
            if self.word("async") {
                return Err(self.err(
                    "'async' is not yet supported in v0.1",
                    "write 'async' once before a tap-handler block",
                ));
            }
            if self.word("await") {
                let effect = self.await_effect()?;
                if !is_async {
                    return Err(Diagnostic::new(
                        self.path,
                        effect.pos().line,
                        effect.pos().col,
                        "'await' requires an async block",
                        "write 'Button(\"Load\") async { await delay(50) }'",
                    ));
                }
                effects.push(effect);
                continue;
            }
            if self.allow_nodes && (self.word("if") || self.word("for")) {
                nodes.push(self.conditional()?);
                continue;
            }
            if self.allow_nodes && is_kind(self.peek()) {
                nodes.push(self.node()?);
                continue;
            }
            if !self.allow_nodes {
                return Err(self.err(
                    "imported module may declare only state, model, api, theme and imports",
                    "move UI nodes, async handlers and statements into the importing app",
                ));
            }
            if matches!(self.peek().tok, Tok::Ident(_)) {
                stmts.push(self.statement()?);
                continue;
            }
            return Err(self.err(
                format!(
                    "expected a node or statement, found {}",
                    self.peek().describe()
                ),
                "container blocks hold nodes; a Button block holds statements",
            ));
        }
        Ok(Block {
            pos: start,
            is_async,
            nodes,
            stmts,
            effects,
        })
    }

    fn node(&mut self) -> DResult<Node> {
        let start = self.pos();
        let (kind, _) = self.expect_ident("a node kind")?;
        let mut args = Vec::new();
        if self.eat_sym("(") {
            if !self.sym(")") {
                loop {
                    args.push(self.arg()?);
                    if self.eat_sym(",") {
                        if self.sym(")") {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect_sym(")", "close the prop list with ')'")?;
        }
        let mut is_async = false;
        if self.word("async") {
            let ap = self.pos();
            self.bump();
            is_async = true;
            if self.word("async") {
                return Err(Diagnostic::new(
                    self.path,
                    ap.line,
                    ap.col,
                    "duplicate 'async' keyword",
                    "write 'async' once before a tap-handler block",
                ));
            }
        }
        let block = if self.sym("{") {
            Some(self.block(is_async)?)
        } else {
            if is_async {
                return Err(self.err(
                    "'async' requires a tap-handler block",
                    "write 'Button(\"Load\") async { await delay(50) }'",
                ));
            }
            None
        };
        self.eat_sym(";");
        Ok(Node {
            kind: NodeKind::Widget(kind),
            pos: start,
            args,
            block,
        })
    }

    /// `if (cond) { ... }` and `for (i in N) { ... }` share one shape.
    fn conditional(&mut self) -> DResult<Node> {
        let start = self.pos();
        if self.word("if") {
            self.bump();
            self.expect_sym("(", "'if' needs an opening '('")?;
            let (cond, cond_pos) = self.expect_ident("a bool state name")?;
            self.expect_sym(")", "'if' needs a closing ')'")?;
            let block = self.block(false)?;
            return Ok(Node {
                kind: NodeKind::If { cond, cond_pos },
                pos: start,
                args: Vec::new(),
                block: Some(block),
            });
        }
        self.bump();
        self.expect_sym("(", "'for' needs an opening '('")?;
        let (var, _) = self.expect_ident("a loop variable name")?;
        if !self.word("in") {
            return Err(self.err(
                format!(
                    "expected 'in' after '{var}', found {}",
                    self.peek().describe()
                ),
                "write 'for (i in 3)' with an integer count",
            ));
        }
        self.bump();
        let t = self.peek().clone();
        let count = match t.tok {
            Tok::Int(n) if n >= 0 => {
                self.bump();
                n as usize
            }
            Tok::Int(n) => {
                return Err(Diagnostic::new(
                    self.path,
                    t.line,
                    t.col,
                    format!("'for' count must not be negative, found {n}"),
                    "write a count between 0 and 64",
                ))
            }
            _ => {
                return Err(Diagnostic::new(
                    self.path,
                    t.line,
                    t.col,
                    format!("'for' count must be an integer, found {}", t.describe()),
                    "write 'for (i in 3)' with an integer count between 0 and 64",
                ))
            }
        };
        self.expect_sym(")", "'for' needs a closing ')'")?;
        let block = self.block(false)?;
        Ok(Node {
            kind: NodeKind::For {
                var,
                count,
                count_pos: Pos {
                    line: t.line,
                    col: t.col,
                },
            },
            pos: start,
            args: Vec::new(),
            block: Some(block),
        })
    }

    fn state_decl(&mut self) -> DResult<StateDecl> {
        self.bump();
        let (name, pos) = self.expect_ident("a state name")?;
        self.expect_sym("=", "a state declaration needs '=' and an initializer")?;
        let (v, vp) = self.value()?;
        let init = match v {
            Value::Lit(l) => l,
            Value::Ident(s) => {
                return Err(Diagnostic::new(
                    self.path,
                    vp.line,
                    vp.col,
                    format!("state initializer must be a literal, found '{s}'"),
                    "write a number, a string or true/false",
                ));
            }
        };
        self.eat_sym(";");
        Ok(StateDecl { name, init, pos })
    }

    fn theme_decl(&mut self) -> DResult<ThemeDecl> {
        let start = self.pos();
        self.bump();
        self.expect_sym("{", "'theme' needs a '{'")?;
        let mut decl = ThemeDecl {
            mode: None,
            accent: None,
            radius: None,
            pos: start,
        };
        loop {
            self.eat_sym(";");
            if self.eat_sym("}") {
                break;
            }
            if matches!(self.peek().tok, Tok::Eof) {
                return Err(self.err(
                    "unexpected end of file, expected '}'",
                    "close the theme block with a matching '}'",
                ));
            }
            let (key, kp) = self.expect_ident("a theme key")?;
            self.expect_sym(":", "write 'mode: dark' with a colon")?;
            match key.as_str() {
                "mode" => {
                    let (v, vp) = self.value()?;
                    let mode = match v {
                        Value::Ident(s) => s,
                        Value::Lit(_) => {
                            return Err(Diagnostic::new(
                                self.path,
                                vp.line,
                                vp.col,
                                "'mode' expects light, dark or system",
                                "write 'mode: dark'",
                            ))
                        }
                    };
                    if !matches!(mode.as_str(), "light" | "dark" | "system") {
                        return Err(Diagnostic::new(
                            self.path,
                            vp.line,
                            vp.col,
                            format!("unknown theme mode '{mode}'"),
                            "use 'light', 'dark' or 'system'",
                        ));
                    }
                    decl.mode = Some(mode);
                }
                "accent" => {
                    let (v, vp) = self.value()?;
                    let accent = match v {
                        Value::Lit(Lit::Str(s)) => s,
                        _ => {
                            return Err(Diagnostic::new(
                                self.path,
                                vp.line,
                                vp.col,
                                "'accent' expects a '#RRGGBB' color string",
                                "write 'accent: \"#FF4D6D\"'",
                            ))
                        }
                    };
                    decl.accent = Some(accent);
                }
                "radius" => {
                    let (v, vp) = self.value()?;
                    let radius = match v {
                        Value::Lit(Lit::Int(n)) => n,
                        _ => {
                            return Err(Diagnostic::new(
                                self.path,
                                vp.line,
                                vp.col,
                                "'radius' expects an integer",
                                "write 'radius: 8'",
                            ))
                        }
                    };
                    decl.radius = Some(radius);
                }
                other => {
                    return Err(Diagnostic::new(
                        self.path,
                        kp.line,
                        kp.col,
                        format!("unknown theme key '{other}'"),
                        "theme accepts: mode, accent, radius",
                    ));
                }
            }
            self.eat_sym(",");
            self.eat_sym(";");
        }
        Ok(decl)
    }

    fn model_decl(&mut self) -> DResult<ModelDecl> {
        let start = self.pos();
        self.bump();
        let (name, _) = self.expect_ident("a model name")?;
        self.expect_sym("{", "a model body starts with '{'")?;
        let mut fields = Vec::new();
        loop {
            self.eat_sym(";");
            if self.eat_sym("}") {
                break;
            }
            if matches!(self.peek().tok, Tok::Eof) {
                return Err(self.err(
                    "unexpected end of file, expected '}'",
                    "close the model body with a matching '}'",
                ));
            }
            let (fname, fpos) = self.expect_ident("a field name")?;
            self.expect_sym(":", "write 'name: type' with a colon")?;
            let t = self.peek().clone();
            let ty = match t.tok {
                Tok::Ident(s) => {
                    self.bump();
                    s
                }
                _ => {
                    return Err(Diagnostic::new(
                        self.path,
                        t.line,
                        t.col,
                        format!("expected a field type, found {}", t.describe()),
                        "field types are int, string or bool",
                    ))
                }
            };
            let mut primary = false;
            let mut default = None;
            loop {
                self.eat_sym(",");
                if self.word("primary") {
                    self.bump();
                    primary = true;
                    continue;
                }
                if self.sym("=") {
                    self.bump();
                    let (v, vp) = self.value()?;
                    let lit = match v {
                        Value::Lit(l) => l,
                        Value::Ident(s) => {
                            return Err(Diagnostic::new(
                                self.path,
                                vp.line,
                                vp.col,
                                format!("field default must be a literal, found '{s}'"),
                                "write a number, a string or true/false",
                            ))
                        }
                    };
                    default = Some(lit);
                    continue;
                }
                break;
            }
            self.eat_sym(";");
            fields.push(FieldDecl {
                name: fname,
                ty,
                primary,
                default,
                pos: fpos,
            });
        }
        Ok(ModelDecl {
            name,
            fields,
            pos: start,
        })
    }

    fn api_decl(&mut self) -> DResult<ApiDecl> {
        let start = self.pos();
        self.bump();
        let t = self.peek().clone();
        let path = match t.tok {
            Tok::Str(s) => {
                self.bump();
                s
            }
            _ => {
                return Err(Diagnostic::new(
                    self.path,
                    t.line,
                    t.col,
                    format!("expected an API path string, found {}", t.describe()),
                    "write 'api \"/todos\" { ... }'",
                ))
            }
        };
        self.expect_sym("{", "an api body starts with '{'")?;
        let mut handlers = Vec::new();
        loop {
            self.eat_sym(";");
            if self.eat_sym("}") {
                break;
            }
            if matches!(self.peek().tok, Tok::Eof) {
                return Err(self.err(
                    "unexpected end of file, expected '}'",
                    "close the api body with a matching '}'",
                ));
            }
            let (verb, vpos) = self.expect_ident("a handler verb")?;
            let (model, mpos) = self.expect_ident("a model name")?;
            self.eat_sym(";");
            handlers.push(HandlerDecl {
                verb,
                model,
                pos: Pos {
                    line: vpos.line,
                    col: mpos.col,
                },
            });
        }
        Ok(ApiDecl {
            path,
            handlers,
            pos: start,
        })
    }
}

pub fn parse(toks: Vec<Token>, path: &str) -> DResult<Program> {
    parse_with_options(toks, path, true, false)
}

fn parse_with_options(
    toks: Vec<Token>,
    path: &str,
    allow_nodes: bool,
    allow_imports_in_body: bool,
) -> DResult<Program> {
    let mut p = Parser {
        toks,
        i: 0,
        path,
        allow_nodes,
        allow_imports_in_body,
    };
    let mut imports = Vec::new();
    while p.word("import") {
        imports.push(p.import_decl()?);
        p.eat_sym(";");
    }
    if p.word("async") {
        return Err(p.err(
            "'async' is not yet supported in v0.1",
            "write 'async' once before a tap-handler block",
        ));
    }
    if p.word("await") {
        return Err(p.err(
            "'await' requires an async block",
            "write 'Button(\"Load\") async { await delay(50) }'",
        ));
    }
    let (kw, kp) = p.expect_ident("the 'app' keyword")?;
    if kw != "app" {
        return Err(Diagnostic::new(
            path,
            kp.line,
            kp.col,
            format!("expected 'app', found '{kw}'"),
            "an OPUROM program starts with 'app Name { ... }'",
        ));
    }
    let (app, _) = p.expect_ident("an app name")?;
    p.expect_sym("{", "an app body starts with '{'")?;
    let mut states = Vec::new();
    let mut nodes = Vec::new();
    let mut theme = None;
    let mut models = Vec::new();
    let mut apis = Vec::new();
    loop {
        p.eat_sym(";");
        if p.eat_sym("}") {
            break;
        }
        if matches!(p.peek().tok, Tok::Eof) {
            return Err(p.err(
                "unexpected end of file, expected '}'",
                "close the app body with a matching '}'",
            ));
        }
        if p.word("state") {
            states.push(p.state_decl()?);
            continue;
        }
        if p.word("theme") {
            if theme.is_some() {
                return Err(p.err(
                    "duplicate 'theme' block",
                    "declare the theme once inside the app",
                ));
            }
            theme = Some(p.theme_decl()?);
            continue;
        }
        if p.word("model") {
            models.push(p.model_decl()?);
            continue;
        }
        if p.word("api") {
            apis.push(p.api_decl()?);
            continue;
        }
        if p.word("import") {
            if p.allow_imports_in_body {
                imports.push(p.import_decl()?);
                continue;
            }
            return Err(p.err(
                "'import' is not yet supported in v0.1",
                "move imports before the 'app' block",
            ));
        }
        if p.word("async") || p.word("await") {
            if !p.allow_nodes {
                return Err(p.err(
                    "imported module may declare only state, model, api, theme and imports",
                    "move UI nodes, async handlers and statements into the importing app",
                ));
            }
            let message = if p.word("async") {
                "'async' is not yet supported in v0.1"
            } else {
                "'await' requires an async block"
            };
            return Err(p.err(
                message,
                "write 'Button(\"Load\") async { await delay(50) }'",
            ));
        }
        if p.allow_nodes && (p.word("if") || p.word("for")) {
            nodes.push(p.conditional()?);
            continue;
        }
        if p.allow_nodes && is_kind(p.peek()) {
            nodes.push(p.node()?);
            continue;
        }
        if !p.allow_nodes {
            return Err(p.err(
                "imported module may declare only state, model, api, theme and imports",
                "move UI nodes, async handlers and statements into the importing app",
            ));
        }
        return Err(p.err(
            format!(
                "expected 'state' or a node kind, found {}",
                p.peek().describe()
            ),
            "write 'state name = literal' or a node such as Column(gap: 8) { Text(\"hi\") }",
        ));
    }
    if !matches!(p.peek().tok, Tok::Eof) {
        return Err(p.err(
            format!(
                "unexpected token after the app body: {}",
                p.peek().describe()
            ),
            "keep exactly one 'app Name { ... }' block per .opu file",
        ));
    }
    Ok(Program {
        app,
        imports,
        states,
        nodes,
        theme,
        models,
        apis,
    })
}

/// Imported modules accept the same declarations as an app, plus imports, but no UI nodes.
/// A module may use the app wrapper or simply list declarations at the top level.
pub fn parse_module(toks: Vec<Token>, path: &str) -> DResult<Program> {
    let wrapped = if starts_with_app_declaration(&toks) {
        toks
    } else {
        let eof = toks.last().cloned().unwrap_or(Token {
            tok: Tok::Eof,
            line: 1,
            col: 1,
        });
        let mut inner = toks;
        if let Some(last) = inner.last_mut() {
            *last = Token {
                tok: Tok::Sym("}".to_string()),
                line: eof.line,
                col: eof.col,
            };
        }
        let mut out = vec![
            Token {
                tok: Tok::Ident("app".to_string()),
                line: 1,
                col: 1,
            },
            Token {
                tok: Tok::Ident("Module".to_string()),
                line: 1,
                col: 1,
            },
            Token {
                tok: Tok::Sym("{".to_string()),
                line: 1,
                col: 1,
            },
        ];
        out.extend(inner);
        out.push(Token {
            tok: Tok::Eof,
            line: eof.line,
            col: eof.col,
        });
        out
    };
    let program = parse_with_options(wrapped, path, false, true)?;
    if let Some(node) = program.nodes.first() {
        return Err(Diagnostic::new(
            path,
            node.pos.line,
            node.pos.col,
            "imported module may declare only state, model, api, theme and imports",
            "move UI nodes, async handlers and statements into the importing app",
        ));
    }
    Ok(program)
}

fn starts_with_app_declaration(toks: &[Token]) -> bool {
    let mut i = 0;
    loop {
        while matches!(
            toks.get(i),
            Some(Token {
                tok: Tok::Sym(s),
                ..
            }) if s == ";"
        ) {
            i += 1;
        }
        if matches!(
            toks.get(i),
            Some(Token {
                tok: Tok::Ident(s),
                ..
            }) if s == "import"
        ) {
            i += 1;
            if !matches!(
                toks.get(i),
                Some(Token {
                    tok: Tok::Str(_),
                    ..
                })
            ) {
                return false;
            }
            i += 1;
            if matches!(
                toks.get(i),
                Some(Token {
                    tok: Tok::Sym(s),
                    ..
                }) if s == ";"
            ) {
                i += 1;
            }
            continue;
        }
        return matches!(
            toks.get(i),
            Some(Token {
                tok: Tok::Ident(s),
                ..
            }) if s == "app"
        );
    }
}
