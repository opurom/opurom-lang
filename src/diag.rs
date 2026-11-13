use std::fmt;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub path: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub help: String,
}

pub type DResult<T> = Result<T, Diagnostic>;

impl Diagnostic {
    pub fn new(
        path: &str,
        line: usize,
        col: usize,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self {
            path: path.to_string(),
            line,
            col,
            message: message.into(),
            help: help.into(),
        }
    }

    pub fn file(path: &str, message: impl Into<String>, help: impl Into<String>) -> Self {
        Self::new(path, 1, 1, message, help)
    }

    /// A type diagnostic keeps the original message and names both sides of the mismatch.
    pub fn type_error(
        path: &str,
        line: usize,
        col: usize,
        message: impl Into<String>,
        expected: impl Into<String>,
        found: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self::new(
            path,
            line,
            col,
            format!(
                "{}: expected {}, found {}",
                message.into(),
                expected.into(),
                found.into()
            ),
            help,
        )
    }

    pub fn render(&self) -> String {
        format!(
            "{}:{}:{}: error: {}\n  help: {}\n",
            self.path, self.line, self.col, self.message, self.help
        )
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}
