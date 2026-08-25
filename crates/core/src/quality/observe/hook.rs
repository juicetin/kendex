//! A hook, read as the rules should see it.
//!
//! A hook observed inside a shared config file is scored on its own
//! registration — the full entry the harness loads, the command it will
//! run, and the script that command invokes — never on the rest of the
//! file. Sibling entries and the `permissions.ask`/`permissions.deny`
//! lists are not this hook's content: an ask-list entry is a guard
//! *against* a dangerous command, and scoring it under every hook's name
//! turned one `mkfs` guard into a high-severity finding on all fifteen
//! hooks in the file (KEN-558). A hook that is its own file — opencode's
//! instruction carrier — is still read whole, because there the file is
//! the hook.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::model::{FileState, HarnessId, ObservedItem, Scope};
use crate::source_read::TREE_BOUND;

use super::{AuditInput, Content};

/// A hook found inside a shared config file whose registration was not
/// there when the audit re-read the file.
const UNREAD_HOOK_ENTRY: &str =
    "this hook's registration was not found in the config file that was scanned for it";
/// A registry file that would not parse holds no entry to dig out.
const HOOK_REGISTRY_UNPARSED: &str = "the config file holding this hook's registration could not be parsed, so none of it was scored";
/// The script exists but is bigger than kendex reads into memory.
const HOOK_SCRIPT_TOO_BIG: &str =
    "the script this hook's command invokes is larger than kendex reads into memory";
const HOOK_SCRIPT_UNREADABLE: &str =
    "the script this hook's command invokes could not be read from disk";
/// The command names a script by a spelling this audit cannot resolve to a
/// file — a relative path, or a variable kendex did not write.
const HOOK_SCRIPT_UNRESOLVED: &str =
    "the script this hook's command names could not be resolved to a file on this machine";
/// Same-named registrations point at different scripts, so nothing here
/// can say which bytes the one listed observation stands for.
const HOOK_SCRIPTS_AMBIGUOUS: &str =
    "same-named registrations of this hook name different scripts, so neither was read";

/// What this hook observation gives the rules to read, computing its own
/// reading. [`super::score`] reads through [`config_entry_input`] instead,
/// so its findings and its review hash come from one snapshot.
pub(super) fn hook_input(item: &ObservedItem) -> AuditInput {
    if item.file_state != FileState::ConfigEntry {
        return file_backed_input(item);
    }
    config_entry_input(item, &hook_reading(item))
}

/// A hook that is its own file: the file is the hook, and all of it is the
/// script.
fn file_backed_input(item: &ObservedItem) -> AuditInput {
    let at = item.path.display().to_string();
    let content = match super::read_document(&item.path) {
        Content::Document { text } => Content::Hook {
            event: String::new(),
            matcher: None,
            command: at.clone(),
            entry: None,
            script: Some((at.clone(), text)),
            script_unread: None,
        },
        unread => unread,
    };
    AuditInput {
        kind: item.kind,
        name: item.name.clone(),
        harness: Some(item.harness),
        location: at,
        content,
    }
}

