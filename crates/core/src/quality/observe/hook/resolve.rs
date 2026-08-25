//! How a script's spelling resolves to a file on this machine: kendex's
//! own project spellings to the scope root, a lookalike or a single-quoted
//! spelling to nothing, and any segment the shell would still evaluate to
//! the said gap rather than a literal read.

use crate::model::ObservedItem;
use crate::quality::audit;
use crate::quality::observe::{Content, input_for};

use super::tests::hook_at;

/// kendex's own project installs spell the script through
/// `$CLAUDE_PROJECT_DIR` or `$(git rev-parse --show-toplevel)` — both of
/// which evaluate to the scope root the observation already knows, so the
/// product's own hooks are scored to their scripts, quoting and all.
#[test]
fn a_project_variable_spelling_resolves_to_the_scope_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    std::fs::create_dir_all(root.join(".claude/hooks")).unwrap();
    std::fs::write(
        root.join(".claude/hooks/guard.sh"),
        "#!/bin/sh\nrm -rf / --no-preserve-root\n",
    )
    .unwrap();
    let path = root.join(".claude/settings.json");
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
    )
    .unwrap();
    let item = ObservedItem {
        scope: crate::model::Scope::Project { root: root.clone() },
        ..hook_at(&path, "PreToolUse:*:guard")
    };

    let found = audit(input_for(&item));

    let script = root.join(".claude/hooks/guard.sh");
    assert!(
        found
            .findings
            .iter()
            .any(|f| f.rule == "dangerous-commands"
                && f.location == format!("{}:2", script.display())),
        "{:?}",
        found.findings
    );
    assert!(found.skipped.is_empty(), "{:?}", found.skipped);
}

/// A script spelled some way kendex cannot resolve — a relative path, a
/// variable it did not write — is a said gap, not a silent one, and the
/// command line still scores.
#[test]
fn an_unresolvable_script_spelling_is_a_said_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash hooks/guard.sh"}]}]}}"#,
    )
    .unwrap();

    let found = audit(input_for(&hook_at(&path, "PreToolUse:*:guard")));

    assert!(
        found
            .skipped
            .iter()
            .any(|s| s.rule == "hook-script" && s.reason.contains("hooks/guard.sh")),
        "{:?}",
        found.skipped
    );
}

/// `$CLAUDE_PROJECT_DIRS` and `$CLAUDE_PROJECT_DIR_backup` are different
/// variables: the shell expands those, not kendex's spelling, so resolving
/// them against the scope root would read — and bind — bytes the harness
/// never runs. They are a said gap instead.
#[test]
fn a_lookalike_variable_is_not_the_project_root() {
    for command in [
        "bash $CLAUDE_PROJECT_DIRS/run.sh",
        "bash $CLAUDE_PROJECT_DIR_backup/run.sh",
        "bash \"$(git rev-parse --show-toplevel)extra/run.sh\"",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("app");
        // A decoy at the path naive resolution would produce: it must not
        // be what gets read.
        std::fs::create_dir_all(root.join("S")).unwrap();
        std::fs::write(root.join("S/run.sh"), "exit 0\n").unwrap();
        let path = root.join("settings.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{"hooks":{{"PreToolUse":[{{"hooks":[{{"type":"command","command":"{}"}}]}}]}}}}"#,
                command.replace('"', "\\\"")
            ),
        )
        .unwrap();
        let item = ObservedItem {
            scope: crate::model::Scope::Project { root: root.clone() },
            ..hook_at(&path, "PreToolUse:*:run")
        };

        let input = input_for(&item);

        let Content::Hook {
            script,
            script_unread,
            ..
        } = &input.content
        else {
            panic!("{command}: {:?}", input.content);
        };
        assert!(script.is_none(), "{command}");
        assert!(script_unread.is_some(), "{command}");
    }
}

/// `GUARD.SH` is the same script as `guard.sh`; a spelling trick must not
/// hide it from resolution.
#[test]
fn an_uppercase_extension_still_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("GUARD.SH");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"hooks":{{"PreToolUse":[{{"hooks":[{{"type":"command","command":"bash {}"}}]}}]}}}}"#,
            script.display()
        ),
    )
    .unwrap();

    let found = audit(input_for(&hook_at(&path, "PreToolUse:*:GUARD")));

    assert!(
        found
            .findings
            .iter()
            .any(|f| f.rule == "dangerous-commands"
                && f.location == format!("{}:2", script.display())),
        "{:?}",
        found.findings
    );
}

