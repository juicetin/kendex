//! How a command line is read into the words that name its scripts:
//! operators as boundaries, quotes kept whole, a substitution kept whole,
//! and a line the reader cannot finish refused whole.

use crate::model::ObservedItem;
use crate::quality::audit;
use crate::quality::observe::input_for;

use super::tests::hook_at;

/// A trailing shell operator is the next command, not part of the path:
/// `bash /abs/evil.sh;` runs `/abs/evil.sh`, and leaving the `;` on the
/// token would fail extension matching and let the script execute with no
/// candidate collected — unread and unmarked.
#[test]
fn a_trailing_shell_operator_does_not_hide_the_script() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("evil.sh");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let command = format!("bash {};", script.display());
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"hooks":{{"PreToolUse":[{{"hooks":[{{"type":"command","command":"{command}"}}]}}]}}}}"#,
        ),
    )
    .unwrap();
    let name = format!("PreToolUse:*:{}", crate::hook::command_stem(&command));

    let found = audit(input_for(&hook_at(&path, &name)));

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

/// An operator glued to a path mid-token is a word boundary, as the shell
/// reads it. Trimming only a *trailing* operator left `/b/evil.sh;bash`
/// as one extensionless token — silently dropped while `/a/ok.sh` beside
/// it was read and bound — and `x.sh>/dev/null` named no script at all.
/// Every form here is a must-fail control on the old trim: the glued
/// script is read and its finding lands in it, or it is named in the gap.
#[test]
fn an_operator_glued_to_a_script_path_still_splits_it_off() {
    let tmp = tempfile::tempdir().unwrap();
    let evil = tmp.path().join("evil.sh");
    std::fs::write(&evil, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let ok = tmp.path().join("ok.sh");
    std::fs::write(&ok, "exit 0\n").unwrap();
    // (what follows the path, whether evil.sh is then the only script)
    let glued = [
        (format!(";bash {}", ok.display()), false),
        ("&&true".to_owned(), true),
        ("|tee".to_owned(), true),
        (">/dev/null".to_owned(), true),
    ];
    for (tail, alone) in &glued {
        let command = format!("bash {}{tail}", evil.display());
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(&command));

        let found = audit(input_for(&hook_at(&path, &name)));

        if *alone {
            assert!(
                found.findings.iter().any(|f| f.rule == "dangerous-commands"
                    && f.location == format!("{}:2", evil.display())),
                "{command}: {:?}",
                found.findings
            );
            assert!(found.skipped.is_empty(), "{command}: {:?}", found.skipped);
        } else {
            // Two scripts named: neither is claimed, and the gap names the
            // one the old trim hid.
            assert!(
                found.skipped.iter().any(|s| s.rule == "hook-script"
                    && s.reason.contains("more than one script")
                    && s.reason.contains("evil.sh")),
                "{command}: {:?}",
                found.skipped
            );
        }
    }
}

/// Every character the shell reads as an operator is a word boundary, not
/// just `;`. The list is spelled here on its own, so a lexer set shrunk to
/// `;` fails the `&` case instead of shrinking this loop with it; the
/// equality pins the two lists to each other so neither drifts.
#[test]
fn every_shell_operator_is_a_word_boundary() {
    const OPERATORS: &[char] = &[';', '&', '|', '(', ')', '<', '>'];
    assert_eq!(OPERATORS, super::scripts::SHELL_OPERATORS);
    let tmp = tempfile::tempdir().unwrap();
    let evil = tmp.path().join("evil.sh");
    std::fs::write(&evil, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    for op in OPERATORS {
        let command = format!("bash {}{op}true", evil.display());
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(&command));

        let found = audit(input_for(&hook_at(&path, &name)));

        assert!(
            found
                .findings
                .iter()
                .any(|f| f.rule == "dangerous-commands"
                    && f.location == format!("{}:2", evil.display())),
            "{command}: {:?}",
            found.findings
        );
        assert!(found.skipped.is_empty(), "{command}: {:?}", found.skipped);
    }
}

/// An unquoted command substitution is one word, not three. Split at its
/// parentheses, `$(pwd)/decoy.sh` left `/decoy.sh` standing alone as an
/// absolute path: kendex read a file at the root of this machine and bound
/// a decision to it while the harness ran the copy under `pwd`. Here the
/// decoy is a real dangerous script at the path the split would have
/// produced; the reading must not open it, and the gap must name the
/// spelling the shell actually evaluates. A must-fail control on the
/// split: the old lexer lands a dangerous-commands finding in the decoy.
#[test]
fn an_unquoted_command_substitution_is_one_unresolved_word() {
    let tmp = tempfile::tempdir().unwrap();
    let decoy = tmp.path().join("decoy.sh");
    std::fs::write(&decoy, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    // (the command, what the gap has to name)
    let cases = [
        (format!("bash $(pwd){}", decoy.display()), "$(pwd)"),
        (
            format!("bash $(git rev-parse --show-toplevel){}", decoy.display()),
            "$(git rev-parse --show-toplevel)",
        ),
        (
            "bash $(pwd)/etc/profile.d/bash_completion.sh".to_owned(),
            "$(pwd)/etc/profile.d/bash_completion.sh",
        ),
        // Process substitution: one word too, so the split cannot leave
        // the decoy's path standing alone. It names no script by
        // extension, so this case asserts only that nothing was read.
        (format!("bash <(cat {})", decoy.display()), ""),
    ];
    for (command, named) in &cases {
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

        let found = audit(input_for(&hook_at(&path, &name)));

        assert!(found.findings.is_empty(), "{command}: {:?}", found.findings);
        if named.is_empty() {
            continue;
        }
        assert!(
            found
                .skipped
                .iter()
                .any(|s| s.rule == "hook-script" && s.reason.contains(named)),
            "{command}: {:?}",
            found.skipped
        );
    }
}

/// The double-quoted git-toplevel spelling kendex itself writes for a
/// project hook still resolves to the scope root and is read: keeping a
/// substitution whole must not cost the one substitution kendex resolves.
#[test]
fn the_quoted_git_toplevel_spelling_still_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    std::fs::create_dir_all(root.join(".codex/hooks")).unwrap();
    let script = root.join(".codex/hooks/guard.sh");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let path = root.join(".claude/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$(git rev-parse --show-toplevel)/.codex/hooks/guard.sh\""}]}]}}"#,
    )
    .unwrap();
    let item = ObservedItem {
        scope: crate::model::Scope::Project { root: root.clone() },
        ..hook_at(&path, "PreToolUse:*:guard")
    };

    let found = audit(input_for(&item));

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

