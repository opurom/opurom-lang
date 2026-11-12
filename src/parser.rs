use crate::diag::{Diagnostic, DResult};
use crate::lexer::{Tok, Token};

#[derive(Clone, Copy, Debug)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub enum Lit {
    Int(i64),
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

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: String,
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
pub struct Program {
    pub app: String,
    pub states: Vec<StateDecl>,
    pub nodes: Vec<Node>,
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
            return Err(Diagnostic::new(
                self.path,
                t.line,
                t.col,
                format!("expected an integer after '-', found {}", t.describe()),
                "negative numbers are written as '-1', not as a separate expression",
            ));
        }
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(n) => {
                self.bump();
                Ok((Value::Lit(Lit::Int(n)), start))
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
                "v0.1 values are integers, strings, true/false or state identifiers",
            )),
        }
    }

    fn arg(&mut self) -> DResult<Arg> {
        if let Tok::Ident(name) = self.peek().tok.clone() {
            let colon = matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Sym(s)) if s == ":");
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
                    "v0.1 supports only 'name.toggle()' on a bool state",
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
                "v0.1 statements are 'x += 1', 'x -= 1', 'x = expr', 'x.toggle()'",
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
            if is_kind(self.peek()) {
                nodes.push(self.node()?);
                continue;
            }
            if self.word("state") || self.word("app") {
                return Err(self.err(
                    "state declarations belong at the top level of the app",
                    "move the declaration directly inside 'app Name { ... }'",
                ));
            }
            if matches!(self.peek().tok, Tok::Ident(_)) {
                stmts.push(self.statement()?);
                continue;
            }
            return Err(self.err(
                format!("expected a node or statement, found {}", self.peek().describe()),
                "container blocks hold nodes; a Button block holds one statement",
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
        let block = if self.sym("{") { Some(self.block()?) } else { None };
        self.eat_sym(";");
        Ok(Node {
            kind,
            pos: start,
            args,
            block,
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
                    "write a number, a string or true/false; state-to-state initialization is not in v0.1",
                ));
            }
        };
        self.eat_sym(";");
        Ok(StateDecl { name, init, pos })
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
        if is_kind(p.peek()) {
            nodes.push(p.node()?);
            continue;
        }
        return Err(p.err(
            format!("expected 'state' or a node kind, found {}", p.peek().describe()),
            "write 'state name = literal' or a node such as Column(gap: 8) { Text(\"hi\") }",
        ));
    }
    if !matches!(p.peek().tok, Tok::Eof) {
        return Err(p.err(
            format!("unexpected token after the app body: {}", p.peek().describe()),
            "keep exactly one 'app Name { ... }' block per .opu file",
        ));
    }
    Ok(Program { app, states, nodes })
}
