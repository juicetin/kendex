//! The bytes a decision is about.
//!
//! `content_hash` names what the rules read, and the rules read a *reduced*
//! representation: symlinks are stepped over, a binary asset contributes its
//! path and its byte count and nothing else, and text is decoded lossily so
//! two different invalid bytes collapse into one replacement character. That
//! is the right input for scoring and the wrong one for a decision. A plugin
//! whose only file is `payload.wasm` reduces to nothing at all: swap the
//! payload for different bytes of the same length and the representation,
//! the findings and the hash are all unchanged, so a recorded decision goes
//! on speaking for content nobody reviewed.
//!
//! This is the other hash. Every owned byte, or the exact config entry, with
//! no decoding at all. A decision binds to it, and the flag that grants one
//! carries it. Where the bytes cannot be reached at all the
//! answer is `None`: a decision with nothing to compare against must never
//! read as live, which is the same rule that reports an artifact kendex
//! cannot compare as uncompared rather than as passing.
//!
//! A hook's hash follows what its rules read on each path. The gate reads
//! the script this plan would write and the entry it would add, and binds
//! both. The audit digs the entry back out of the shared settings file it
//! landed in and follows its command to the script on disk — kendex's own
//! project spellings included — and binds exactly that: the entry and the
//! script's bytes, never the rest of the file. The rules do not read
//! sibling entries or the permission lists (KEN-558), and a hash over
//! bytes the rules never read would stale a hook's decisions every time
//! anything else in the settings file moved. A command that names a script
//! the audit could not read or resolve binds nothing at all — a decision
//! must not answer for bytes nobody opened — while a command-bodied hook
//! binds its entry alone and round-trips from gate to install like a
//! server entry does.

use std::path::PathBuf;

use serde_json::Value;

use crate::configedit::ConfigEdit;
use sha2::{Digest, Sha256};

use crate::hash::{hash_bytes, hash_files};
use crate::model::{ItemKind, ObservedItem};

use super::desired::{Artifact, Desired};

/// What this plan would install, hashed before a byte of it is written.
///
/// Sealed by the kind the artifact *is* on disk, not the kind it is
/// declared as: a Codex command is written and scanned back as a skill
/// tree, and a decision that sealed itself as a command would read as
/// absent the moment the audit looked.
pub(super) fn desired(item: &Desired) -> Option<String> {
    Some(seal(installed_kind(item), &inner_hash(item)?))
}

fn inner_hash(item: &Desired) -> Option<String> {
    Some(match &item.artifact {
        Artifact::File { bytes, .. } => hash_bytes(bytes),
        Artifact::Tree { files, .. } => hash_files(files),
        Artifact::Registration { script, edits } => registration(script.as_ref(), edits)?,
    })
}

fn installed_kind(item: &Desired) -> ItemKind {
    item.emitted
        .as_ref()
        .map_or(item.kind, |emitted| emitted.kind)
}

/// What is installed here right now, read back off disk. A config-entry
/// hook's reading is handed in by the caller — the same one its findings
/// were scored from — never re-read here, or the hash could describe a
/// different snapshot than the score.
pub(super) fn observed(
    item: &ObservedItem,
    hook_reading: Option<&Result<crate::quality::observe::HookReading, &'static str>>,
) -> Option<String> {
    let inner = match item.kind {
        ItemKind::Skill | ItemKind::Plugin => match item.path.is_dir() {
            true => owned_tree(&item.path)?,
            false => return None,
        },
        ItemKind::Agent | ItemKind::Command | ItemKind::PiExtension => {
            hash_bytes(&std::fs::read(&item.path).ok()?)
        }
        ItemKind::Hook => match item.file_state {
            crate::model::FileState::ConfigEntry => hook_config_entry(hook_reading?)?,
            // A hook that is its own file — opencode's instruction carrier —
            // is read whole, so it binds whole.
            _ => hash_bytes(&std::fs::read(&item.path).ok()?),
        },
        ItemKind::McpServer => hash_bytes(
            canonical(&crate::quality::observe::mcp_entry(&item.path, &item.name)?).as_bytes(),
        ),
    };
    Some(seal(item.kind, &inner))
}

/// The whole tree, every byte of it — the same construction as the hash a
/// rendered tree gets before it is written, so the two readings agree. A
/// link inside the tree is hashed as a link, by where it points, and never
/// read through: the scoring walk stops at links for the same reason (what
/// is past one is somebody else's files under this item's name), and
/// following one would also turn an audit refresh into an unbounded read
/// of wherever the link leads. The item's own path is followed, since a
/// harness-native link to the canonical tree is how a shared skill is
/// installed and that tree is what the tool loads.
fn owned_tree(root: &std::path::Path) -> Option<String> {
    let mut hasher = Sha256::new();
    walk(&mut hasher, root, std::path::Path::new(""), 0).ok()?;
    Some(crate::hash::hex(&hasher.finalize()))
}

/// Deeper than any rendered tree goes.
const MAX_DEPTH: usize = 32;

