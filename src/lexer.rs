use crate::diag::{Diagnostic, DResult};

// v0.2 constructs are rejected at lex time so they can never be silently miscompiled.
const RESERVED: &[&str] = &["async", "await", "if", "for", "fn", "import"];

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    Bool(bool),
    Sym(String),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn describe(&self) -> String {
        match &self.tok {
            Tok::Ident(s) => format!("'{s}'"),
            Tok::Int(n) => n.to_string(),
            Tok::Str(s) => format!("string \"{s}\""),
            Tok::Bool(b) => b.to_string(),
            Tok::Sym(s) => format!("'{s}'"),
            Tok::Eof => "end of file".to_string(),
        }
    }
}

fn bump(cs: &[char], i: &mut usize, line: &mut usize, col: &mut usize) {
    if *i >= cs.len() {
        return;
    }
    if cs[*i] == '\n' {
        *line += 1;
        *col = 1;
    } else {
        *col += 1;
    }
    *i += 1;
}

pub fn lex(src: &str, path: &str) -> DResult<Vec<Token>> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;

    while i < cs.len() {
        let c = cs[i];
        if c == '/' && i + 1 < cs.len() && cs[i + 1] == '/' {
            while i < cs.len() && cs[i] != '\n' {
                bump(&cs, &mut i, &mut line, &mut col);
            }
            continue;
        }
        if c.is_whitespace() {
            bump(&cs, &mut i, &mut line, &mut col);
            continue;
        }
        let start_line = line;
        let start_col = col;

        if c == '"' {
            bump(&cs, &mut i, &mut line, &mut col);
            let mut s = String::new();
            let mut closed = false;
            while i < cs.len() {
                let d = cs[i];
                if d == '"' {
                    bump(&cs, &mut i, &mut line, &mut col);
                    closed = true;
                    break;
                }
                if d == '\n' {
                    return Err(Diagnostic::new(
                        path,
                        start_line,
                        start_col,
                        "unterminated string literal",
                        "close the string with a double quote before the end of the line",
                    ));
                }
                if d == '\\' {
                    bump(&cs, &mut i, &mut line, &mut col);
                    if i >= cs.len() {
                        break;
                    }
                    match cs[i] {
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        other => {
                            return Err(Diagnostic::new(
                                path,
                                line,
                                col,
                                format!("unknown escape sequence '\\{other}'"),
                                "v0.1 strings support \\\\, \\\", \\n and \\t; write '{{' for a literal '{'",
                            ));
                        }
                    }
                    bump(&cs, &mut i, &mut line, &mut col);
                    continue;
                }
                s.push(d);
                bump(&cs, &mut i, &mut line, &mut col);
            }
            if !closed {
                return Err(Diagnostic::new(
                    path,
                    start_line,
                    start_col,
                    "unterminated string literal",
                    "close the string with a double quote before the end of the file",
                ));
            }
            out.push(Token {
                tok: Tok::Str(s),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < cs.len() && cs[i].is_ascii_digit() {
                s.push(cs[i]);
                bump(&cs, &mut i, &mut line, &mut col);
            }
            let n = s.parse::<i64>().map_err(|_| {
                Diagnostic::new(
                    path,
                    start_line,
                    start_col,
                    format!("integer literal '{s}' is out of range"),
                    "use a value between -9223372036854775808 and 9223372036854775807",
                )
            })?;
            out.push(Token {
                tok: Tok::Int(n),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                s.push(cs[i]);
                bump(&cs, &mut i, &mut line, &mut col);
            }
            if RESERVED.contains(&s.as_str()) {
                return Err(Diagnostic::new(
                    path,
                    start_line,
                    start_col,
                    format!("'{s}' is not yet supported in v0.1"),
                    format!(
                        "remove '{s}' and write state, nodes and event statements; language-level '{s}' arrives in v0.2"
                    ),
                ));
            }
            let tok = match s.as_str() {
                "true" => Tok::Bool(true),
                "false" => Tok::Bool(false),
                _ => Tok::Ident(s),
            };
            out.push(Token {
                tok,
                line: start_line,
                col: start_col,
            });
            continue;
        }

        if i + 1 < cs.len() {
            let pair = [c, cs[i + 1]];
            if (pair == ['+', '=']) || (pair == ['-', '=']) {
                let sym: String = pair.iter().collect();
                bump(&cs, &mut i, &mut line, &mut col);
                bump(&cs, &mut i, &mut line, &mut col);
                out.push(Token {
                    tok: Tok::Sym(sym),
                    line: start_line,
                    col: start_col,
                });
                continue;
            }
        }
        if "{}():,=.;+-".contains(c) {
            bump(&cs, &mut i, &mut line, &mut col);
            out.push(Token {
                tok: Tok::Sym(c.to_string()),
                line: start_line,
                col: start_col,
            });
            continue;
        }
        return Err(Diagnostic::new(
            path,
            start_line,
            start_col,
            format!("unexpected character '{c}'"),
            "v0.1 source uses letters, digits, strings, '//' comments and punctuation",
        ));
    }
    out.push(Token {
        tok: Tok::Eof,
        line,
        col,
    });
    Ok(out)
}
