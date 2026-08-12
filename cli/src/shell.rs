//! Shell rendering for commands vstack advertises for the operator to paste.

/// A piece of an advertised command: shell text vstack wrote itself, or an
/// argument that has to reach the program verbatim.
pub(crate) enum Part<'a> {
    Fixed(&'a str),
    Arg(&'a str),
    /// Run everything after this only if everything before it succeeded.
    ///
    /// Written as a part rather than as `Fixed("&&")` because `&&` is not a
    /// Windows PowerShell 5.1 operator — the build that ships with Windows
    /// fails to parse the line before the second command ever runs — so each
    /// shell has to render the sequencing its own way.
    AndThen,
}

/// Render a command for the operator to paste, already wrapped in the backticks
/// the surrounding message shows it in.
///
/// Sources, paths and item names reach these commands from consumer input and
/// are accepted with spaces and metacharacters in them; rendered raw, the shell
/// splits or interprets the argument and the advertised recovery runs against
/// something else.
///
/// unix gets one POSIX line. Windows gets one line per native shell, because no
/// single string reaches both: `cmd.exe` reads a backslash run against a quote
/// as escapes and rewrites `%NAME%` and `!NAME!` even inside double quotes,
/// while PowerShell does neither and instead expands `$name` and `$(...)` and
/// reads a backtick as an escape inside its own double quotes.
pub(crate) fn command(parts: &[Part<'_>]) -> String {
    #[cfg(unix)]
    {
        format!(
            "`{}`",
            render(parts, |arg| Some(posix_quote(arg)), &AND_THEN_OPERATOR)
                .expect("POSIX quoting renders every argument")
        )
    }
    #[cfg(not(unix))]
    {
        windows_command(parts)
    }
}

/// How one shell writes [`Part::AndThen`]: the text that opens the guarded
/// sequence, and the text that closes it once every later part is rendered.
struct Sequencing {
    open: &'static str,
    close: &'static str,
}

/// `&&` reads the same to a POSIX shell and to `cmd.exe`, and neither needs
/// anything closed afterwards.
const AND_THEN_OPERATOR: Sequencing = Sequencing {
    open: " &&",
    close: "",
};

/// PowerShell 5.1 has no `&&`. `;` alone would run the rest regardless of what
/// came before — a `cd` that failed would leave npm installing into the wrong
/// directory — so the rest is guarded on `$?` and closed at the end.
#[cfg(any(not(unix), test))]
const AND_THEN_POWERSHELL: Sequencing = Sequencing {
    open: "; if ($?) {",
    close: " }",
};

fn render(
    parts: &[Part<'_>],
    quote_arg: impl Fn(&str) -> Option<String>,
    sequencing: &Sequencing,
) -> Option<String> {
    let mut rendered = String::new();
    let mut open_sequences = 0usize;
    for part in parts {
        match part {
            // Its own spacing: the opener has to hug what precedes it, because
            // PowerShell's `;` does.
            Part::AndThen => {
                rendered.push_str(sequencing.open);
                open_sequences += 1;
            }
            Part::Fixed(text) => {
                push_separated(&mut rendered, text);
            }
            Part::Arg(arg) => {
                push_separated(&mut rendered, &quote_arg(arg)?);
            }
        }
    }
    for _ in 0..open_sequences {
        rendered.push_str(sequencing.close);
    }
    Some(rendered)
}

fn push_separated(rendered: &mut String, text: &str) {
    if !rendered.is_empty() {
        rendered.push(' ');
    }
    rendered.push_str(text);
}

/// POSIX single-quoting: everything inside is literal, and an embedded quote
/// closes, escapes and reopens.
#[cfg(any(unix, test))]
pub(crate) fn posix_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// The Windows pair. Both shells are offered because vstack cannot know which
/// one the operator pasted into, and neither rendering is safe in the other.
///
/// The `cmd.exe` line is dropped entirely when an argument carries an expansion
/// trigger `cmd.exe` acts on before quoting is considered — see
/// [`cmd_expansion_hazard`]. Advertising the line anyway would hand the
/// operator a command that silently names a different path.
#[cfg(any(not(unix), test))]
pub(crate) fn windows_command(parts: &[Part<'_>]) -> String {
    let powershell = render(
        parts,
        |arg| Some(powershell_quote(arg)),
        &AND_THEN_POWERSHELL,
    )
    .expect("PowerShell single-quoting renders every argument");
    match render(parts, cmd_quote, &AND_THEN_OPERATOR) {
        Some(cmd) => format!("`{cmd}` (cmd.exe) or `{powershell}` (PowerShell)"),
        None => {
            // The refusal names the trigger the operator has to work around,
            // so the first argument carrying one decides the wording.
            let hazard = parts
                .iter()
                .filter_map(|part| match part {
                    Part::Arg(arg) => cmd_expansion_hazard(arg),
                    _ => None,
                })
                .next()
                .expect("a refused rendering carries a hazardous argument");
            format!(
                "`{powershell}` (PowerShell); cmd.exe {hazard}, so it cannot be given this argument verbatim"
            )
        }
    }
}

/// What `cmd.exe` would do to an argument that no quoting there can prevent,
/// or `None` when it would carry it verbatim.
///
/// Percent expansion runs over the whole line before any quote is considered,
/// so a quoted `%NAME%` is still substituted when `NAME` is set and no escape
/// suppresses it inside a quoted argument.
///
/// Delayed expansion is the same hazard one pass later, and vstack cannot see
/// whether it is on: `cmd /V:ON` and the `DelayedExpansion` registry default
/// both enable it, and there is no way to ask from here. With it on, a quoted
/// `!NAME!` is substituted and a lone `!` is deleted; with it off, the `^!` that
/// would have escaped either is itself literal. One line cannot be right on
/// both machines, so the `cmd.exe` line is refused for any `!` rather than
/// rendered wrong on half of them. The refusal is per line, not per argument:
/// a single `!` anywhere on the line also turns `^` into an escape in every
/// other argument on it.
#[cfg(any(not(unix), test))]
fn cmd_expansion_hazard(arg: &str) -> Option<&'static str> {
    if arg.contains('%') {
        return Some("expands %NAME% inside quotes");
    }
    if arg.contains('!') {
        return Some("rewrites ! inside quotes when delayed expansion is enabled");
    }
    None
}

/// `cmd.exe` quoting. Double quotes group the argument and a doubled `""` is
/// how the callee's argument parsing reads an embedded quote.
///
/// A backslash is ordinary to Windows argument parsing everywhere except
/// immediately before a quote, where a run of `2n` backslashes collapses to `n`
/// and the quote stays a delimiter. Every run that lands against a quote — the
/// closing one this adds, or an embedded one — is therefore doubled, or a path
/// ending in `\` (`C:\`) would eat its own closing quote and swallow the next
/// argument.
///
/// `None` when the argument carries an expansion trigger no quoting here can
/// protect — see [`cmd_expansion_hazard`].
#[cfg(any(not(unix), test))]
fn cmd_quote(arg: &str) -> Option<String> {
    if cmd_expansion_hazard(arg).is_some() {
        return None;
    }
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
    Some(quoted)
}

/// PowerShell single-quoting: the only PowerShell string form that is wholly
/// literal. Nothing inside is expanded — not `$name`, not `$(...)`, not a
/// backtick, not a `%`, not a `!` — and a doubled `''` is an embedded quote.
#[cfg(any(not(unix), test))]
fn powershell_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::{Part, cmd_quote, command, posix_quote, powershell_quote, windows_command};
    use std::collections::HashMap;

    fn add_command(source: &str) -> String {
        command(&[
            Part::Fixed("vstack add"),
            Part::Arg(source),
            Part::Fixed("--skill demo"),
        ])
    }

    #[test]
    fn posix_quote_survives_spaces_metacharacters_and_embedded_quotes() {
        assert_eq!(posix_quote("plain"), "'plain'");
        assert_eq!(posix_quote("/my source (v2)"), "'/my source (v2)'");
        assert_eq!(posix_quote("a'b"), r#"'a'\''b'"#);
        assert_eq!(posix_quote(""), "''");
    }

    /// The Windows rendering is not executable on this host, so it is asserted
    /// directly rather than through `command`.
    #[test]
    fn cmd_quote_groups_spaces_and_doubles_embedded_quotes() {
        assert_eq!(cmd_quote("plain").unwrap(), "\"plain\"");
        assert_eq!(
            cmd_quote(r"C:\my source (v2)").unwrap(),
            "\"C:\\my source (v2)\"",
            "a spaced Windows path must be grouped by double quotes"
        );
        assert_eq!(cmd_quote("a\"b").unwrap(), "\"a\"\"b\"");
        // An apostrophe is an ordinary character to `cmd.exe`: the POSIX
        // `'\''` dance would be pasted through literally.
        assert_eq!(cmd_quote("a'b").unwrap(), "\"a'b\"");
        assert_eq!(cmd_quote("").unwrap(), "\"\"");
    }

    /// Control: the renderings really differ, so a platform that picked the
    /// wrong one could not pass the assertions of the others.
    #[test]
    fn the_three_renderings_disagree_on_a_spaced_path_and_an_apostrophe() {
        for arg in [r"C:\my source (v2)", "a'b"] {
            let posix = posix_quote(arg);
            let cmd = cmd_quote(arg).unwrap();
            let powershell = powershell_quote(arg);
            assert_ne!(
                posix, cmd,
                "the renderings agree on {arg:?}, so neither test constrains the platform choice"
            );
            assert_ne!(cmd, powershell, "cmd.exe and PowerShell agree on {arg:?}");
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

    /// `cmd.exe`'s percent expansion, which runs over the whole line before any
    /// quote is considered — quoting cannot suppress it, and a caret cannot
    /// escape it inside quotes because carets are literal there. An undefined
    /// name is left alone, which is the interactive prompt's behaviour.
    fn expand_cmd_percent(command_line: &str, env: &HashMap<&str, &str>) -> String {
        let mut expanded = String::new();
        let mut rest = command_line;
        while let Some(open) = rest.find('%') {
            let (before, after) = rest.split_at(open);
            expanded.push_str(before);
            let body = &after[1..];
            match body.find('%').and_then(|close| {
                env.get(&body[..close])
                    .map(|value| (*value, &body[close + 1..]))
            }) {
                Some((value, tail)) => {
                    expanded.push_str(value);
                    rest = tail;
                }
                None => {
                    expanded.push('%');
                    rest = body;
                }
            }
        }
        expanded.push_str(rest);
        expanded
    }

    /// `cmd.exe`'s delayed expansion, which runs after percent expansion whenever
    /// the shell was started with `/V:ON` or the machine sets the registry
    /// default. Quotes do not suppress it either: a `!NAME!` pair is replaced by
    /// the variable's value — empty when the name is unset — and an unpaired `!`
    /// is dropped outright.
    fn expand_cmd_delayed(command_line: &str, env: &HashMap<&str, &str>) -> String {
        let mut expanded = String::new();
        let mut rest = command_line;
        while let Some(open) = rest.find('!') {
            let (before, after) = rest.split_at(open);
            expanded.push_str(before);
            let body = &after[1..];
            match body.find('!') {
                Some(close) => {
                    expanded.push_str(env.get(&body[..close]).copied().unwrap_or(""));
                    rest = &body[close + 1..];
                }
                // No closing `!`: the bang itself is consumed and the rest of
                // the line is carried through.
                None => rest = body,
            }
        }
        expanded.push_str(rest);
        expanded
    }

    /// PowerShell's own argument parsing: a single-quoted string is wholly
    /// literal with `''` for an embedded quote, while a double-quoted string
    /// expands `$name` and `$(...)` and reads a backtick as an escape.
    fn parse_powershell_argv(command_line: &str, env: &HashMap<&str, &str>) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut started = false;
        let mut chars = command_line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\'' => {
                    started = true;
                    loop {
                        match chars.next() {
                            Some('\'') if chars.peek() == Some(&'\'') => {
                                chars.next();
                                current.push('\'');
                            }
                            Some('\'') | None => break,
                            Some(other) => current.push(other),
                        }
                    }
                }
                '"' => {
                    started = true;
                    loop {
                        match chars.next() {
                            Some('"') if chars.peek() == Some(&'"') => {
                                chars.next();
                                current.push('"');
                            }
                            Some('"') | None => break,
                            Some('`') => {
                                if let Some(escaped) = chars.next() {
                                    current.push(escaped);
                                }
                            }
                            Some('$') if chars.peek() == Some(&'(') => {
                                // A subexpression: its output replaces it, which
                                // for this model is simply "not the literal".
                                for inner in chars.by_ref() {
                                    if inner == ')' {
                                        break;
                                    }
                                }
                                current.push_str("<subexpression>");
                            }
                            Some('$') => {
                                let mut name = String::new();
                                while chars
                                    .peek()
                                    .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
                                {
                                    name.push(chars.next().expect("peeked"));
                                }
                                current.push_str(env.get(name.as_str()).copied().unwrap_or(""));
                            }
                            Some(other) => current.push(other),
                        }
                    }
                }
                ' ' => {
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                other => {
                    started = true;
                    current.push(other);
                }
            }
        }
        if started {
            args.push(current);
        }
        args
    }

    fn windows_env() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("TEMP", r"C:\Users\op\AppData\Local\Temp"),
            ("name", "expanded"),
        ])
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
            "a$b",                // ordinary to cmd.exe, active in PowerShell
            "a`b",
        ] {
            let rendered = cmd_quote(arg).expect("no expansion trigger, so cmd.exe can carry it");
            let line = format!("--skill {rendered} --force");
            // Both expansion passes, in the order `cmd.exe` runs them: an
            // argument the rendering accepts has to survive a shell started
            // with `/V:ON` as well as the default one.
            let expanded =
                expand_cmd_delayed(&expand_cmd_percent(&line, &windows_env()), &windows_env());
            assert_eq!(
                parse_windows_argv(&expanded),
                vec![
                    "--skill".to_string(),
                    arg.to_string(),
                    "--force".to_string()
                ],
                "rendering {arg:?} as {rendered} does not parse back to it"
            );
        }
    }

    /// `cmd.exe` substitutes `%NAME%` inside double quotes, and has no escape
    /// for it there. The rendering must refuse rather than advertise a command
    /// that silently names a different path.
    #[test]
    fn cmd_rendering_refuses_arguments_cmd_exe_would_percent_expand() {
        let env = windows_env();

        // Control: the hazard is real and this model witnesses it — the naive
        // quoted rendering of the path comes back out as a different one.
        let path = r"C:\work\%TEMP%\pkg";
        let naive = format!("vstack add \"{path}\"");
        assert_ne!(
            parse_windows_argv(&expand_cmd_percent(&naive, &env)),
            vec!["vstack".to_string(), "add".to_string(), path.to_string()],
            "the model does not expand a quoted %NAME%, so it cannot witness the bug"
        );

        assert!(cmd_quote(path).is_none());
        assert!(cmd_quote("100%").is_none());

        let advertised = windows_command(&[Part::Fixed("vstack add"), Part::Arg(path)]);
        assert!(
            !advertised.contains("(cmd.exe)"),
            "no cmd.exe line may be advertised for {path}: {advertised}"
        );
        assert!(
            advertised.contains("(PowerShell)"),
            "the operator still needs a command that works: {advertised}"
        );
        // The PowerShell line it falls back to still carries the path verbatim.
        let powershell = advertised
            .split('`')
            .nth(1)
            .expect("the advertised line is backticked");
        assert_eq!(
            parse_powershell_argv(powershell, &env),
            vec!["vstack".to_string(), "add".to_string(), path.to_string()]
        );
    }

    /// `cmd.exe` with delayed expansion enabled rewrites `!` inside double
    /// quotes just as it rewrites `%NAME%`, and the escape that would suppress
    /// it (`^!`) is itself literal when delayed expansion is off — so no single
    /// line is correct on both machines. The rendering must refuse rather than
    /// advertise one that is wrong on half of them.
    #[test]
    fn cmd_rendering_refuses_arguments_delayed_expansion_would_rewrite() {
        let env = windows_env();

        // Control: the hazard is real and this model witnesses both shapes of
        // it — a name pair substituted, and a lone bang deleted.
        for path in [r"C:\work\!TEMP!\pkg", r"C:\work\rev!\pkg"] {
            let naive = format!("vstack add \"{path}\"");
            assert_ne!(
                parse_windows_argv(&expand_cmd_delayed(&expand_cmd_percent(&naive, &env), &env)),
                vec!["vstack".to_string(), "add".to_string(), path.to_string()],
                "the model leaves {path} alone, so it cannot witness the bug"
            );
            assert!(
                cmd_quote(path).is_none(),
                "cmd.exe cannot carry {path} verbatim"
            );

            let advertised = windows_command(&[Part::Fixed("vstack add"), Part::Arg(path)]);
            assert!(
                !advertised.contains("(cmd.exe)"),
                "no cmd.exe line may be advertised for {path}: {advertised}"
            );
            assert!(
                advertised.contains("delayed expansion"),
                "the refusal must name its cause: {advertised}"
            );
            // The operator still needs a command that works, and it must carry
            // the path verbatim.
            let powershell = advertised
                .split('`')
                .nth(1)
                .expect("the advertised line is backticked");
            assert_eq!(
                parse_powershell_argv(powershell, &env),
                vec!["vstack".to_string(), "add".to_string(), path.to_string()]
            );
        }

        // The two refusals are told apart: a percent argument still names
        // percent expansion, not delayed expansion.
        let percent =
            windows_command(&[Part::Fixed("vstack add"), Part::Arg(r"C:\work\%TEMP%\pkg")]);
        assert!(percent.contains("%NAME%"), "{percent}");
        assert!(!percent.contains("delayed expansion"), "{percent}");
    }

    /// PowerShell expands `$name` and `$(...)` and reads a backtick as an
    /// escape inside double quotes, so the `cmd.exe` line is not a PowerShell
    /// line. The PowerShell rendering must survive its own parser.
    #[test]
    fn powershell_rendering_survives_powershell_expansion() {
        let env = windows_env();

        // Control: the cmd.exe rendering really is mangled by PowerShell, so
        // the round trip below is constraining something.
        let expandable = "$name";
        let as_cmd = cmd_quote(expandable).unwrap();
        assert_ne!(
            parse_powershell_argv(&format!("vstack add {as_cmd}"), &env),
            vec![
                "vstack".to_string(),
                "add".to_string(),
                expandable.to_string()
            ],
            "the model does not expand $name inside double quotes, so it cannot witness the bug"
        );

        for arg in [
            "plain",
            "",
            r"C:\work\%TEMP%\pkg",
            r"C:\my source (v2)",
            r"C:\my source\",
            "$name",
            "$(Remove-Item C:\\)",
            "a`b",
            "a'b",
            "a\"b",
            "100%",
            "C:\\work\\!TEMP!\\pkg",
            "rev!",
        ] {
            let rendered = powershell_quote(arg);
            assert_eq!(
                parse_powershell_argv(&format!("vstack add {rendered} --force"), &env),
                vec![
                    "vstack".to_string(),
                    "add".to_string(),
                    arg.to_string(),
                    "--force".to_string()
                ],
                "rendering {arg:?} as {rendered} does not parse back to it"
            );
        }
    }

    /// `&&` is a POSIX and `cmd.exe` operator but not a Windows PowerShell 5.1
    /// one, so the sequencing part must render differently per shell — and the
    /// PowerShell form must still refuse to run the tail when the head failed.
    #[test]
    fn and_then_renders_per_shell_and_still_guards_the_tail() {
        let parts = [
            Part::Fixed("cd"),
            Part::Arg("/my dir"),
            Part::AndThen,
            Part::Fixed("npm ci"),
        ];

        // Control: written as fixed text instead, the operator reaches every
        // shell verbatim — which is the bug this part exists to prevent.
        let as_fixed = windows_command(&[
            Part::Fixed("cd"),
            Part::Arg("/my dir"),
            Part::Fixed("&& npm ci"),
        ]);
        assert!(
            as_fixed.matches("&&").count() == 2,
            "fixed `&&` should reach both shells: {as_fixed}"
        );

        #[cfg(unix)]
        assert_eq!(command(&parts), "`cd '/my dir' && npm ci`");

        let advertised = windows_command(&parts);
        assert!(
            advertised.contains(r#"`cd "/my dir" && npm ci` (cmd.exe)"#),
            "{advertised}"
        );
        assert!(
            advertised.contains("`cd '/my dir'; if ($?) { npm ci }` (PowerShell)"),
            "{advertised}"
        );
        assert_eq!(
            advertised.matches("&&").count(),
            1,
            "only the cmd.exe line may carry `&&`: {advertised}"
        );
    }

    #[test]
    fn advertised_commands_name_the_shell_they_are_for() {
        let unix = "`vstack add '/my source (v2)' --skill demo`";
        #[cfg(unix)]
        assert_eq!(add_command("/my source (v2)"), unix);
        #[cfg(not(unix))]
        assert_ne!(add_command("/my source (v2)"), unix);

        // Windows advertises both native shells, because one string cannot
        // reach both: the same argument is rendered differently for each.
        let both = windows_command(&[
            Part::Fixed("vstack add"),
            Part::Arg(r"C:\my source (v2)"),
            Part::Fixed("--skill demo"),
        ]);
        assert!(both.contains(r#"`vstack add "C:\my source (v2)" --skill demo` (cmd.exe)"#), "{both}");
        assert!(
            both.contains(r"`vstack add 'C:\my source (v2)' --skill demo` (PowerShell)"),
            "{both}"
        );
    }
}