/// A quoted parenthesis inside a substitution is text, not nesting. Counted
/// as nesting, `$(echo "(")` never closed and swallowed the rest of the
/// line — `bash /abs/benign.sh $(echo "(") ; bash /abs/evil.sh ; true` read
/// benign.sh alone, clean and bound, while the shell ran evil.sh; and
/// `$(echo ")")` closed early, leaving the tail one word with no script
/// extension, so evil.sh ran with nothing read and nothing said. Each form
/// here, double- and single-quoted, either lands evil.sh's finding or says
/// a gap; what it never does is read nothing and say nothing.
#[test]
fn a_quoted_paren_inside_a_substitution_does_not_desync() {
    let tmp = tempfile::tempdir().unwrap();
    let benign = tmp.path().join("benign.sh");
    std::fs::write(&benign, "exit 0\n").unwrap();
    let evil = tmp.path().join("evil.sh");
    std::fs::write(&evil, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let (b, e) = (benign.display(), evil.display());
    // (the command, whether evil.sh is the only script the line names)
    let cases = [
        (format!(r#"bash {b} $(echo "(") ; bash {e} ; true"#), false),
        (format!(r#"bash $(echo ")"); bash {e}; true"#), true),
        (format!("bash {b} $(echo '(') ; bash {e} ; true"), false),
        (format!("bash $(echo ')'); bash {e}; true"), true),
    ];
    for (command, alone) in &cases {
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

        let found = audit(input_for(&hook_at(&path, &name)));

        let evil_read = found
            .findings
            .iter()
            .any(|f| f.rule == "dangerous-commands" && f.location == format!("{e}:2"));
        let gap = found.skipped.iter().any(|s| s.rule == "hook-script");
        assert!(
            evil_read || gap,
            "read nothing and said nothing: {command}: {found:?}"
        );
        if *alone {
            assert!(
                evil_read && found.skipped.is_empty(),
                "{command}: {found:?}"
            );
        } else {
            // Two scripts named: neither is claimed and both are said.
            assert!(!evil_read && gap, "{command}: {found:?}");
            assert!(
                found.skipped.iter().any(|s| s.reason.contains("evil.sh")),
                "{command}: {:?}",
                found.skipped
            );
        }
    }
}

/// A line that runs out with a quote or a substitution still open has no
/// words this audit can vouch for, whatever caused it: the whole line is
/// refused as one unresolvable spelling and the gap is said. That rule is
/// what catches the next desync — an escape, a comment, a spelling the
/// reader does not know — not the list of shapes the tests above name. An
/// unambiguous script earlier on the same line is not read either: the
/// boundaries around it are the ones in doubt.
#[test]
fn a_line_the_reader_cannot_finish_is_refused_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("x.sh");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let x = script.display();
    let cases = [
        format!("bash $(pwd {x}"),
        format!(r#"bash "{x}"#),
        format!(r#"bash {x} $(echo ")""#),
    ];
    for command in &cases {
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

        let found = audit(input_for(&hook_at(&path, &name)));

        assert!(found.findings.is_empty(), "{command}: {:?}", found.findings);
        assert!(
            found
                .skipped
                .iter()
                .any(|s| s.rule == "hook-script" && s.reason.contains("x.sh")),
            "{command}: {:?}",
            found.skipped
        );
    }
}

/// Quotes inside a double-quoted substitution nest the way the shell nests
/// them: `"$(echo "a")/x.sh"` is one word, still a spelling nobody can
/// resolve, and still a said gap.
#[test]
fn a_quoted_substitution_with_inner_quotes_keeps_its_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let command = r#"bash "$(echo "a")/x.sh""#;
    let doc = serde_json::json!(
        {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
    );
    std::fs::write(&path, doc.to_string()).unwrap();
    let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

    let found = audit(input_for(&hook_at(&path, &name)));

    assert!(found.findings.is_empty(), "{:?}", found.findings);
    assert!(
        found
            .skipped
            .iter()
            .any(|s| s.rule == "hook-script" && s.reason.contains(r#"$(echo "a")/x.sh"#)),
        "{:?}",
        found.skipped
    );
}
