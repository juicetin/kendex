//! What each desired artifact gives the safety rules to read.

use crate::configedit::ConfigEdit;
use crate::model::ItemKind;
use crate::quality::{AuditInput, Authored, Content, McpEntry, UNREADABLE_PLUGIN};

use super::super::desired::{Artifact, Desired};

/// What this item's rendering gives the rules to read.
pub(in crate::engine) fn input_for(item: &Desired) -> AuditInput {
    let (location, content) = match &item.artifact {
        Artifact::File { path, bytes } => (
            path.display().to_string(),
            Content::Document {
                text: String::from_utf8_lossy(bytes).into_owned(),
            },
        ),
        // Read through the same constructor the observed audit uses, so the
        // two paths score and hash one construction.
        Artifact::Tree {
            canonical, files, ..
        } => (
            canonical.display().to_string(),
            crate::quality::observe::tree_content_from_bytes(files),
        ),
        Artifact::Registration { script, edits } => registration(item, script.as_ref(), edits),
    };
    AuditInput {
        kind: item.kind,
        name: item.name.clone(),
        harness: Some(item.harness),
        location,
        content,
    }
}

/// How much of this item its publisher wrote — what their record is
/// allowed to answer for, and nothing else.
///
/// The builder that rendered the artifact reports it; nothing here derives
/// it back out of the rendered text, because text a project supplied can
/// carry anything, marker or otherwise. The match is exhaustive on purpose:
/// a kind whose rendering starts splicing in project text has to be
/// classified here, and a kind that should have reported one and did not
/// reads as unreadable — so the record settles nothing and the plan says a
/// carried review did not apply, which is the direction a mistake here has
/// to fail in.
pub(in crate::engine) fn authored_for(item: &Desired) -> Authored {
    match (&item.authored, item.kind) {
        (Some(authored), _) => authored.clone(),
        // Nothing in these renderings comes from the project: the
        // publisher's bytes are the whole of what is read, so there is no
        // block in them and no line of one.
        //
        // This arm is a standing obligation on the renderers, and the one
        // half of this match the compiler cannot hold. A new kind cannot be
        // added without answering here — that part it does hold. But
        // teaching one of these kinds to splice in project text compiles
        // clean and silently widens every publisher review for that kind,
        // which is the failure this whole match exists to prevent. Adding
        // project text to any kind listed here means moving it to the arm
        // below and making its builder report where that text went.
        (
            None,
            ItemKind::Command
            | ItemKind::Hook
            | ItemKind::McpServer
            | ItemKind::Plugin
            | ItemKind::PiExtension,
        ) => Authored::Around(None),
        // These two carry project text — `[skill-instructions]`, and
        // everything in `desired_agent::Project`. Their builders owe an
        // answer beside the artifact; reaching here means one did not give
        // it, which a body-cap refusal, a harness that cannot express the
        // agent, and instructions whose block the rendering does not carry
        // all do. Unreadable is the answer: nothing is settled, and the
        // plan says a carried review did not apply.
        (None, ItemKind::Skill | ItemKind::Agent) => Authored::Rendered {
            publishers: Content::Unread {
                why: "kendex could not tell this item's own content from this project's",
            },
            supplied: std::collections::BTreeSet::new(),
        },
    }
}

type Script = (std::path::PathBuf, Vec<u8>);

fn registration(
    item: &Desired,
    script: Option<&Script>,
    edits: &[(std::path::PathBuf, ConfigEdit)],
) -> (String, Content) {
    // The registry file, the way the audit locates the same hook: command
    // and entry findings belong to the file that carries them, and only
    // script findings to the script.
    let location = edits
        .first()
        .map(|(path, _)| path.display().to_string())
        .or_else(|| script.map(|(path, _)| path.display().to_string()))
        .unwrap_or_else(|| item.name.clone());
    let content = match item.kind {
        ItemKind::McpServer => match mcp_entry(edits) {
            Some(entry) => Content::Mcp(entry),
            // A disabled server is planned as a removal, so the plan holds
            // no entry to read and nothing about it can be judged.
            None => Content::Unread {
                why: "this server is being removed from the harness's configuration, not written to it",
            },
        },
        ItemKind::Plugin => Content::Unread {
            why: UNREADABLE_PLUGIN,
        },
        // The registered command is the content, read off the registration
        // edit so the rules judge exactly what the harness will run; the
        // entry text is built by the same functions that write the file,
        // so the gate scans and binds what the audit will read back.
        _ => {
            let entries = hook_entries(edits);
            let (event, matcher, command) = match hook_edit(edits) {
                Some((event, matcher, command)) => (
                    event.clone(),
                    matcher.clone().filter(|m| !m.is_empty()),
                    command.clone(),
                ),
                None => (String::new(), None, location.clone()),
            };
            Content::Hook {
                event,
                matcher,
                command,
                entry: (!entries.is_empty()).then(|| entries.join("\n")),
                script: script.map(|(path, bytes)| {
                    (
                        path.display().to_string(),
                        String::from_utf8_lossy(bytes).into_owned(),
                    )
                }),
                script_unread: None,
            }
        }
    };
    (location, content)
}

fn hook_edit(
    edits: &[(std::path::PathBuf, ConfigEdit)],
) -> Option<(&String, &Option<String>, &String)> {
    edits.iter().find_map(|(_, edit)| match edit {
        ConfigEdit::UpsertHook {
            event,
            matcher,
            command,
            ..
        }
        | ConfigEdit::UpsertCopilotHook {
            event,
            matcher,
            command,
            ..
        } => Some((event, matcher, command)),
        _ => None,
    })
}

/// Every entry this plan registers, as the scanned text the audit's
/// read-back produces for the same file — one construction on both sides,
/// with the command stripped the same way, since it is its own document.
fn hook_entries(edits: &[(std::path::PathBuf, ConfigEdit)]) -> Vec<String> {
    edits
        .iter()
        .filter_map(|(_, edit)| match edit {
            ConfigEdit::UpsertHook {
                event,
                matcher,
                command,
                timeout,
            } => Some(crate::scan::hooks::scanned_entry(
                event,
                crate::configedit::spelled(matcher.as_deref()),
                &crate::configedit::handler_json(command, *timeout),
                command,
            )),
            ConfigEdit::UpsertCopilotHook {
                event,
                matcher,
                command,
                timeout,
            } => Some(crate::scan::hooks::scanned_entry(
                event,
                crate::configedit::spelled(matcher.as_deref()),
                &crate::configedit::copilot_entry_json(matcher.as_deref(), command, *timeout),
                command,
            )),
            _ => None,
        })
        .collect()
}

/// The server entry this plan would write, taken from the config edit that
/// writes it — command, arguments, environment, headers and url, exactly as
/// the harness will store them.
fn mcp_entry(edits: &[(std::path::PathBuf, ConfigEdit)]) -> Option<McpEntry> {
    edits
        .iter()
        .find_map(|(_, edit)| match edit {
            ConfigEdit::UpsertMcpServer { value, .. } => Some(value),
            _ => None,
        })
        .map(McpEntry::from_json)
}

#[cfg(test)]
mod tests;
