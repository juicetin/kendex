//! How a command line names the scripts it runs.
//!
//! A hook's command is read the way the shell reads it — words split on
//! unquoted whitespace and unquoted operators, quote characters dropped, a
//! command substitution kept whole — because what the audit resolves has
//! to be what the harness executes. A line this reader cannot follow to
//! its end is refused whole rather than read by guesswork. Everything
//! here answers one question: which files does this line name, and can
//! this machine open them?

use std::path::{Path, PathBuf};

use crate::model::Scope;

/// What one command line says about one script it names.
pub(super) enum Named {
    /// A file this machine can open: an absolute path with a script
    /// extension, after kendex's own project spellings are resolved.
    Path(PathBuf),
    /// A token that is plainly a script — it has the extension — reached
    /// through a spelling this audit cannot resolve.
    Unresolved(String),
}

/// The spellings kendex itself registers a project hook's script under —
/// see `engine::targets`. Each resolves to the scope's own root, which is
/// exactly what the variable or substitution evaluates to when the harness
/// runs the command from that project.
const PROJECT_ROOT_SPELLINGS: &[&str] =
    &["$CLAUDE_PROJECT_DIR", "$(git rev-parse --show-toplevel)"];

/// Every script a hook's command line names — all of them, not the first:
/// `bash /a/ok.sh; bash /b/evil.sh` runs both, and reading only the one a
/// writer put first would bind a decision while the other executes
/// unread. Tokens split on whitespace and operators outside quotes — the
/// `"$(git rev-parse --show-toplevel)/x.sh"` kendex writes carries spaces
/// and parentheses inside its quotes and stays one token — and quote
/// characters are dropped, the way the shell drops them. An interpreter
/// like `/bin/bash` has no script extension and is passed over.
///
/// A line the reader could not follow to its end — a quote or a
/// substitution still open when the text runs out — has no words this
/// audit can vouch for, so the whole line is one unresolvable spelling:
/// whatever the shell makes of it, nothing was read and the gap is said.
/// Reading the words as far as they were understood would let "read
/// nothing" coincide with "said nothing", which is the one outcome this
/// module exists to rule out.
pub(super) fn scripts_named(command: &str, scope: &Scope) -> Vec<Named> {
    let Some(tokens) = tokens(command) else {
        return vec![Named::Unresolved(plain(command))];
    };
    tokens
        .into_iter()
        .filter(|token| script_ext(Path::new(&token.text)))
        .map(|token| match resolve_root(&token, scope) {
            Some(path) if path.is_absolute() => Named::Path(path),
            _ => Named::Unresolved(plain(&token.text)),
        })
        .collect()
}

/// The token with kendex's own project spelling replaced by the scope root
/// it evaluates to. A token carrying no spelling passes through; a project
/// spelling outside a project scope, one that is only the *prefix* of a
/// longer identifier, or one written inside single quotes resolves to
/// nothing — `$CLAUDE_PROJECT_DIRS/run.sh` names a different variable and
/// `'$CLAUDE_PROJECT_DIR/run.sh'` is a literal `$` path the shell never
/// expands, so resolving either here would read and bind bytes the harness
/// never runs.
fn resolve_root(token: &Token, scope: &Scope) -> Option<PathBuf> {
    for spelling in PROJECT_ROOT_SPELLINGS {
        let Some(rest) = token.text.strip_prefix(spelling) else {
            continue;
        };
        if !(rest.is_empty() || rest.starts_with('/')) {
            return None;
        }
        // The flag is token-wide, so a spelling merely adjacent to a
        // single-quoted span is refused too — that falls to the said gap,
        // never to reading the wrong bytes.
        if token.single_quoted {
            return None;
        }
        let Scope::Project { root } = scope else {
            return None;
        };
        return Some(root.join(rest.trim_start_matches('/')));
    }
    Some(PathBuf::from(&token.text))
}

/// `text` with every control character, bidi control (U+202A–U+202E,
/// U+2066–U+2069) and zero-width character (U+200B–U+200D, U+FEFF)
/// replaced, so a path a hostile config embeds terminal escapes or
/// direction overrides in cannot recolor, rewrite or visually reverse the
/// output it is reported through — then run through the same redactor a
/// finding's message goes through, so a path segment that is an issued
/// token travels as its fingerprint, never as the key. Applied where
/// command-derived text enters a reason or a location; every consumer
/// downstream inherits it.
pub(super) fn plain(text: &str) -> String {
    let invisible = |c: char| {
        matches!(
            c,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}'
        )
    };
    let stripped: String = text
        .chars()
        .map(|c| match c.is_control() || invisible(c) {
            true => '\u{FFFD}',
            false => c,
        })
        .collect();
    crate::quality::redact(&stripped)
}

