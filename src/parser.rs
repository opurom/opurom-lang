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
    pub nodes: Vec<Node>,
    pub stmts: Vec<Stmt>,
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

    fn block(&mut self) -> DResult<Block> {
        let start = self.pos();
        self.bump();
        let mut nodes = Vec::new();
        let mut stmts = Vec::new();
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
            if self.word("if") || self.word("for") {
                nodes.push(self.conditional()?);
                continue;
            }
            if is_kind(self.peek()) {
                nodes.push(self.node()?);
                continue;
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
            nodes,
            stmts,
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
        let block = if self.sym("{") {
            Some(self.block()?)
        } else {
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
            let block = self.block()?;
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
        let block = self.block()?;
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
    let mut p = Parser { toks, i: 0, path };
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
        if p.word("if") || p.word("for") {
            nodes.push(p.conditional()?);
            continue;
        }
        if is_kind(p.peek()) {
            nodes.push(p.node()?);
            continue;
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
        states,
        nodes,
        theme,
        models,
        apis,
    })
}
