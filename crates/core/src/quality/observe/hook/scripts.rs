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
/// runs the command from that project — provided the whole word sat
/// inside double quotes, which is the only way kendex writes it. Unquoted,
/// the shell word-splits and globs the expansion, so a root holding a
/// space or a glob character runs something other than the one path
/// kendex would read and bind; that spelling is one kendex did not write,
/// and it falls to the said gap.
const PROJECT_ROOT_SPELLINGS: &[&str] =
    &["$CLAUDE_PROJECT_DIR", "$(git rev-parse --show-toplevel)"];

/// Characters this reader treats as dynamic — ones that leave a token's
/// spelling for the shell to finish at run time: a variable, a command or
/// process substitution (its parentheses and redirection arrows included:
/// outside quotes those are operators, so inside a word they can only
/// have come from a substitution or from quoted text), a glob, a tilde,
/// an escape. A token carrying one anywhere but inside kendex's own
/// spelling names a path this audit cannot compute — `bash
/// /tmp/$USER/guard.sh` runs whatever `$USER` expands to, while a literal
/// read of the spelling opens a file that never runs (a planted decoy, or
/// nothing) and binds a decision to it. Such a token falls to the said
/// gap, and it does so whenever a script extension appears *anywhere* in
/// it, not only at its tail: `$(bash /b/evil.sh)`, `<(bash /b/evil.sh)`
/// and `/b/evil.sh$x` all run evil.sh, and a tail-only match would drop
/// each of them in silence. Quote characters are gone by the time this
/// list is consulted, and a quoted `$` the shell keeps literal takes the
/// same gap rather than a literal read — only the managed spellings
/// kendex writes resolve, and those only as it writes them.
const DYNAMIC: &[char] = &['$', '`', '*', '?', '[', '~', '\\', '(', ')', '<', '>'];

/// Every script a hook's command line names — all of them, not the first:
/// `bash /a/ok.sh; bash /b/evil.sh` runs both, and reading only the one a
/// writer put first would bind a decision while the other executes
/// unread. Tokens split on whitespace and operators outside quotes — the
/// `"$(git rev-parse --show-toplevel)/x.sh"` kendex writes carries spaces
/// and parentheses inside its quotes and stays one token — and quote
/// characters are dropped, the way the shell drops them. An interpreter
/// like `/bin/bash` has no script extension and is passed over. A token
/// the shell would still evaluate — one carrying a [`DYNAMIC`] character
/// outside kendex's own spelling — is an unresolvable spelling whenever a
/// script extension appears anywhere in it, never a literal read and
/// never a silent drop; the extension filter runs after that question,
/// not before it.
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
        .filter_map(|token| named(&token, scope))
        .collect()
}

/// What one word says about a script, if it names one. A literal word —
/// kendex's own spelling plus a literal path, or a plain literal path —
/// names a script by its tail extension, the way a file name does, and
/// resolves if it is absolute. A word the shell still evaluates names one
/// wherever the extension sits in it, and never resolves.
fn named(token: &Token, scope: &Scope) -> Option<Named> {
    match literal(token, scope) {
        Some(path) => script_ext(&path).then(|| match path.is_absolute() {
            true => Named::Path(path),
            false => Named::Unresolved(plain(&token.text)),
        }),
        None => names_script(&token.text).then(|| Named::Unresolved(plain(&token.text))),
    }
}