/// A config-entry hook's audit input from a reading already taken. The
/// location is the registry file — the command doc and the entry doc are
/// labeled inside it, and only script findings point at the script's own
/// path.
pub(crate) fn config_entry_input(
    item: &ObservedItem,
    reading: &Result<HookReading, &'static str>,
) -> AuditInput {
    let registry = item.path.display().to_string();
    let content = match reading {
        Err(why) => Content::Unread { why },
        Ok(reading) => {
            let first = &reading.registrations[0];
            Content::Hook {
                event: first.event.clone(),
                matcher: Some(first.matcher.clone())
                    .filter(|m| m != crate::scan::hooks::ANY_MATCHER),
                command: reading
                    .registrations
                    .iter()
                    .map(|reg| reg.command.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                entry: Some(
                    reading
                        .registrations
                        .iter()
                        .map(crate::scan::hooks::Registration::scanned_text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                script: reading.script.as_ref().map(|(path, bytes)| {
                    (
                        path.display().to_string(),
                        String::from_utf8_lossy(bytes).into_owned(),
                    )
                }),
                script_unread: reading.script_unread.clone(),
            }
        }
    };
    AuditInput {
        kind: item.kind,
        name: item.name.clone(),
        harness: Some(item.harness),
        location: registry,
        content,
    }
}

/// One config-entry hook's own bytes: every registration in its file that
/// answers to this observation's name, and the script those registrations
/// invoke. Both readers of a hook — the rules, and the review hash a
/// decision binds to — read one of these, so what a dismissal covers can
/// never drift from what the rules read.
pub struct HookReading {
    /// Every registration the file holds under this name. Names collide
    /// only when event, matcher and command stem all agree, so the one
    /// observation the scan lists stands for all of them and all of them
    /// are read.
    pub(crate) registrations: Vec<crate::scan::hooks::Registration>,
    pub(crate) script: Option<(PathBuf, Vec<u8>)>,
    /// A script the commands name that was not read, and why — resolution
    /// failed, two candidates disagreed, or the file refused. The command
    /// and entry still score; a decision must not bind while this is set,
    /// because part of what would run was never read.
    pub(crate) script_unread: Option<String>,
}

/// Dig this hook's registration back out of the config file it lives in.
///
/// The file is parsed by the same reader the scan chose for its harness —
/// Copilot's entries carry their action inline; every other harness shares
/// one shape, with or without a matcher group — and matched by the same
/// name construction the scan listed it under, so an entry the scan found
/// is the entry this reads. `Err` means there is no entry to score at all;
/// a script that could not be read is carried as `script_unread` instead,
/// because the command line and the entry are complete readings of their
/// own and a missing script must not silence them.
pub(crate) fn hook_reading(item: &ObservedItem) -> Result<HookReading, &'static str> {
    let text = match crate::fs::read_if_exists(&item.path) {
        Ok(Some(text)) => text,
        _ => return Err(super::UNREADABLE_FILE),
    };
    let parsed = match item.harness {
        HarnessId::Copilot => crate::scan::copilot::registrations_text(&text),
        _ => crate::scan::hooks::registrations_text(&text),
    };
    let Ok(mut registrations) = parsed else {
        return Err(HOOK_REGISTRY_UNPARSED);
    };
    registrations.retain(|reg| reg.name() == item.name);
    if registrations.is_empty() {
        return Err(UNREAD_HOOK_ENTRY);
    }
    let (script, script_unread) = script_of(&registrations, &item.scope);
    Ok(HookReading {
        registrations,
        script,
        script_unread,
    })
}

/// The one script these same-named registrations invoke, or the reason it
/// was not read.
fn script_of(
    registrations: &[crate::scan::hooks::Registration],
    scope: &Scope,
) -> (Option<(PathBuf, Vec<u8>)>, Option<String>) {
    let mut resolved: Vec<PathBuf> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for reg in registrations {
        match script_named(&reg.command, scope) {
            Named::Path(path) if !resolved.contains(&path) => resolved.push(path),
            Named::Path(_) => {}
            Named::Unresolved(token) => unresolved.push(token),
            Named::None => {}
        }
    }
    match (resolved.as_slice(), unresolved.as_slice()) {
        ([], []) => (None, None),
        ([], [token, ..]) => (None, Some(format!("{HOOK_SCRIPT_UNRESOLVED} ({token})"))),
        ([path], []) => match read_script(path) {
            Ok(bytes) => (Some((path.clone(), bytes)), None),
            Err(why) => (None, Some(format!("{why} ({})", path.display()))),
        },
        // Two candidates — or one plus a spelling nobody could resolve —
        // and nothing here can say which bytes this observation stands
        // for, so none are claimed and the gap is said.
        (many, _) => (
            None,
            Some(format!(
                "{HOOK_SCRIPTS_AMBIGUOUS} ({})",
                many.iter()
                    .map(|p| p.display().to_string())
                    .chain(unresolved.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        ),
    }
}

/// What one command line says about its script.
enum Named {
    /// A file this machine can open: an absolute path with a script
    /// extension, after kendex's own project spellings are resolved.
    Path(PathBuf),
    /// A token that is plainly a script — it has the extension — reached
    /// through a spelling this audit cannot resolve.
    Unresolved(String),
    /// No token looks like a script; the command is the whole content.
    None,
}

/// The spellings kendex itself registers a project hook's script under —
/// see `engine::targets`. Each resolves to the scope's own root, which is
/// exactly what the variable or substitution evaluates to when the harness
/// runs the command from that project.
const PROJECT_ROOT_SPELLINGS: &[&str] =
    &["$CLAUDE_PROJECT_DIR", "$(git rev-parse --show-toplevel)"];

/// The script file a hook's command line invokes. Tokens split on
/// whitespace outside quotes — `$(git rev-parse --show-toplevel)` carries
/// spaces and must stay one token — and quote characters are dropped, the
/// way the shell drops them. An interpreter like `/bin/bash` has no script
/// extension and is passed over.
fn script_named(command: &str, scope: &Scope) -> Named {
    for token in tokens(command) {
        if !script_ext(Path::new(&token)) {
            continue;
        }
        let resolved = resolve_root(&token, scope);
        return match resolved {
            Some(path) if path.is_absolute() => Named::Path(path),
            _ => Named::Unresolved(token),
        };
    }
    Named::None
}

/// `token` with kendex's own project spelling replaced by the scope root
/// it evaluates to. A token carrying no spelling passes through; a project
/// spelling outside a project scope resolves to nothing.
fn resolve_root(token: &str, scope: &Scope) -> Option<PathBuf> {
    for spelling in PROJECT_ROOT_SPELLINGS {
        let Some(rest) = token.strip_prefix(spelling) else {
            continue;
        };
        let Scope::Project { root } = scope else {
            return None;
        };
        return Some(root.join(rest.trim_start_matches('/')));
    }
    Some(PathBuf::from(token))
}

/// Whitespace-splitting that honors quotes, dropping the quote characters
/// the way the shell does. No escape handling: kendex writes none, and a
/// hand-written command that needs them simply resolves no script.
fn tokens(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Extensions a hook script is written in. `ps1` because Copilot hooks run
/// on Windows too; deliberately not shared with the plugin-source list,
/// whose job is different.
fn script_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("sh" | "bash" | "zsh" | "py" | "js" | "ts" | "mjs" | "cjs" | "ps1")
    )
}

/// The named script's bytes, under the same memory bound every other read
/// answers to. The handle is opened once and asked about itself — a
/// metadata-then-read pair of path lookups would let the file be grown or
/// swapped between them — and the read is capped at the bound so a file
/// that grows under the open handle still cannot exhaust memory.
fn read_script(path: &Path) -> Result<Vec<u8>, &'static str> {
    // Refuse a FIFO or device before open: opening a FIFO for reading
    // blocks until a writer appears, which would wedge the whole scan.
    // The handle's own metadata below re-answers authoritatively for
    // anything that changed in between.
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        _ => return Err(HOOK_SCRIPT_UNREADABLE),
    }
    let Ok(file) = std::fs::File::open(path) else {
        return Err(HOOK_SCRIPT_UNREADABLE);
    };
    let Ok(meta) = file.metadata() else {
        return Err(HOOK_SCRIPT_UNREADABLE);
    };
    if !meta.is_file() {
        return Err(HOOK_SCRIPT_UNREADABLE);
    }
    if TREE_BOUND.past(1, meta.len()) {
        return Err(HOOK_SCRIPT_TOO_BIG);
    }
    let mut bytes = Vec::new();
    match (&file).take(TREE_BOUND.bytes + 1).read_to_end(&mut bytes) {
        Ok(_) if !TREE_BOUND.past(1, bytes.len() as u64) => Ok(bytes),
        Ok(_) => Err(HOOK_SCRIPT_TOO_BIG),
        Err(_) => Err(HOOK_SCRIPT_UNREADABLE),
    }
}
