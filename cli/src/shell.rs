//! Shell rendering for commands vstack advertises for the operator to paste.

/// Single-quote an argument so a shell passes it through verbatim.
///
/// Sources, paths and item names reach these commands from consumer input and
/// are accepted with spaces and metacharacters in them; rendered raw, the shell
/// splits or interprets the argument and the advertised recovery runs against
/// something else. Quoting unconditionally keeps one rendering to reason about.
pub(crate) fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn quote_survives_spaces_metacharacters_and_embedded_quotes() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("/my source (v2)"), "'/my source (v2)'");
        assert_eq!(quote("a'b"), r#"'a'\''b'"#);
        assert_eq!(quote(""), "''");
    }
}