/// The literal path a token spells, with kendex's own project spelling
/// replaced by the scope root it evaluates to — or nothing, when the
/// shell would still have work to do on it. A token carrying no spelling
/// and no [`DYNAMIC`] character passes through; a project spelling outside
/// a project scope, one that is only the *prefix* of a longer identifier,
/// or one written inside single quotes resolves to nothing —
/// `$CLAUDE_PROJECT_DIRS/run.sh` names a different variable and
/// `'$CLAUDE_PROJECT_DIR/run.sh'` is a literal `$` path the shell never
/// expands, so resolving either here would read and bind bytes the harness
/// never runs. An unquoted spelling — any character of the word outside
/// quotes, `"$CLAUDE_PROJECT_DIR"/run.sh` included — resolves to nothing
/// for the reason on [`PROJECT_ROOT_SPELLINGS`]. What is left of the token
/// once the spelling is taken off
/// must be literal too — a [`DYNAMIC`] character in it is refused the same
/// way. The check is on the token's own text, never on the joined path:
/// the scope root is a directory this machine already knows, whatever
/// characters its name holds.
fn literal(token: &Token, scope: &Scope) -> Option<PathBuf> {
    for spelling in PROJECT_ROOT_SPELLINGS {
        let Some(rest) = token.text.strip_prefix(spelling) else {
            continue;
        };
        if !(rest.is_empty() || rest.starts_with('/')) {
            return None;
        }
        // Both flags are token-wide, so a spelling merely adjacent to a
        // single-quoted or an unquoted span is refused too — that falls
        // to the said gap, never to reading the wrong bytes.
        if token.single_quoted || token.unquoted || rest.contains(DYNAMIC) {
            return None;
        }
        let Scope::Project { root } = scope else {
            return None;
        };
        return Some(root.join(rest.trim_start_matches('/')));
    }
    if token.text.contains(DYNAMIC) {
        return None;
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

/// One word of a command line, with the quoting facts resolution needs.
struct Token {
    text: String,
    /// Some of these characters sat inside single quotes, where the shell
    /// expands nothing — a variable spelling in there is a literal `$`.
    single_quoted: bool,
    /// Some of these characters sat outside any quote, where the shell
    /// word-splits and globs whatever an expansion produces. A managed
    /// spelling resolves only when neither flag is set: the whole word
    /// inside double quotes, as kendex writes it.
    unquoted: bool,
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
/// `None` when the line ran out with a quote or a substitution still open,
/// or when a backslash appears outside single quotes. The words read up
/// to there are not the shell's words — a comment, or a spelling this
/// reader does not know, may have moved a boundary — and the caller
/// refuses the whole line instead of trusting them. That rule, not the
/// list of shapes above, is what keeps a desync from reading one script
/// while another runs.
///
/// No escape handling: kendex writes none. A backslash outside single
/// quotes refuses the line whole. Read as text, `\"` inside a
/// substitution would pair up for this reader and close a quote the
/// shell keeps open — `bash $(echo \") ; bash /abs/evil.sh ; true $(echo
/// \")` runs evil.sh while the tail read as one extensionless word, no
/// candidate and no gap — and a reader that cannot tell which quotes are
/// the shell's has no words to vouch for. A full escape reader is
/// KEN-588's job. Inside single quotes a backslash is the literal
/// character the shell keeps: the token carries it, and if that token
/// names a script it falls to the said gap through [`DYNAMIC`].
fn tokens(command: &str) -> Option<Vec<Token>> {
    fn flush(current: &mut String, quoting: &mut (bool, bool), out: &mut Vec<Token>) {
        if !current.is_empty() {
            out.push(Token {
                text: std::mem::take(current),
                single_quoted: quoting.0,
                unquoted: quoting.1,
            });
        }
        *quoting = (false, false);
    }
    let mut out = Vec::new();
    let mut current = String::new();
    // (some character sat in single quotes, some sat outside any quote)
    let mut quoting = (false, false);
    // The innermost open quote, at whatever depth.
    let mut quote: Option<char> = None;
    // Open parentheses of the substitution the word is inside, if any, and
    // the quote that was open when it began — resumed when it closes.
    let mut depth = 0usize;
    let mut resume: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        // Only single quotes make a backslash literal; anywhere else the
        // shell reads it as an escape this reader does not model.
        if c == '\\' && quote != Some('\'') {
            return None;
        }
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
                // The substitution's own quoting is the quote it opened
                // in; its insides inherit that and set no flag of their
                // own.
                quoting.1 |= quote.is_none();
                current.push(c);
                current.push('(');
                chars.next();
                depth = 1;
                resume = quote.take();
            }
            Some(q) => {
                quoting.0 |= q == '\'';
                current.push(c);
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() || SHELL_OPERATORS.contains(&c) => {
                flush(&mut current, &mut quoting, &mut out);
            }
            None => {
                quoting.1 = true;
                current.push(c);
            }
        }
    }
    if depth > 0 || quote.is_some() {
        return None;
    }
    flush(&mut current, &mut quoting, &mut out);
    Some(out)
}

/// Extensions a hook script is written in, matched case-insensitively —
/// `GUARD.SH` is the same script. `ps1` because Copilot hooks run on
/// Windows too; deliberately not shared with the plugin-source list, whose
/// job is different.
fn is_script_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "sh" | "bash" | "zsh" | "py" | "js" | "ts" | "mjs" | "cjs" | "ps1"
    )
}

/// A literal path's tail extension is a script extension.
fn script_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_script_ext)
}

/// A script extension appears anywhere in the word: a `.` followed by an
/// extension and then by something that is not a letter or digit — the
/// end of the word, a `)`, a `$`, a `<`. `.shell` and `.json` are not
/// `.sh` and `.js`; `evil.sh)` and `evil.sh$x` are.
fn names_script(text: &str) -> bool {
    text.split('.').skip(1).any(|after| {
        let run: String = after
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        is_script_ext(&run)
    })
}
