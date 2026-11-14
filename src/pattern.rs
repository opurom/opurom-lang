//! Bounded regex-lite syntax accepted by `Input(pattern: ...)`.
//!
//! shortcut: only literals, `.`, character classes/ranges and `?`/`*`/`+`
//! are accepted; upgrade when a full regex engine is available.

const MAX_CHARS: usize = 64;
const MAX_ATOMS: usize = 32;

pub fn validate(pattern: &str) -> Result<(), String> {
    let cs: Vec<char> = pattern.chars().collect();
    if cs.is_empty() {
        return Err("pattern is empty".to_string());
    }
    if cs.len() > MAX_CHARS {
        return Err(format!(
            "pattern has {} characters; at most {MAX_CHARS} are allowed",
            cs.len()
        ));
    }
    let mut start = 0;
    if cs[0] == '^' {
        start = 1;
    }
    let end = if cs.last() == Some(&'$') {
        cs.len() - 1
    } else {
        cs.len()
    };
    if start >= end {
        return Err("pattern has no matchable atom".to_string());
    }
    let mut i = start;
    let mut atoms = 0;
    while i < end {
        match cs[i] {
            '[' => character_class(&cs, &mut i, end)?,
            '.' => i += 1,
            '\\' => return Err("backslash escapes are not supported".to_string()),
            '(' | ')' | '|' | '{' | '}' | '^' | '$' => {
                return Err(format!("unsupported regex syntax '{}'", cs[i]));
            }
            '*' | '+' | '?' => {
                return Err(format!("quantifier '{}' has no preceding atom", cs[i]));
            }
            c if c.is_ascii_graphic() || c == ' ' => i += 1,
            c => return Err(format!("unsupported character '{c}'")),
        }
        if i < end && matches!(cs[i], '?' | '*' | '+') {
            i += 1;
            if i < end && matches!(cs[i], '?' | '*' | '+' | '{' | '}' | '(' | ')' | '|') {
                return Err(format!("unsupported quantifier sequence at '{}'", cs[i]));
            }
        }
        atoms += 1;
        if atoms > MAX_ATOMS {
            return Err(format!(
                "pattern has more than {MAX_ATOMS} atoms; split it or simplify it"
            ));
        }
    }
    Ok(())
}

fn character_class(cs: &[char], i: &mut usize, end: usize) -> Result<(), String> {
    *i += 1;
    if *i < end && cs[*i] == '^' {
        *i += 1;
    }
    let mut found = false;
    while *i < end && cs[*i] != ']' {
        let c = cs[*i];
        if matches!(c, '[' | '\\' | '(' | ')' | '|' | '{' | '}' | '^' | '$') {
            return Err(format!("unsupported character '{c}' in a class"));
        }
        if *i + 2 < end && cs[*i + 1] == '-' && cs[*i + 2] != ']' {
            let lo = c;
            let hi = cs[*i + 2];
            if !lo.is_ascii_alphanumeric() || !hi.is_ascii_alphanumeric() {
                return Err("character ranges must use ASCII letters or digits".to_string());
            }
            if lo > hi {
                return Err(format!("reversed character range '{lo}-{hi}'"));
            }
            *i += 3;
        } else {
            if !(c.is_ascii_graphic() || c == ' ') {
                return Err(format!("unsupported character '{c}' in a class"));
            }
            *i += 1;
        }
        found = true;
    }
    if *i >= end {
        return Err("unterminated character class".to_string());
    }
    if !found {
        return Err("empty character class".to_string());
    }
    *i += 1;
    Ok(())
}
