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

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::model::{FileState, HarnessId, ObservedItem, Scope};
use crate::source_read::TREE_BOUND;

use super::{AuditInput, Content};

#[cfg(test)]
mod hostile;
#[cfg(test)]
mod resolve;
mod scripts;
#[cfg(test)]
mod substitutions;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod words;

use scripts::{Named, plain, scripts_named};

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
/// The named script candidates do not agree on one file — one command
/// naming two scripts, two registrations naming one each, or a path beside
/// a spelling nobody could resolve — so nothing here can say which bytes
/// the one listed observation stands for.
const HOOK_SCRIPTS_AMBIGUOUS: &str = "this hook's command(s) name more than one script, or one beside a spelling nobody could resolve, so none was read";

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
                        // The location came out of a command string, so it
                        // is laundered of control and invisible characters
                        // like every other command-derived text that
                        // reaches output.
                        plain(&path.display().to_string()),
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

/// How many script candidates a gap reason quotes back. A command can name
/// any number of distinct script-looking tokens; past this many the reason
/// says how many more there were instead of echoing them all.
const ECHOED_CANDIDATES: usize = 8;

/// The one script these same-named registrations invoke, or the reason it
/// was not read. Candidates collect into sets — a command naming the same
/// script twice names one script, and a hostile command naming thousands
/// of distinct ones costs a lookup each, not a scan of everything before.
/// Only a command line is asked for scripts: a URL or a prompt still
/// scores as the hook's content, but the harness runs no file it names,
/// so it names none here and says no gap.
fn script_of(
    registrations: &[crate::scan::hooks::Registration],
    scope: &Scope,
) -> (Option<(PathBuf, Vec<u8>)>, Option<String>) {
    let mut resolved: BTreeSet<PathBuf> = BTreeSet::new();
    let mut unresolved: BTreeSet<String> = BTreeSet::new();
    for reg in registrations {
        if reg.action != crate::scan::hooks::Action::Command {
            continue;
        }
        for named in scripts_named(&reg.command, scope) {
            match named {
                Named::Path(path) => {
                    resolved.insert(path);
                }
                Named::Unresolved(token) => {
                    unresolved.insert(token);
                }
            }
        }
    }
    let mut paths = resolved.into_iter();
    match (paths.next(), paths.next(), unresolved.is_empty()) {
        (None, _, true) => (None, None),
        (None, _, false) => (
            None,
            Some(format!(
                "{HOOK_SCRIPT_UNRESOLVED} ({})",
                echoed(unresolved.into_iter())
            )),
        ),
        (Some(path), None, true) => match read_script(&path) {
            Ok(bytes) => (Some((path, bytes)), None),
            Err(why) => (
                None,
                Some(format!("{why} ({})", plain(&path.display().to_string()))),
            ),
        },
        // The candidates disagree — two distinct paths, or a path beside a
        // spelling nobody could resolve — and nothing here can say which
        // bytes this observation stands for, so none are claimed and the
        // gap is said.
        (first, second, _) => (
            None,
            Some(format!(
                "{HOOK_SCRIPTS_AMBIGUOUS} ({})",
                echoed(
                    first
                        .into_iter()
                        .chain(second)
                        .chain(paths)
                        .map(|p| plain(&p.display().to_string()))
                        .chain(unresolved)
                )
            )),
        ),
    }
}

/// The first [`ECHOED_CANDIDATES`] candidates joined for a reason, and a
/// count of the rest, so a reason stays one line however many a command
/// named.
fn echoed(mut candidates: impl Iterator<Item = String>) -> String {
    let shown: Vec<String> = candidates.by_ref().take(ECHOED_CANDIDATES).collect();
    let rest = candidates.count();
    match rest {
        0 => shown.join(", "),
        _ => format!("{}, and {rest} more", shown.join(", ")),
    }
}

/// The named script's bytes, under the same memory bound every other read
/// answers to. The handle is opened once and asked about itself — a
/// metadata-then-read pair of path lookups would let the file be grown or
/// swapped between them — and the read is capped at the bound so a file
/// that grows under the open handle still cannot exhaust memory.
fn read_script(path: &Path) -> Result<Vec<u8>, &'static str> {
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    // A FIFO opened for reading blocks until a writer appears, which would
    // wedge the whole scan; O_NONBLOCK makes that open return instead and
    // is a no-op on a regular file. A path pre-check could not close this —
    // the file can become a FIFO between the check and the open — so the
    // flag rides the open itself, and the handle's metadata below refuses
    // whatever was opened that is not a regular file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open.custom_flags(libc::O_NONBLOCK);
    }
    let Ok(file) = open.open(path) else {
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