fn walk(
    hasher: &mut Sha256,
    path: &std::path::Path,
    rel: &std::path::Path,
    depth: usize,
) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(std::io::Error::other("nested too deep"));
    }
    let meta = match depth {
        0 => std::fs::metadata(path)?,
        _ => std::fs::symlink_metadata(path)?,
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(b"->");
        hasher.update(target.to_string_lossy().as_bytes());
        hasher.update([0]);
    } else if meta.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            let Some(name) = entry.file_name() else {
                continue;
            };
            walk(hasher, &entry, &rel.join(name), depth + 1)?;
        }
    } else if meta.is_file() {
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(path)?);
        hasher.update([0]);
    } else {
        return Err(std::io::Error::other("not a regular file or directory"));
    }
    Ok(())
}

/// The kind is folded in so no two kinds' material can be the same string.
fn seal(kind: ItemKind, inner: &str) -> String {
    hash_bytes(format!("{}|{inner}", kind.name()).as_bytes())
}

/// An entry inside shared harness config: the backing script's bytes, the
/// registration itself, or both. `None` where the plan writes neither — a
/// plugin is one switch in a settings file and a removal has no entry at
/// all, so there is nothing for a decision to bind to.
///
/// A hook's entry text is built by the same functions that write the file
/// (`configedit::handler_json` / `copilot_entry_json`), spelled by the same
/// `owned_entry` the scanner's read-back uses — so the entry the gate binds
/// and the entry the audit digs back out are one text by construction.
fn registration(
    script: Option<&(PathBuf, Vec<u8>)>,
    edits: &[(PathBuf, ConfigEdit)],
) -> Option<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut servers: Vec<String> = Vec::new();
    for (_, edit) in edits {
        match edit {
            ConfigEdit::UpsertHook {
                event,
                matcher,
                command,
                timeout,
            } => entries.push(crate::scan::hooks::owned_entry(
                event,
                crate::configedit::spelled(matcher.as_deref()),
                &crate::configedit::handler_json(command, *timeout),
            )),
            ConfigEdit::UpsertCopilotHook {
                event,
                matcher,
                command,
                timeout,
            } => entries.push(crate::scan::hooks::owned_entry(
                event,
                crate::configedit::spelled(matcher.as_deref()),
                &crate::configedit::copilot_entry_json(matcher.as_deref(), command, *timeout),
            )),
            ConfigEdit::UpsertMcpServer { value, .. } => servers.push(canonical(value)),
            _ => {}
        }
    }
    if !servers.is_empty() {
        // The MCP construction predates the hook one and stays as it is:
        // changing it would stale every server decision already recorded.
        let mut parts: Vec<String> = Vec::new();
        if let Some((_, bytes)) = script {
            parts.push(hash_bytes(bytes));
        }
        parts.append(&mut servers);
        return Some(hash_bytes(parts.join("|").as_bytes()));
    }
    hook_material(script.map(|(_, bytes)| bytes.as_slice()), entries)
}

/// A config-entry hook binds to what its rules read: the registrations
/// under its name — every field of each owning entry — and the bytes of
/// the script they invoke: the same reading the rules just scored, through
/// the same [`hook_material`] the gate builds for a plan, so where the
/// script resolves to the file the plan wrote, a decision taken at the
/// gate recognises the install once the write lands. A script the reading
/// named but could not read binds nothing at all: a decision must not
/// answer for bytes nobody opened.
fn hook_config_entry(
    reading: &Result<crate::quality::observe::HookReading, &'static str>,
) -> Option<String> {
    let reading = reading.as_ref().ok()?;
    if reading.script_unread.is_some() {
        return None;
    }
    hook_material(
        reading.script.as_ref().map(|(_, bytes)| bytes.as_slice()),
        reading
            .registrations
            .iter()
            .map(crate::scan::hooks::Registration::owned_text),
    )
}

/// A hook's binding material, assembled in exactly one place for the gate
/// and the audit — hand-parallel assemblies would drift silently, and the
/// failure mode of a drift is a decision that never recognises its own
/// install. Every part is a fixed-width digest before the join, so a
/// command string carrying the join character cannot forge a boundary
/// between parts: distinct part sets cannot collide.
fn hook_material(
    script: Option<&[u8]>,
    entries: impl IntoIterator<Item = String>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(bytes) = script {
        parts.push(hash_bytes(bytes));
    }
    parts.extend(entries.into_iter().map(|text| hash_bytes(text.as_bytes())));
    match parts.is_empty() {
        true => None,
        false => Some(hash_bytes(parts.join("|").as_bytes())),
    }
}

/// `value` as text with object keys in one order — see
/// [`crate::hash::canonical_json`]: the JSON reader preserves the order it
/// found, and a decision must not go stale because somebody moved a key.
fn canonical(value: &Value) -> String {
    crate::hash::canonical_json(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reader keeps insertion order, so the same entry written two ways
    /// must still hash the same — a moved key is not a content change.
    #[test]
    fn key_order_does_not_change_an_entry() {
        let first: Value =
            serde_json::from_str(r#"{"command":"node","args":["a"],"env":{"B":"2","A":"1"}}"#)
                .unwrap();
        let second: Value =
            serde_json::from_str(r#"{"env":{"A":"1","B":"2"},"args":["a"],"command":"node"}"#)
                .unwrap();
        assert_eq!(canonical(&first), canonical(&second));
    }

    /// And a value that actually moved is a different entry.
    #[test]
    fn a_changed_value_changes_an_entry() {
        let first: Value = serde_json::from_str(r#"{"args":["a"]}"#).unwrap();
        let second: Value = serde_json::from_str(r#"{"args":["b"]}"#).unwrap();
        assert_ne!(canonical(&first), canonical(&second));
    }
}
