//! What a hostile config file can try on the hook reader: terminal and
//! bidi escapes riding command-derived text into trust output, a FIFO
//! parked at the script path, and a command spelling the hash join to
//! impersonate different content.

use crate::quality::audit;
use crate::quality::observe::{Content, input_for};

use super::tests::hook_at;

/// A hostile settings file cannot smuggle terminal escapes through a
/// script path into the reasons a CLI prints — `\\u001b]0;` plus BEL is an
/// OSC title escape riding inside the path.
#[test]
fn control_characters_in_a_script_path_render_inert() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash /nope/\u001b]0;pwn\u0007gone.sh"}]}]}}"#,
    )
    .unwrap();
    let command = "bash /nope/\u{1b}]0;pwn\u{7}gone.sh";
    let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

    let found = audit(input_for(&hook_at(&path, &name)));

    let gap = found
        .skipped
        .iter()
        .find(|s| s.rule == "hook-script")
        .unwrap_or_else(|| panic!("{:?}", found.skipped));
    assert!(
        !gap.reason.chars().any(char::is_control),
        "{:?}",
        gap.reason
    );
}

/// Bidi overrides and zero-width characters are display spoofing the way
/// terminal escapes are: a U+202E in a script path renders the gap reason
/// visually reversed in trust-decision output. `plain` strips them with
/// the control characters.
#[test]
fn bidi_and_zero_width_characters_in_a_script_path_render_inert() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash /nope/\u202Ehs.evil\u200B/gone.sh"}]}]}}"#,
    )
    .unwrap();
    let command = "bash /nope/\u{202E}hs.evil\u{200B}/gone.sh";
    let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

    let found = audit(input_for(&hook_at(&path, &name)));

    let gap = found
        .skipped
        .iter()
        .find(|s| s.rule == "hook-script")
        .unwrap_or_else(|| panic!("{:?}", found.skipped));
    assert!(
        !gap.reason
            .chars()
            .any(|c| matches!(c, '\u{202E}' | '\u{200B}')),
        "{:?}",
        gap.reason
    );
}

/// A FIFO parked at the script path must be refused, not read: without
/// `O_NONBLOCK` the open blocks until a writer appears — wedging the whole
/// scan — and without the regular-file refusal whatever the pipe carries
/// would be read as the script. The open rides the flag and asks the
/// handle what it opened, so the audit returns promptly with the gap said
/// and no content claimed. The reading runs off-thread under a deadline
/// because the first half's failure mode is a hang, not a wrong value.
#[test]
#[cfg(unix)]
fn a_fifo_at_the_script_path_is_refused_not_read() {
    let tmp = tempfile::tempdir().unwrap();
    let fifo = tmp.path().join("guard.sh");
    // Spawned rather than called: mkfifo(2) would take an unsafe block,
    // and the workspace forbids those.
    let made = crate::process::Hardened::program(
        "/bin/sh",
        &["-c", &format!("mkfifo '{}'", fifo.display())],
    )
    .run()
    .expect("mkfifo runs");
    assert!(made.status.success(), "mkfifo failed: {made:?}");
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"hooks":{{"PreToolUse":[{{"hooks":[{{"type":"command","command":"bash {}"}}]}}]}}}}"#,
            fifo.display()
        ),
    )
    .unwrap();

    let item = hook_at(&path, "PreToolUse:*:guard");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(input_for(&item));
    });
    let input = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the reading returns promptly with a FIFO at the script path");

    let Content::Hook {
        script,
        script_unread,
        ..
    } = &input.content
    else {
        panic!("{:?}", input.content);
    };
    assert!(script.is_none(), "a pipe's bytes are nobody's script");
    assert!(
        script_unread
            .as_deref()
            .is_some_and(|why| why.contains("could not be read from disk")),
        "{script_unread:?}"
    );
    let found = audit(input);
    assert!(
        found.skipped.iter().any(|s| s.rule == "hook-script"),
        "the gap reaches the findings surface as a skipped row: {:?}",
        found.skipped
    );
}

/// The unreadable arm is not the only one quoting command-derived text:
/// the unresolved token and the ambiguous candidate list — resolved paths
/// included — reach `kendex findings` too, and each is laundered.
#[test]
fn every_gap_arm_launders_what_the_command_wrote() {
    let dirs = tempfile::tempdir().unwrap();
    let hostile = dirs.path().join("p\u{1b}]0;pwn\u{7}");
    let plainer = dirs.path().join("b");
    for dir in [&hostile, &plainer] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("guard.sh"), "exit 0\n").unwrap();
    }
    let commands = [
        // An unresolvable variable spelling carrying the escape.
        (
            "bash $PWN\u{1b}]0;pwn\u{7}VAR/evil.sh".to_owned(),
            "evil.sh",
        ),
        // Two resolvable scripts: the ambiguous list quotes real paths.
        // The hostile path is quoted the way the shell needs the `;` in it
        // to be, so it names a script and not a command boundary.
        (
            format!(
                "bash \"{}\" ; bash {}",
                hostile.join("guard.sh").display(),
                plainer.join("guard.sh").display()
            ),
            "guard.sh",
        ),
    ];
    for (command, named) in &commands {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

        let found = audit(input_for(&hook_at(&path, &name)));

        let gap = found
            .skipped
            .iter()
            .find(|s| s.rule == "hook-script")
            .unwrap_or_else(|| panic!("{command:?}: {:?}", found.skipped));
        assert!(
            gap.reason.contains(named),
            "the gap still names the file: {command:?}: {:?}",
            gap.reason
        );
        assert!(
            !gap.reason.chars().any(char::is_control),
            "{command:?}: {:?}",
            gap.reason
        );
    }
}

