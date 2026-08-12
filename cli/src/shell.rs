//! Shell rendering for commands vstack advertises for the operator to paste.

/// Quote an argument so the shell the operator will paste into passes it
/// through verbatim.
///
/// Sources, paths and item names reach these commands from consumer input and
/// are accepted with spaces and metacharacters in them; rendered raw, the shell
/// splits or interprets the argument and the advertised recovery runs against
/// something else. The rendering follows the platform, because there is no one
/// form both families read: a POSIX-quoted path pasted into `cmd.exe` keeps its
/// single quotes as literal characters and names a path that does not exist.
pub(crate) fn quote(arg: &str) -> String {
    #[cfg(unix)]
    {
        posix_quote(arg)
    }
    #[cfg(not(unix))]
    {
        cmd_quote(arg)
    }
}

/// POSIX single-quoting: everything inside is literal, and an embedded quote
/// closes, escapes and reopens.
#[cfg(any(unix, test))]
fn posix_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// `cmd.exe` quoting — the Windows shell whose `&&` chaining the advertised
/// commands already use. Double quotes group the argument, and a doubled `""`
/// is how both the callee's argument parsing and PowerShell read an embedded
/// quote.
///
/// A backslash is ordinary to Windows argument parsing everywhere except
/// immediately before a quote, where a run of `2n` backslashes collapses to `n`
/// and the quote stays a delimiter. Every run that lands against a quote — the
/// closing one this adds, or an embedded one — is therefore doubled, or a path
/// ending in `\` (`C:\`) would eat its own closing quote and swallow the next
/// argument.
#[cfg(any(not(unix), test))]
fn cmd_quote(arg: &str) -> String {
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..backslashes * 2 {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push_str("\"\"");
            }
            _ => {
                for _ in 0..backslashes {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    for _ in 0..backslashes * 2 {
        quoted.push('\\');
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::{cmd_quote, posix_quote, quote};

    #[test]
    fn posix_quote_survives_spaces_metacharacters_and_embedded_quotes() {
        assert_eq!(posix_quote("plain"), "'plain'");
        assert_eq!(posix_quote("/my source (v2)"), "'/my source (v2)'");
        assert_eq!(posix_quote("a'b"), r#"'a'\''b'"#);
        assert_eq!(posix_quote(""), "''");
    }

    /// The Windows rendering is not executable on this host, so it is asserted
    /// directly rather than through `quote`.
    #[test]
    fn cmd_quote_groups_spaces_and_doubles_embedded_quotes() {
        assert_eq!(cmd_quote("plain"), "\"plain\"");
        assert_eq!(
            cmd_quote(r"C:\my source (v2)"),
            "\"C:\\my source (v2)\"",
            "a spaced Windows path must be grouped by double quotes"
        );
        assert_eq!(cmd_quote("a\"b"), "\"a\"\"b\"");
        // An apostrophe is an ordinary character to `cmd.exe`: the POSIX
        // `'\''` dance would be pasted through literally.
        assert_eq!(cmd_quote("a'b"), "\"a'b\"");
        assert_eq!(cmd_quote(""), "\"\"");
    }

    /// Control: the two renderings really differ, so a platform that picked the
    /// wrong one could not pass the assertions of the other.
    #[test]
    fn the_two_renderings_disagree_on_a_spaced_path_and_an_apostrophe() {
        for arg in [r"C:\my source (v2)", "a'b"] {
            assert_ne!(
                posix_quote(arg),
                cmd_quote(arg),
                "the renderings agree on {arg:?}, so neither test constrains the platform choice"
            );
        }
    }

    /// The Windows argument parser (`CommandLineToArgvW`), reimplemented so the
    /// rendering can be checked by what it decodes to rather than by an
    /// expected string that only restates the implementation. Backslashes are
    /// literal except immediately before a quote, where `2n` collapse to `n`
    /// and the quote keeps its meaning, `2n+1` collapse to `n` and the quote is
    /// literal, and `""` inside a quoted run is a literal quote.
    fn parse_windows_argv(command_line: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut started = false;
        let mut in_quotes = false;
        let mut chars = command_line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    let mut backslashes = 1usize;
                    while chars.peek() == Some(&'\\') {
                        chars.next();
                        backslashes += 1;
                    }
                    started = true;
                    if chars.peek() == Some(&'"') {
                        for _ in 0..backslashes / 2 {
                            current.push('\\');
                        }
                        if backslashes % 2 == 1 {
                            chars.next();
                            current.push('"');
                        }
                    } else {
                        for _ in 0..backslashes {
                            current.push('\\');
                        }
                    }
                }
                '"' => {
                    started = true;
                    if in_quotes && chars.peek() == Some(&'"') {
                        chars.next();
                        current.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                }
                ' ' if !in_quotes => {
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                _ => {
                    started = true;
                    current.push(ch);
                }
            }
        }
        if started {
            args.push(current);
        }
        args
    }

    /// Every argument the rendering is handed must come back out of the Windows
    /// parser unchanged, and must not bleed into the argument after it.
    #[test]
    fn cmd_quote_survives_backslash_runs_against_quotes_and_the_closing_quote() {
        // Control: the parser can see the failure this pins. The naive
        // rendering — wrap and double quotes only — loses a trailing backslash
        // to the closing quote and swallows the following argument, so a parser
        // that rubber-stamped anything would not report this.
        let naive = format!("--skill \"{}\" --force", r"C:\".replace('"', "\"\""));
        assert_ne!(
            parse_windows_argv(&naive),
            vec![
                "--skill".to_string(),
                r"C:\".to_string(),
                "--force".to_string()
            ],
            "the reference parser accepts an unescaped trailing backslash, so it cannot witness the bug"
        );

        for arg in [
            "plain",
            "",
            r"C:\",               // trailing single backslash
            r"C:\\",              // trailing backslash run
            r"C:\my source (v2)", // backslash away from any quote
            r"C:\my source\",     // spaces and a trailing backslash
            "a\"b",               // embedded quote, no backslashes
            "a\\\"b",             // single backslash before an embedded quote
            "a\\\\\"b",           // backslash run before an embedded quote
            "a'b",                // apostrophes are ordinary to cmd.exe
        ] {
            let line = format!("--skill {} --force", cmd_quote(arg));
            assert_eq!(
                parse_windows_argv(&line),
                vec![
                    "--skill".to_string(),
                    arg.to_string(),
                    "--force".to_string()
                ],
                "rendering {arg:?} as {} does not parse back to it",
                cmd_quote(arg)
            );
        }
    }

    #[test]
    fn quote_dispatches_on_the_host_platform() {
        let arg = "/my source (v2)";
        #[cfg(unix)]
        assert_eq!(quote(arg), posix_quote(arg));
        #[cfg(not(unix))]
        assert_eq!(quote(arg), cmd_quote(arg));
    }
}