/// Inside single quotes the shell expands nothing: a single-quoted
/// `$CLAUDE_PROJECT_DIR` spelling runs a literal-`$` path, so resolving it
/// to the project root would read — and bind a decision to — bytes the
/// harness never runs. It is a said gap instead, and the file naive
/// resolution would have produced stays unread.
#[test]
fn a_single_quoted_variable_spelling_is_never_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    // A decoy at the path naive resolution would produce: it must not be
    // what gets read.
    std::fs::create_dir_all(root.join(".claude/hooks")).unwrap();
    std::fs::write(
        root.join(".claude/hooks/guard.sh"),
        "#!/bin/sh\nrm -rf / --no-preserve-root\n",
    )
    .unwrap();
    let path = root.join(".claude/settings.json");
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash '$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh'"}]}]}}"#,
    )
    .unwrap();
    let item = ObservedItem {
        scope: crate::model::Scope::Project { root: root.clone() },
        ..hook_at(&path, "PreToolUse:*:guard")
    };

    let input = input_for(&item);

    let Content::Hook {
        script,
        script_unread,
        ..
    } = &input.content
    else {
        panic!("{:?}", input.content);
    };
    assert!(script.is_none(), "{script:?}");
    assert!(
        script_unread
            .as_deref()
            .is_some_and(|why| why.contains("could not be resolved")),
        "{script_unread:?}"
    );
}

/// A token the shell still evaluates at run time — a variable, a
/// substitution, a glob, a tilde, an escape — names a path this audit
/// cannot compute. Reading the spelling literally would open a file that
/// never runs and bind a decision to it: each form here plants a decoy at
/// exactly that literal path, dangerous enough that reading it lands a
/// finding, and that finding is the must-fail control — it must not
/// appear. The gap is said and names the spelling; a plain absolute path
/// beside them still resolves.
#[test]
fn a_dynamic_path_segment_is_a_said_gap_never_a_literal_read() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let segments = ["$USER", "`id`", "*", "x?", "[ab]", "~", "a\\b"];
    for segment in segments {
        let decoy = tmp.path().join(segment).join("guard.sh");
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(&decoy, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
        let command = format!("bash {}", decoy.display());
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(&command));

        let found = audit(input_for(&hook_at(&path, &name)));

        assert!(
            !found
                .findings
                .iter()
                .any(|f| f.location.starts_with(&decoy.display().to_string())),
            "{command}: the decoy was read: {:?}",
            found.findings
        );
        let gap = found
            .skipped
            .iter()
            .find(|s| s.rule == "hook-script")
            .unwrap_or_else(|| panic!("{command}: {:?}", found.skipped));
        assert!(
            gap.reason.contains("could not be resolved")
                && gap.reason.contains(&format!("{segment}/guard.sh")),
            "{command}: {:?}",
            gap.reason
        );
    }
    // `~/x.sh` on its own is not an absolute path either way; it takes the
    // same gap.
    let command = "bash ~/guard.sh";
    let doc = serde_json::json!(
        {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
    );
    std::fs::write(&path, doc.to_string()).unwrap();
    let found = audit(input_for(&hook_at(&path, "PreToolUse:*:guard")));
    assert!(
        found
            .skipped
            .iter()
            .any(|s| s.rule == "hook-script" && s.reason.contains("~/guard.sh")),
        "{:?}",
        found.skipped
    );

    // The control the rule must not swallow: a plain absolute path.
    let plain = tmp.path().join("plain").join("guard.sh");
    std::fs::create_dir_all(plain.parent().unwrap()).unwrap();
    std::fs::write(&plain, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let command = format!("bash {}", plain.display());
    let doc = serde_json::json!(
        {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
    );
    std::fs::write(&path, doc.to_string()).unwrap();
    let found = audit(input_for(&hook_at(&path, "PreToolUse:*:guard")));
    assert!(
        found
            .findings
            .iter()
            .any(|f| f.rule == "dangerous-commands"
                && f.location == format!("{}:2", plain.display())),
        "{:?}",
        found.findings
    );
    assert!(found.skipped.is_empty(), "{:?}", found.skipped);
}

/// A dynamic segment after kendex's own project spelling is refused the
/// same way: `$CLAUDE_PROJECT_DIR/hooks/$NAME.sh` is not a file this
/// audit can name, and the decoy a literal join would land on stays
/// unread.
#[test]
fn a_dynamic_segment_after_the_project_spelling_is_not_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    let decoy = root.join("hooks/$NAME.sh");
    std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
    std::fs::write(&decoy, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let path = root.join("settings.json");
    let doc = serde_json::json!(
        {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/hooks/$NAME.sh\""}]}]}}
    );
    std::fs::write(&path, doc.to_string()).unwrap();
    let item = ObservedItem {
        scope: crate::model::Scope::Project { root: root.clone() },
        ..hook_at(&path, "PreToolUse:*:$NAME")
    };

    let found = audit(input_for(&item));

    assert!(
        !found
            .findings
            .iter()
            .any(|f| f.location.starts_with(&decoy.display().to_string())),
        "the decoy was read: {:?}",
        found.findings
    );
    assert!(
        found
            .skipped
            .iter()
            .any(|s| s.rule == "hook-script" && s.reason.contains("could not be resolved")),
        "{:?}",
        found.skipped
    );
}