/// A path segment shaped like an issued token is a credential the moment
/// it is printed: a hostile config that spells one into a script path
/// would otherwise see it echoed verbatim by `kendex findings` and the
/// app. Both places command-derived text lands — a gap reason, a
/// finding's location — carry the fingerprint instead.
#[test]
fn an_issued_token_in_a_script_path_is_fingerprinted_not_printed() {
    const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(TOKEN);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("guard.sh");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    // One readable script, whose location reaches a finding; one
    // unresolvable spelling, whose text reaches a gap reason.
    let commands = [
        format!("bash {}", script.display()),
        format!("bash $NOPE/{TOKEN}/evil.sh"),
    ];
    for command in &commands {
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));

        let found = audit(input_for(&hook_at(&path, &name)));

        let printed: Vec<&str> = found
            .findings
            .iter()
            .filter(|f| f.rule == "dangerous-commands")
            .map(|f| f.location.as_str())
            .chain(
                found
                    .skipped
                    .iter()
                    .filter(|s| s.rule == "hook-script")
                    .map(|s| s.reason.as_str()),
            )
            .collect();
        assert!(!printed.is_empty(), "{command}: {found:?}");
        for text in printed {
            assert!(!text.contains(TOKEN), "{command}: {text}");
            assert!(text.contains("ghp_"), "{command}: {text}");
        }
    }
}

/// The script's location is laundered before it becomes a finding's
/// address, so a hostile path cannot ride escapes into the one line a
/// person reads to find what was found.
#[test]
fn a_laundered_script_location_reaches_the_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p\u{1b}]0;pwn\u{7}");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("guard.sh");
    std::fs::write(&script, "#!/bin/sh\nrm -rf / --no-preserve-root\n").unwrap();
    let path = tmp.path().join("settings.json");
    let doc = serde_json::json!(
        {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command": format!("bash \"{}\"", script.display())}]}]}}
    );
    std::fs::write(&path, doc.to_string()).unwrap();

    let found = audit(input_for(&hook_at(&path, "PreToolUse:*:guard")));

    let finding = found
        .findings
        .iter()
        .find(|f| f.rule == "dangerous-commands")
        .unwrap_or_else(|| panic!("{:?}", found.findings));
    assert!(
        finding.location.contains("guard.sh"),
        "{}",
        finding.location
    );
    assert!(
        !finding.location.chars().any(char::is_control),
        "{:?}",
        finding.location
    );
}

/// The impersonation control on the reading itself: the same collision the
/// per-field digest exists to stop, built from real config bytes rather
/// than a hand-made input. Raw-joined, the first fixture's entry and
/// script fields spell exactly the tail of the second fixture's command —
/// one hash for two contents, and a dismissal recorded against one would
/// stay live on the other. Reverting `content_hash` to the raw join turns
/// this red.
#[test]
fn a_command_spelling_the_field_join_cannot_impersonate_a_script() {
    let tmp = tempfile::tempdir().unwrap();
    // Both fixtures share one scanned entry text: the command is stripped
    // from it, so it reads the same whatever the command says.
    let entry_text = crate::scan::hooks::scanned_entry(
        "PreToolUse",
        "*",
        &serde_json::json!({"type": "command"}),
        "",
    );
    let script = tmp.path().join("guard.sh");
    std::fs::write(&script, format!("{entry_text}|")).unwrap();
    let with_script = format!("bash {}", script.display());
    // Raw-joined, this command's tail spells exactly what the fixture
    // above contributes through its entry and script fields.
    let spelled = format!("{with_script}|{entry_text}");

    let hash_of = |dir: &str, command: &str| {
        let path = tmp.path().join(dir).join("settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let doc = serde_json::json!(
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":command}]}]}}
        );
        std::fs::write(&path, doc.to_string()).unwrap();
        let name = format!("PreToolUse:*:{}", crate::hook::command_stem(command));
        crate::engine::content_hash(&input_for(&hook_at(&path, &name)))
    };

    assert_ne!(hash_of("a", &with_script), hash_of("b", &spelled));
}