/// One word of a command line, with the quoting fact resolution needs.
struct Token {
    text: String,
    /// Some of these characters sat inside single quotes, where the shell
    /// expands nothing — a variable spelling in there is a literal `$`.
    single_quoted: bool,
}

/// Characters the shell reads as operators outside quotes and outside a
/// substitution. Each one ends the word before it, wherever it sits:
/// `bash /b/evil.sh;bash /a/ok.sh` names both scripts, and
/// `x.sh>/dev/null` names `x.sh`. Leaving an operator glued to a path
/// would hide the script from extension matching and let it execute
/// unread.
pub(super) const SHELL_OPERATORS: &[char] = &[';', '&', '|', '(', ')', '<', '>'];

/// Word-splitting that honors quotes: unquoted whitespace and unquoted
/// operators end a word, quote characters are dropped the way the shell
/// drops them, and everything inside quotes — an operator included — is
/// part of the word. The operators themselves are never words: no script
/// is spelled `;`.
///
/// A substitution — `$(`, `<(` or `>(` through its matching `)` — is part
/// of the word it sits in, parentheses nested inside it counted rather
/// than split on. Splitting `$(pwd)/x.sh` at its parentheses would leave
/// `/x.sh` standing alone as an absolute path: kendex would read a file at
/// the root of this machine and bind a decision to it while the harness
/// runs the copy under whatever `pwd` says. Kept whole, the word begins
/// with `$` and falls to the said gap, the way every spelling kendex did
/// not write does. Quotes inside a substitution are tracked the way they
/// are outside one: a parenthesis inside them is text, not nesting, so
/// `$(echo ")")` closes where the shell closes it and the words after it
/// are the shell's words. `$(` opens a substitution inside double quotes
/// too, as it does for the shell; the quote it interrupted resumes when
/// the substitution closes.
///
/// `None` when the line ran out with a quote or a substitution still open.
/// The words read up to there are not the shell's words — an escape, a
/// comment, or a spelling this reader does not know may have moved a
/// boundary — and the caller refuses the whole line instead of trusting
/// them. That rule, not the list of shapes above, is what keeps a desync
/// from reading one script while another runs. No escape handling: kendex
/// writes none, and a hand-written command that needs them either still
/// reads the way the shell does or falls to this refusal.
fn tokens(command: &str) -> Option<Vec<Token>> {
    fn flush(current: &mut String, single_quoted: &mut bool, out: &mut Vec<Token>) {
        if !current.is_empty() {
            out.push(Token {
                text: std::mem::take(current),
                single_quoted: *single_quoted,
            });
        }
        *single_quoted = false;
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut single_quoted = false;
    // The innermost open quote, at whatever depth.
    let mut quote: Option<char> = None;
    // Open parentheses of the substitution the word is inside, if any, and
    // the quote that was open when it began — resumed when it closes.
    let mut depth = 0usize;
    let mut resume: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if depth > 0 {
            current.push(c);
            match (quote, c) {
                (Some(q), _) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '(') => depth += 1,
                (None, ')') => {
                    depth -= 1;
                    if depth == 0 {
                        quote = resume.take();
                    }
                }
                _ => {}
            }
            continue;
        }
        let opens = |c: char, quote: Option<char>| match quote {
            None => matches!(c, '$' | '<' | '>'),
            Some('"') => c == '$',
            Some(_) => false,
        };
        match quote {
            Some(q) if c == q => quote = None,
            _ if opens(c, quote) && chars.peek() == Some(&'(') => {
                current.push(c);
                current.push('(');
                chars.next();
                depth = 1;
                resume = quote.take();
            }
            Some(q) => {
                single_quoted |= q == '\'';
                current.push(c);
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() || SHELL_OPERATORS.contains(&c) => {
                flush(&mut current, &mut single_quoted, &mut out);
            }
            None => current.push(c),
        }
    }
    if depth > 0 || quote.is_some() {
        return None;
    }
    flush(&mut current, &mut single_quoted, &mut out);
    Some(out)
}

/// Extensions a hook script is written in, matched case-insensitively —
/// `GUARD.SH` is the same script. `ps1` because Copilot hooks run on
/// Windows too; deliberately not shared with the plugin-source list, whose
/// job is different.
fn script_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("sh" | "bash" | "zsh" | "py" | "js" | "ts" | "mjs" | "cjs" | "ps1")
    )
}
