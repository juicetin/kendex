//! A hook, read as the rules should see it.
//!
//! A hook observed inside a shared config file is scored on its own
//! registration — the command the harness will run, and the script that
//! command invokes — never on the rest of the file. Sibling entries and the
//! `permissions.ask`/`permissions.deny` lists are not this hook's content:
//! an ask-list entry is a guard *against* a dangerous command, and scoring
//! it under every hook's name turned one `mkfs` guard into a high-severity
//! finding on all fifteen hooks in the file (KEN-558). A hook that is its
//! own file — opencode's instruction carrier — is still read whole, because
//! there the file is the hook.

use std::path::{Path, PathBuf};

use crate::model::{FileState, HarnessId, ObservedItem};
use crate::source_read::TREE_BOUND;

use super::{AuditInput, Content};

/// A hook found inside a shared config file whose registration was not
/// there when the audit re-read the file.
const UNREAD_HOOK_ENTRY: &str =
    "this hook's registration was not found in the config file that was scanned for it";
/// A registry file that would not parse holds no entry to dig out.
const HOOK_REGISTRY_UNPARSED: &str = "the config file holding this hook's registration could not be parsed, so none of it was scored";
/// The script exists but is bigger than kendex reads into memory. Like a
/// tree past the bound, a hook whose script cannot be read is not scored at
/// all — scoring the command line alone would report "clean" over the part
/// that actually runs.
const HOOK_SCRIPT_TOO_BIG: &str = "the script this hook's command invokes is larger than kendex reads into memory, so none of the hook was scored";
const HOOK_SCRIPT_UNREADABLE: &str = "the script this hook's command invokes could not be read from disk, so none of the hook was scored";

/// What this hook observation gives the rules to read.
pub(super) fn hook_input(item: &ObservedItem) -> AuditInput {
    let at = |location: String, content: Content| AuditInput {
        kind: item.kind,
        name: item.name.clone(),
        harness: Some(item.harness),
        location,
        content,
    };
    let registry = item.path.display().to_string();
    if item.file_state != FileState::ConfigEntry {
        let content = match super::read_document(&item.path) {
            Content::Document { text } => Content::Hook {
                event: String::new(),
                matcher: None,
                command: registry.clone(),
                script: Some(text),
            },
            unread => unread,
        };
        return at(registry, content);
    }
    match hook_reading(item) {
        Err(why) => at(registry, Content::Unread { why }),
        Ok(reading) => {
            // Findings in the script belong to the script's own file; with
            // no script the registry file is the only location there is.
            // The same choice the gate makes, so the two paths report one
            // location for one problem.
            let location = reading
                .script
                .as_ref()
                .map(|(path, _)| path.display().to_string())
                .unwrap_or(registry);
            let first = &reading.registrations[0];
            let command = reading
                .registrations
                .iter()
                .map(|reg| reg.command.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            at(
                location,
                Content::Hook {
                    event: first.event.clone(),
                    matcher: Some(first.matcher.clone())
                        .filter(|m| m != crate::scan::hooks::ANY_MATCHER),
                    command,
                    script: reading
                        .script
                        .map(|(_, bytes)| String::from_utf8_lossy(&bytes).into_owned()),
                },
            )
        }
    }
}

/// One config-entry hook's own bytes: every registration in its file that
/// answers to this observation's name, and the script those registrations
/// invoke. Both readers of a hook — the rules, and the review hash a
/// decision binds to — go through here, so what a dismissal covers can
/// never drift from what the rules read.
pub(crate) struct HookReading {
    /// Every registration the file holds under this name. Names collide
    /// only when event, matcher and command stem all agree, so the one
    /// observation the scan lists stands for all of them and all of them
    /// are read.
    pub(crate) registrations: Vec<crate::scan::hooks::Registration>,
    pub(crate) script: Option<(PathBuf, Vec<u8>)>,
}

/// Dig this hook's registration back out of the config file it lives in.
///
/// The file is parsed by the same reader the scan chose for it — Copilot's
/// entries carry their action inline, every other harness nests handlers
/// under a matcher group — and matched by the same name construction the
/// scan listed it under, so an entry the scan found is the entry this
/// reads. `Err` is the one reason there is no reading; a hook whose script
/// cannot be read is refused whole rather than scored on the half that
/// opened.
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
    let mut paths: Vec<PathBuf> = registrations
        .iter()
        .filter_map(|reg| script_named(&reg.command))
        .collect();
    paths.dedup();
    // Two same-named registrations naming two different scripts is a
    // reading nothing here can attribute, so neither script is read and
    // the command lines answer alone.
    let script = match paths.as_slice() {
        [path] => Some((path.clone(), read_script(path)?)),
        _ => None,
    };
    Ok(HookReading {
        registrations,
        script,
    })
}

/// The script file a hook's command line invokes, when the line names one
/// plainly: the first token that is an absolute path with a script
/// extension. Quotes are trimmed the way `command_stem` trims them. A
/// command that reaches its script through a variable or a relative path
/// names no file an observation can open, and an interpreter like
/// `/bin/bash` has no script extension — resolving nothing leaves the
/// command line to answer alone.
fn script_named(command: &str) -> Option<PathBuf> {
    command
        .split_whitespace()
        .map(|token| token.trim_matches('"').trim_matches('\''))
        .map(Path::new)
        .find(|token| token.is_absolute() && super::is_source(token))
        .map(Path::to_path_buf)
}

/// The named script's bytes, under the same memory bound every other read
/// answers to. Every failure is the whole hook's — a script that is gone,
/// unopenable, or too large to hold is content the harness was told to run
/// and kendex could not read, and scoring the command line alone would
/// report "clean" over the part that runs.
fn read_script(path: &Path) -> Result<Vec<u8>, &'static str> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Err(HOOK_SCRIPT_UNREADABLE);
    };
    if !meta.is_file() {
        return Err(HOOK_SCRIPT_UNREADABLE);
    }
    if TREE_BOUND.past(1, meta.len()) {
        return Err(HOOK_SCRIPT_TOO_BIG);
    }
    std::fs::read(path).map_err(|_| HOOK_SCRIPT_UNREADABLE)
}
