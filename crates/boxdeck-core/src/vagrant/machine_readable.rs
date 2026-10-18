//! Parser for `vagrant --machine-readable` output: `timestamp,target,type,data...` per line.
//!
//! Vagrant escapes only commas (`%!(VAGRANT_COMMA)`), newlines (`\n`) and carriage returns (`\r`)
//! inside fields, and never escapes backslashes, so a literal `\n` in the data can't be told apart
//! from an escaped newline. Lines that don't match the format (plain text from early errors or
//! commands like `ssh-config`) are skipped by [`parse`] and kept by [`split`].

use crate::error::Error;

/// One machine-readable line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub timestamp: u64,
    /// Machine name, or empty for environment-wide lines (and always empty in `global-status`).
    pub target: String,
    pub kind: String,
    /// Data fields, already unescaped.
    pub data: Vec<String>,
}

impl Line {
    /// First data field, or "" when there is none.
    pub fn first(&self) -> &str {
        self.data.first().map(String::as_str).unwrap_or("")
    }
}

/// Undoes Vagrant's field escaping.
pub fn unescape(field: &str) -> String {
    field
        .replace("%!(VAGRANT_COMMA)", ",")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
}

/// Parses one line; `None` if it isn't machine-readable.
pub fn parse_line(line: &str) -> Option<Line> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut parts = line.splitn(4, ',');
    let timestamp = parts.next()?;
    if timestamp.is_empty() || !timestamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let timestamp = timestamp.parse().ok()?;
    let target = parts.next()?;
    let kind = parts.next()?;
    if kind.is_empty() {
        return None;
    }
    let data = match parts.next() {
        Some(rest) => rest.split(',').map(unescape).collect(),
        None => Vec::new(),
    };
    Some(Line {
        timestamp,
        target: unescape(target),
        kind: kind.to_owned(),
        data,
    })
}

/// All machine-readable lines in `output`, in order. Unknown types are kept; callers ignore them.
pub fn parse(output: &str) -> Vec<Line> {
    output.lines().filter_map(parse_line).collect()
}

/// Machine-readable lines plus the plain-text lines that weren't.
pub fn split(output: &str) -> (Vec<Line>, Vec<&str>) {
    let mut lines = Vec::new();
    let mut other = Vec::new();
    for raw in output.lines() {
        match parse_line(raw) {
            Some(line) => lines.push(line),
            None => other.push(raw),
        }
    }
    (lines, other)
}

/// Turns a failed run into an error: the `error-exit` line when there is one, otherwise the
/// plain text (Vagrant reports early errors such as Vagrantfile syntax errors only as text).
pub fn failure(stdout: &str, stderr: &str, exit_code: Option<i32>) -> Error {
    let (lines, other) = split(stdout);
    if let Some(exit) = lines.iter().find(|l| l.kind == "error-exit") {
        return Error::Vagrant {
            class: exit.data.first().cloned(),
            message: exit.data.get(1).cloned().unwrap_or_default(),
            exit_code,
        };
    }
    let mut message = other.join("\n").trim().to_owned();
    if message.is_empty() {
        message = stderr.trim().to_owned();
    }
    if message.is_empty() {
        message = format!("exit code {exit_code:?} with no output");
    }
    Error::Vagrant {
        class: None,
        message,
        exit_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_line() {
        let l = parse_line("1792353581,web,state,running").unwrap();
        assert_eq!(l.timestamp, 1792353581);
        assert_eq!(l.target, "web");
        assert_eq!(l.kind, "state");
        assert_eq!(l.data, vec!["running"]);
    }

    #[test]
    fn unescapes_commas_and_newlines() {
        let l = parse_line("1,,ui,info,a%!(VAGRANT_COMMA) b\\nc\\r").unwrap();
        assert_eq!(l.data, vec!["info", "a, b\nc\r"]);
    }

    #[test]
    fn keeps_multiple_fields_and_empty_ones() {
        let l = parse_line("1,,box-info,homepage,https://x/a%!(VAGRANT_COMMA)b").unwrap();
        assert_eq!(l.data, vec!["homepage", "https://x/a,b"]);
        let l = parse_line("1,,ui,info,").unwrap();
        assert_eq!(l.data, vec!["info", ""]);
    }

    #[test]
    fn line_without_data() {
        let l = parse_line("1,web,metadata").unwrap();
        assert!(l.data.is_empty());
        assert_eq!(l.first(), "");
    }

    #[test]
    fn rejects_plain_text() {
        assert!(parse_line("Current machine states:").is_none());
        assert!(parse_line("").is_none());
        assert!(parse_line("12a,web,state,running").is_none());
        assert!(parse_line("1,web").is_none());
        assert!(parse_line("1,web,").is_none());
    }

    #[test]
    fn tolerates_crlf() {
        let l = parse_line("1,web,state,running\r").unwrap();
        assert_eq!(l.data, vec!["running"]);
    }

    #[test]
    fn failure_prefers_error_exit() {
        let out =
            "1,,ui,error,boom\n1,,error-exit,Vagrant::Errors::X,boom%!(VAGRANT_COMMA) badly\n";
        match failure(out, "", Some(1)) {
            Error::Vagrant { class, message, .. } => {
                assert_eq!(class.as_deref(), Some("Vagrant::Errors::X"));
                assert_eq!(message, "boom, badly");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn failure_falls_back_to_text_then_stderr() {
        match failure("\nVagrant failed early\n", "", Some(1)) {
            Error::Vagrant { class, message, .. } => {
                assert!(class.is_none());
                assert_eq!(message, "Vagrant failed early");
            }
            other => panic!("unexpected {other:?}"),
        }
        match failure("", "ruby crashed\n", Some(1)) {
            Error::Vagrant { message, .. } => assert_eq!(message, "ruby crashed"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
