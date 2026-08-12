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
#[cfg(any(not(unix), test))]
fn cmd_quote(arg: &str) -> String {
    format!("\"{}\"", arg.replace('"', "\"\""))
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

    #[test]
    fn quote_dispatches_on_the_host_platform() {
        let arg = "/my source (v2)";
        #[cfg(unix)]
        assert_eq!(quote(arg), posix_quote(arg));
        #[cfg(not(unix))]
        assert_eq!(quote(arg), cmd_quote(arg));
    }
}
