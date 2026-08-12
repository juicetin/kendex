use super::*;
use crate::config::{InstallMethod, ItemKind, LockEntry};
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn tmpdir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "vstack-propagate-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn demo_skill_source(body: &str) -> String {
    format!("---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n{body}")
}

fn write_skill_source(root: &Path, body: &str) {
    let skill_dir = root.join("skills").join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), demo_skill_source(body)).unwrap();
}

/// The bytes `installer::install_skill` leaves at an installed `SKILL.md` for
/// the skill `write_skill_source` writes. A fixture that writes the raw source
/// there has not written an install: the pre-stage check holds an installed
/// skill to its rendered source and reads the difference as a local edit.
fn write_installed_skill_md(path: &Path, body: &str) {
    write_file(
        path,
        &crate::skill::render_installed_skill_md(&demo_skill_source(body), None),
    );
}

fn write_agent_skill_source(root: &Path, role_skills: bool) {
    std::fs::create_dir_all(root.join("agents")).unwrap();
    std::fs::create_dir_all(root.join("skills/demo")).unwrap();
    std::fs::write(
        root.join("agents/rust.md"),
        "---\nname: rust\ndescription: Rust\nmodel: sonnet\nrole: engineer\n---\n\n# Rust\n",
    )
    .unwrap();
    std::fs::write(
        root.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
    )
    .unwrap();
    let mapping = if role_skills {
        "[role-skills]\nengineer = [\"demo\"]\n"
    } else {
        "[role-skills]\n"
    };
    std::fs::write(root.join("vstack.toml"), mapping).unwrap();
}

/// The exact bytes the installer copies to `.claude/hooks/<name>.sh` and
/// `.codex/hooks/<name>.sh` — the whole source file, frontmatter included.
fn hook_script_contents(name: &str) -> String {
    format!(
        "# ---\n# name: {name}\n# event: PreToolUse\n# matcher: Bash\n# description: Guard shell commands\n# safety: Keep shell commands safe\n# ---\n#!/bin/sh\nexit 0\n"
    )
}

/// Write an installed native hook script the way the installer does: exact
/// source bytes, mode 0755.
fn write_installed_hook_script(path: &Path, name: &str) {
    write_file(path, &hook_script_contents(name));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn write_hook_source(root: &Path, name: &str) {
    write_file(
        &root.join("hooks").join(format!("{name}.sh")),
        &hook_script_contents(name),
    );
}

/// A hook whose event Codex has no native equivalent for, so it installs as
/// advisory prose in every agent TOML instead of a script.
fn write_prose_fallback_hook_source(root: &Path, name: &str) {
    write_file(
        &root.join("hooks").join(format!("{name}.sh")),
        &format!(
            "# ---\n# name: {name}\n# event: TaskCompleted\n# description: Guard turn completion\n# safety: Keep turns safe\n# ---\n#!/bin/sh\nexit 0\n"
        ),
    );
}

fn demo_entry(source: &Path) -> LockEntry {
    LockEntry {
        name: "demo".to_string(),
        kind: ItemKind::Skill,
        source: source.display().to_string(),
        source_repo: None,
        harnesses: vec!["claude-code".to_string()],
        method: InstallMethod::Symlink,
        installed_at: "2026-08-11T00:00:00Z".to_string(),
        source_hash: String::new(),
    }
}

/// The same `demo` skill installed by copy: its harness path is a real
/// directory rather than a link into `.agents/skills`.
fn demo_copy_entry(source: &Path) -> LockEntry {
    LockEntry {
        method: InstallMethod::Copy,
        ..demo_entry(source)
    }
}

fn agent_entry(name: &str, source: &Path) -> LockEntry {
    LockEntry {
        name: name.to_string(),
        kind: ItemKind::Agent,
        source: source.display().to_string(),
        source_repo: None,
        harnesses: vec!["claude-code".to_string()],
        method: InstallMethod::Copy,
        installed_at: "2026-08-11T00:00:00Z".to_string(),
        source_hash: String::new(),
    }
}

fn hook_entry(name: &str, source: &Path, harnesses: &[&str]) -> LockEntry {
    LockEntry {
        name: name.to_string(),
        kind: ItemKind::Hook,
        source: source.display().to_string(),
        source_repo: None,
        harnesses: harnesses
            .iter()
            .map(|harness| (*harness).to_string())
            .collect(),
        method: InstallMethod::Copy,
        installed_at: "2026-08-11T00:00:00Z".to_string(),
        source_hash: String::new(),
    }
}

fn pi_entry(name: &str, source: &Path) -> LockEntry {
    LockEntry {
        name: name.to_string(),
        kind: ItemKind::PiExtension,
        source: source.display().to_string(),
        source_repo: None,
        harnesses: vec!["pi".to_string()],
        method: InstallMethod::Copy,
        installed_at: "2026-08-11T00:00:00Z".to_string(),
        source_hash: String::new(),
    }
}

fn git(project: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(project)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        project.display()
    );
}

fn init_git_project(project: &Path) {
    git(project, &["init"]);
    git(project, &["config", "user.email", "test@example.com"]);
    git(project, &["config", "user.name", "VStack Test"]);
    git(project, &["config", "commit.gpgsign", "false"]);
    git(project, &["config", "core.hooksPath", "/dev/null"]);
}

fn git_output(project: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap()
}

fn write_file(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn lock_entry(name: &str, kind: ItemKind, harnesses: &[&str]) -> LockEntry {
    LockEntry {
        name: name.to_string(),
        kind,
        source: "/unused/source".to_string(),
        source_repo: None,
        harnesses: harnesses
            .iter()
            .map(|harness| (*harness).to_string())
            .collect(),
        method: InstallMethod::Copy,
        installed_at: "2026-08-11T00:00:00Z".to_string(),
        source_hash: "stored-hash".to_string(),
    }
}

#[test]
fn detect_drift_distinguishes_clean_and_changed_source() {
    let project = tmpdir("project");
    let source = tmpdir("source");
    std::fs::create_dir_all(&project).unwrap();
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();

        let clean = detect_drift_for_scope(false).unwrap();
        assert!(
            clean.rows.is_empty(),
            "unchanged source is the negative control"
        );

        write_skill_source(&source, "v2\n");
        let changed = detect_drift_for_scope(false).unwrap();
        assert_eq!(changed.rows.len(), 1);
        assert_eq!(changed.rows[0].name, "demo");
        assert_eq!(changed.rows[0].status, DriftStatus::Changed);
    });
}

#[test]
fn detects_and_refreshes_mapping_only_role_skill_drift() {
    let project = tmpdir("role-skill-drift-project");
    let source = tmpdir("role-skill-drift-source");
    std::fs::create_dir_all(&project).unwrap();
    write_agent_skill_source(&source, false);

    crate::test_util::with_project_root(&project, || {
        write_file(&project.join("vstack.toml"), "[agent-skills]\nrust = []\n");
        let mut rust = agent_entry("rust", &source);
        rust.source_hash = config::compute_source_hash(&rust);
        let mut demo = demo_entry(&source);
        demo.source_hash = config::compute_source_hash(&demo);
        let mut lock = LockFile::default();
        lock.add(rust);
        lock.add(demo);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "# Demo\n");
        write_file(&project.join(".claude/agents/rust.md"), "# Rust\n");

        let lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        let rust_hash = lock.entries["rust"].source_hash.clone();
        let demo_hash = lock.entries["demo"].source_hash.clone();
        assert!(!rust_hash.is_empty());
        assert!(!demo_hash.is_empty());

        write_agent_skill_source(&source, true);
        assert_ne!(
            config::compute_source_hash(&lock.entries["rust"]),
            rust_hash,
            "role-skills changes must affect the agent drift hash"
        );
        assert_eq!(
            config::compute_source_hash(&lock.entries["demo"]),
            demo_hash,
            "negative control: skill bytes are unchanged"
        );

        let err = run(ScopeFilter::Project, true, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("propagation needed"), "{err}");

        run(ScopeFilter::Project, false, false, false, true).unwrap();

        let project_config = std::fs::read_to_string(project.join("vstack.toml")).unwrap();
        assert!(project_config.contains("rust = ["), "{project_config}");
        assert!(project_config.contains("\"demo\""), "{project_config}");
        let agent = std::fs::read_to_string(project.join(".claude/agents/rust.md")).unwrap();
        assert!(agent.contains("demo"), "{agent}");
    });
}

#[test]
fn detect_drift_reports_unavailable_sources() {
    let project = tmpdir("project-missing");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&project.join("missing-source"));
        entry.source_hash = "stored-hash".to_string();
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();

        let drift = detect_drift_for_scope(false).unwrap();
        assert_eq!(drift.rows.len(), 1);
        assert_eq!(drift.rows[0].status, DriftStatus::SourceUnavailable);
        assert!(drift.has_unavailable_sources());
    });
}

#[test]
fn run_rejects_conflicting_stage_and_check_flags() {
    let err = run(ScopeFilter::Project, true, false, true, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("--stage cannot be combined with --check"));

    let err = run(ScopeFilter::Global, false, false, true, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("--stage is only supported with --scope project"));
}

#[test]
fn run_errors_for_explicit_empty_scope_but_allows_default_empty_scope() {
    let project = tmpdir("empty-scope");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let default_empty = run(ScopeFilter::Project, false, false, false, false);
        assert!(
            default_empty.is_ok(),
            "default empty project scope stays a no-op"
        );

        let explicit_empty = run(ScopeFilter::Project, false, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(explicit_empty.contains("no installed items found"));
    });
}

#[test]
fn check_reports_drift_without_refreshing_lock() {
    let project = tmpdir("check-project");
    let source = tmpdir("check-source");
    std::fs::create_dir_all(&project).unwrap();
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let stored_hash = entry.source_hash.clone();
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();

        write_skill_source(&source, "v2\n");
        let err = run(ScopeFilter::Project, true, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("propagation needed"));

        let lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        assert_eq!(
            lock.entries["demo"].source_hash, stored_hash,
            "--check must not normalize or refresh the lock"
        );
    });
}

#[test]
fn check_reports_legacy_hash_without_normalizing_lock() {
    let project = tmpdir("legacy-project");
    let source = tmpdir("legacy-source");
    std::fs::create_dir_all(&project).unwrap();
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(demo_entry(&source));
        lock.save(&config::lock_file_path(false)).unwrap();

        let err = run(ScopeFilter::Project, true, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("propagation needed"));

        let lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        assert!(
            lock.entries["demo"].source_hash.is_empty(),
            "--check must not normalize legacy lock hashes"
        );
    });
}

#[test]
fn run_fails_closed_when_a_locked_source_is_unavailable() {
    let project = tmpdir("unavailable-project");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("demo", ItemKind::Skill, &["claude-code"]));
        lock.save(&config::lock_file_path(false)).unwrap();

        let err = run(ScopeFilter::Project, false, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("locked sources are unavailable"));
    });
}

#[test]
fn stage_paths_are_scoped_to_lock_outputs_and_opencode_config() {
    let project = tmpdir("stage-project");
    let source = tmpdir("stage-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("vstack.toml"),
        "[catalog]\npi_extensions = [\"pi-extensions/@scope/pkg\"]\n",
    );
    write_file(
        &source.join("pi-extensions/@scope/pkg/package.json"),
        r#"{"name":"@scope/pkg","pi":{"extensions":[],"appendSystem":"instructions.md"},"bin":{"pi-tool":"bin/tool.js"}}"#,
    );
    write_file(
        &source.join("pi-extensions/@scope/pkg/instructions.md"),
        "pi rules\n",
    );
    write_file(
        &source.join("pi-extensions/@scope/pkg/bin/tool.js"),
        "tool\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("worker", ItemKind::Agent, &["opencode"]));
        lock.add(lock_entry("protect", ItemKind::Hook, &["opencode"]));
        lock.add(pi_entry("@scope/pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();

        write_file(
            &project.join(".opencode/agents/worker.md"),
            "managed agent\n",
        );
        write_file(
            &project.join(".agents/skill-failure-reporting.md"),
            "managed reference\n",
        );
        write_file(
            &project.join(".opencode/instructions/vstack-hook-protect.md"),
            "managed hook\n",
        );
        write_file(&project.join("opencode.json"), "{}\n");
        write_file(
            &project.join(".pi/packages/@scope/pkg/package.json"),
            r#"{"name":"@scope/pkg","pi":{"extensions":[],"appendSystem":"instructions.md"},"bin":{"pi-tool":"bin/tool.js"}}"#,
        );
        write_file(
            &project.join(".pi/packages/@scope/pkg/instructions.md"),
            "pi rules\n",
        );
        write_file(
            &project.join(".pi/packages/@scope/pkg/bin/tool.js"),
            "tool\n",
        );
        write_file(&project.join(".pi/bin/pi-tool"), "tool link\n");
        write_file(&project.join(".pi/settings.json"), "{}\n");
        write_file(&project.join(".pi/.vstack-source.json"), "{}\n");
        write_file(&project.join(".pi/APPEND_SYSTEM.md"), "append\n");
        write_file(&project.join(".opencode/secret.txt"), "repo secret\n");
        write_file(
            &project.join(".opencode/agents/unrelated.md"),
            "unrelated agent\n",
        );

        let paths = project_stage_paths(&lock, false).unwrap();
        assert!(paths.contains(&PathBuf::from(".vstack-lock.json")));
        assert!(paths.contains(&PathBuf::from(".opencode/agents/worker.md")));
        assert!(paths.contains(&PathBuf::from(".agents/skill-failure-reporting.md")));
        assert!(paths.contains(&PathBuf::from("opencode.json")));
        assert!(paths.contains(&PathBuf::from(
            ".opencode/instructions/vstack-hook-protect.md"
        )));
        assert!(paths.contains(&PathBuf::from(".pi/packages/@scope/pkg/package.json")));
        assert!(paths.contains(&PathBuf::from(".pi/packages/@scope/pkg/instructions.md")));
        assert!(paths.contains(&PathBuf::from(".pi/packages/@scope/pkg/bin/tool.js")));
        assert!(paths.contains(&PathBuf::from(".pi/bin/pi-tool")));
        assert!(paths.contains(&PathBuf::from(".pi/settings.json")));
        assert!(paths.contains(&PathBuf::from(".pi/.vstack-source.json")));
        assert!(paths.contains(&PathBuf::from(".pi/APPEND_SYSTEM.md")));
        assert!(!paths.contains(&PathBuf::from(
            ".pi/packages/@scope/pkg/node_modules/dep/index.js"
        )));
        assert!(!paths.contains(&PathBuf::from(".opencode/secret.txt")));
        assert!(!paths.contains(&PathBuf::from(".opencode/agents/unrelated.md")));

        write_file(
            &project.join(".pi/packages/@scope/pkg/node_modules/dep/index.js"),
            "generated dependency\n",
        );
        stage_paths(&paths).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"));
        assert!(staged.contains(".opencode/agents/worker.md\n"));
        assert!(staged.contains(".agents/skill-failure-reporting.md\n"));
        assert!(staged.contains("opencode.json\n"));
        assert!(staged.contains(".opencode/instructions/vstack-hook-protect.md\n"));
        assert!(staged.contains(".pi/packages/@scope/pkg/package.json\n"));
        assert!(staged.contains(".pi/packages/@scope/pkg/instructions.md\n"));
        assert!(staged.contains(".pi/packages/@scope/pkg/bin/tool.js\n"));
        assert!(!staged.contains(".pi/packages/@scope/pkg/node_modules/dep/index.js"));
        assert!(staged.contains(".pi/bin/pi-tool\n"));
        assert!(staged.contains(".pi/APPEND_SYSTEM.md\n"));
        assert!(!staged.contains(".opencode/secret.txt"));
        assert!(!staged.contains(".opencode/agents/unrelated.md"));
    });
}

#[test]
fn staging_does_not_stage_shared_configs_without_lock_ownership() {
    let project = tmpdir("stage-unowned-shared-config");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("demo", ItemKind::Skill, &["claude-code"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".agents/skill-failure-reporting.md"),
            "consumer reference\n",
        );
        write_file(&project.join(".claude/settings.json"), "{}\n");
        write_file(
            &project.join(".codex/config.toml"),
            "approval_policy = \"never\"\n",
        );
        write_file(&project.join("opencode.json"), "{}\n");
        write_file(&project.join(".pi/settings.json"), "{}\n");
        write_file(&project.join(".pi/.vstack-source.json"), "{}\n");
        write_file(&project.join(".pi/APPEND_SYSTEM.md"), "consumer prompt\n");

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
        assert!(
            !staged.contains(".agents/skill-failure-reporting.md"),
            "{staged}"
        );
        assert!(!staged.contains(".claude/settings.json"), "{staged}");
        assert!(!staged.contains(".codex/config.toml"), "{staged}");
        assert!(!staged.contains("opencode.json"), "{staged}");
        assert!(!staged.contains(".pi/settings.json"), "{staged}");
        assert!(!staged.contains(".pi/.vstack-source.json"), "{staged}");
        assert!(!staged.contains(".pi/APPEND_SYSTEM.md"), "{staged}");
    });
}

#[test]
fn staging_does_not_stage_cursor_safety_rules_without_hook_ownership() {
    let project = tmpdir("stage-unowned-cursor-rule");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join(".cursor/rules/safety-consumer.mdc"),
            "consumer rule\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
        assert!(
            !staged.contains(".cursor/rules/safety-consumer.mdc"),
            "{staged}"
        );
    });
}

#[test]
fn staging_records_owned_cursor_safety_rules() {
    let project = tmpdir("stage-owned-cursor-rule");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("guard", ItemKind::Hook, &["cursor"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".cursor/rules/safety-guard.mdc"),
            "managed cursor hook\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains(".cursor/rules/safety-guard.mdc\n"),
            "{staged}"
        );
    });
}

#[test]
fn retry_staging_records_deleted_owned_cursor_safety_rules_from_committed_lock() {
    let project = tmpdir("stage-deleted-cursor-rule");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("guard", ItemKind::Hook, &["cursor"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".cursor/rules/safety-guard.mdc"),
            "managed cursor hook\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        std::fs::remove_file(project.join(".cursor/rules/safety-guard.mdc")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("M\t.vstack-lock.json"), "{staged}");
        assert!(
            staged.contains("D\t.cursor/rules/safety-guard.mdc"),
            "{staged}"
        );
    });
}

#[test]
fn staging_does_not_stage_opencode_hook_instructions_without_hook_ownership() {
    let project = tmpdir("stage-unowned-opencode-hook");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join(".opencode/instructions/vstack-hook-consumer.md"),
            "consumer instruction\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
        assert!(
            !staged.contains(".opencode/instructions/vstack-hook-consumer.md"),
            "{staged}"
        );
    });
}

#[test]
fn retry_staging_records_deleted_owned_opencode_hook_instructions_from_committed_lock() {
    let project = tmpdir("stage-deleted-opencode-hook");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("guard", ItemKind::Hook, &["opencode"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".opencode/instructions/vstack-hook-guard.md"),
            "managed opencode hook\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        std::fs::remove_file(project.join(".opencode/instructions/vstack-hook-guard.md")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("M\t.vstack-lock.json"), "{staged}");
        assert!(
            staged.contains("D\t.opencode/instructions/vstack-hook-guard.md"),
            "{staged}"
        );
    });
}

#[test]
fn staging_pre_refresh_paths_records_refresh_deletions() {
    let project = tmpdir("stage-delete-project");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut pre_lock = LockFile::default();
        pre_lock.add(lock_entry("protect", ItemKind::Hook, &["opencode"]));
        pre_lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".opencode/instructions/vstack-hook-protect.md"),
            "managed hook\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        let pre_paths = project_stage_paths(&pre_lock, true).unwrap();
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        std::fs::remove_file(project.join(".opencode/instructions/vstack-hook-protect.md"))
            .unwrap();

        stage_project_paths(&pre_paths).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("M\t.vstack-lock.json"), "{staged}");
        assert!(
            staged.contains("D\t.opencode/instructions/vstack-hook-protect.md"),
            "{staged}"
        );
    });
}

#[test]
fn retry_staging_records_deleted_owned_shared_config_from_committed_lock() {
    let project = tmpdir("stage-deleted-owned-shared-config");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("guard", ItemKind::Hook, &["codex"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".codex/config.toml"),
            "approval_policy = \"never\"\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        std::fs::remove_file(project.join(".codex/config.toml")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("M\t.vstack-lock.json"), "{staged}");
        assert!(staged.contains("D\t.codex/config.toml"), "{staged}");
    });
}

#[test]
fn retry_staging_records_deleted_claude_and_codex_hook_scripts() {
    let project = tmpdir("stage-deleted-native-hooks");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry(
            "guard",
            ItemKind::Hook,
            &["claude-code", "codex"],
        ));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".claude/hooks/guard.sh"),
            "managed claude hook\n",
        );
        write_file(
            &project.join(".codex/hooks/guard.sh"),
            "managed codex hook\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".claude/hooks/guard.sh")).unwrap();
        std::fs::remove_file(project.join(".codex/hooks/guard.sh")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("D\t.claude/hooks/guard.sh"), "{staged}");
        assert!(staged.contains("D\t.codex/hooks/guard.sh"), "{staged}");
    });
}

#[test]
fn retry_staging_does_not_stage_consumer_owned_deleted_native_hooks() {
    let project = tmpdir("stage-consumer-owned-native-hooks");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join(".claude/hooks/consumer.sh"),
            "consumer claude hook\n",
        );
        write_file(
            &project.join(".codex/hooks/consumer.sh"),
            "consumer codex hook\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".claude/hooks/consumer.sh")).unwrap();
        std::fs::remove_file(project.join(".codex/hooks/consumer.sh")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(!staged.contains(".claude/hooks/consumer.sh"), "{staged}");
        assert!(!staged.contains(".codex/hooks/consumer.sh"), "{staged}");
    });
}

#[test]
fn staging_skips_ignored_managed_paths_and_stages_remaining_paths() {
    let project = tmpdir("stage-ignored-project");
    let source = tmpdir("stage-ignored-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/ignored-pkg/package.json"),
        r#"{"name":"ignored-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("ignored-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(&project.join(".gitignore"), ".pi/packages/ignored-pkg/\n");
        write_file(
            &project.join(".pi/packages/ignored-pkg/package.json"),
            r#"{"name":"ignored-pkg","pi":{"extensions":[]}}"#,
        );

        let paths = project_stage_paths(&lock, false).unwrap();
        assert!(paths.contains(&PathBuf::from(".pi/packages/ignored-pkg/package.json")));
        stage_paths(&paths).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
        assert!(!staged.contains(".pi/packages/ignored-pkg"), "{staged}");
    });
}

#[test]
fn staging_records_tracked_pi_node_modules_deletions_without_untracked_dependencies() {
    let project = tmpdir("stage-pi-node-modules-delete");
    let source = tmpdir("stage-pi-node-modules-delete-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/dep-pkg/package.json"),
        r#"{"name":"dep-pkg","pi":{"extensions":[]}}"#,
    );
    write_file(
        &source.join("pi-extensions/dep-pkg/node_modules/old/index.js"),
        "tracked dependency\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("dep-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/dep-pkg/package.json"),
            r#"{"name":"dep-pkg","pi":{"extensions":[]}}"#,
        );
        write_file(
            &project.join(".pi/packages/dep-pkg/node_modules/old/index.js"),
            "tracked dependency\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".pi/packages/dep-pkg/node_modules/old/index.js"))
            .unwrap();
        write_file(
            &project.join(".pi/packages/dep-pkg/node_modules/new/index.js"),
            "untracked dependency\n",
        );

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("D\t.pi/packages/dep-pkg/node_modules/old/index.js"),
            "{staged}"
        );
        assert!(
            !staged.contains(".pi/packages/dep-pkg/node_modules/new/index.js"),
            "{staged}"
        );
    });
}

#[test]
fn staging_scopes_pi_package_to_source_owned_files() {
    let project = tmpdir("stage-pi-source-owned-files");
    let source = tmpdir("stage-pi-source-owned-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":["extensions/owned.ts"]}}"#,
    );
    write_file(
        &source.join("pi-extensions/demo-pkg/extensions/owned.ts"),
        "owned\n",
    );
    write_file(
        &source.join("pi-extensions/demo-pkg/extensions/new-upstream.ts"),
        "new upstream\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["extensions/owned.ts"]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/extensions/owned.ts"),
            "owned\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["extensions/owned.ts","extensions/new-upstream.ts"]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/extensions/new-upstream.ts"),
            "new upstream\n",
        );
        std::fs::remove_file(project.join(".pi/packages/demo-pkg/extensions/owned.ts")).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/consumer-secret.txt"),
            "consumer secret\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("M\t.pi/packages/demo-pkg/package.json"),
            "{staged}"
        );
        assert!(
            staged.contains("A\t.pi/packages/demo-pkg/extensions/new-upstream.ts"),
            "{staged}"
        );
        assert!(
            staged.contains("D\t.pi/packages/demo-pkg/extensions/owned.ts"),
            "{staged}"
        );
        assert!(!staged.contains(".pi/packages/demo-pkg/consumer-secret.txt"));
    });
}

#[test]
fn project_stage_paths_fail_when_locked_pi_source_is_unavailable() {
    let project = tmpdir("stage-pi-missing-source");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("demo-pkg", ItemKind::PiExtension, &["pi"]));
        lock.save(&config::lock_file_path(false)).unwrap();

        let err = project_stage_paths(&lock, false).unwrap_err().to_string();
        assert!(err.contains("demo-pkg"), "{err}");
        assert!(err.contains("source"), "{err}");
    });
}

#[test]
fn retry_staging_does_not_stage_consumer_pi_package_deletions() {
    let project = tmpdir("stage-pi-consumer-delete");
    let source = tmpdir("stage-pi-consumer-delete-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/consumer-data.txt"),
            "consumer data\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".pi/packages/demo-pkg/consumer-data.txt")).unwrap();

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            !staged.contains(".pi/packages/demo-pkg/consumer-data.txt"),
            "{staged}"
        );
    });
}

#[test]
fn retry_staging_records_deleted_pi_bin_from_committed_manifest_only() {
    let project = tmpdir("stage-deleted-pi-bin");
    let source = tmpdir("stage-deleted-pi-bin-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("demo-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"old-cmd":"bin/old.js"}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/bin/old.js"),
            "old bin\n",
        );
        write_file(&project.join(".pi/bin/old-cmd"), "managed old link\n");
        write_file(&project.join(".pi/bin/consumer-cmd"), "consumer link\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{}}"#,
        );
        std::fs::remove_file(project.join(".pi/packages/demo-pkg/bin/old.js")).unwrap();
        std::fs::remove_file(project.join(".pi/bin/old-cmd")).unwrap();
        std::fs::remove_file(project.join(".pi/bin/consumer-cmd")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("M\t.pi/packages/demo-pkg/package.json"),
            "{staged}"
        );
        assert!(
            staged.contains("D\t.pi/packages/demo-pkg/bin/old.js"),
            "{staged}"
        );
        assert!(staged.contains("D\t.pi/bin/old-cmd"), "{staged}");
        assert!(!staged.contains(".pi/bin/consumer-cmd"), "{staged}");
    });
}

#[test]
fn retry_staging_accepts_pi_bin_names_valid_for_installer() {
    let project = tmpdir("stage-deleted-pi-bin-tilde");
    let source = tmpdir("stage-deleted-pi-bin-tilde-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("demo-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"old~cmd":"bin/old.js"}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/bin/old.js"),
            "old bin\n",
        );
        write_file(&project.join(".pi/bin/old~cmd"), "managed old link\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{}}"#,
        );
        std::fs::remove_file(project.join(".pi/packages/demo-pkg/bin/old.js")).unwrap();
        std::fs::remove_file(project.join(".pi/bin/old~cmd")).unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(staged.contains("D\t.pi/bin/old~cmd"), "{staged}");
    });
}

#[test]
fn staging_project_owned_skills_keeps_unmanaged_descendants_unstaged() {
    let project = tmpdir("stage-project-owned-skill");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("project-skills/local/SKILL.md"),
            "---\nname: local\ndescription: Local skill\n---\n\n# Local\n",
        );
        write_file(
            &project.join("project-skills/local/notes.md"),
            "consumer notes\n",
        );
        write_file(
            &project.join(".agents/skills/default-local/SKILL.md"),
            "---\nname: default-local\ndescription: Local skill\n---\n\n# Local\n",
        );
        write_file(
            &project.join(".agents/skills/default-local/notes.md"),
            "consumer notes\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains("project-skills/local/SKILL.md\n"),
            "{staged}"
        );
        assert!(
            !staged.contains("project-skills/local/notes.md"),
            "{staged}"
        );
        assert!(
            staged.contains(".agents/skills/default-local/SKILL.md\n"),
            "{staged}"
        );
        assert!(
            !staged.contains(".agents/skills/default-local/notes.md"),
            "{staged}"
        );
    });
}

/// `refresh` links `.agents/skills/<name>` at the tracked skill under
/// `project-skills-dir`; git refuses any pathspec that walks through that link
/// ("is beyond a symbolic link"), so staging must name the link target's real
/// path instead of its through-link descendant.
#[cfg(unix)]
#[test]
fn staging_relocated_project_skills_names_the_real_tracked_path() {
    let project = tmpdir("stage-relocated-project-skill");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("project-skills/local/SKILL.md"),
            "---\nname: local\ndescription: Local skill\n---\n\n# Local\n",
        );
        std::fs::create_dir_all(project.join(".agents/skills")).unwrap();
        std::os::unix::fs::symlink(
            "../../project-skills/local",
            project.join(".agents/skills/local"),
        )
        .unwrap();

        // Control: the link really does resolve, so a check that follows it
        // sees a file and the assertions below are not vacuous.
        assert!(
            project.join(".agents/skills/local/SKILL.md").is_file(),
            "the fixture link does not resolve, so nothing here exercises through-link staging"
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains("project-skills/local/SKILL.md\n"),
            "{staged}"
        );
        assert!(
            !staged.contains(".agents/skills/local/SKILL.md"),
            "{staged}"
        );
    });
}

/// A `project-skills-dir` entry can itself be a link at another in-repo skill,
/// a layout `refresh` accepts and updates through. Nothing else enumerates the
/// target, so skipping every link left the modified file unstaged.
#[cfg(unix)]
#[test]
fn staging_a_linked_project_skills_entry_names_its_in_repo_target() {
    let project = tmpdir("stage-linked-project-skill-target");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("vendor-skills/shared/SKILL.md"),
            "---\nname: shared\ndescription: Shared skill\n---\n\n# Shared\n",
        );
        std::fs::create_dir_all(project.join("project-skills")).unwrap();
        std::os::unix::fs::symlink(
            "../vendor-skills/shared",
            project.join("project-skills/shared"),
        )
        .unwrap();

        // Control: the link really does resolve, so the assertions below are
        // about staging and not about a fixture that names nothing.
        assert!(
            project.join("project-skills/shared/SKILL.md").is_file(),
            "the fixture link does not resolve, so nothing here exercises a linked entry"
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains("vendor-skills/shared/SKILL.md\n"),
            "{staged}"
        );
        assert!(
            !staged.contains("project-skills/shared/SKILL.md"),
            "git refuses a pathspec that walks through the link: {staged}"
        );
    });
}

/// A link that leaves the project has no in-project path git could stage, and
/// must not drag an outside file into the consumer's commit.
#[cfg(unix)]
#[test]
fn staging_ignores_a_project_skills_entry_linked_outside_the_project() {
    let outside = tmpdir("stage-linked-skill-outside");
    let project = tmpdir("stage-linked-skill-project");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &outside.join("shared/SKILL.md"),
        "---\nname: shared\ndescription: Shared skill\n---\n\n# Shared\n",
    );

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        std::fs::create_dir_all(project.join("project-skills")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("shared"),
            project.join("project-skills/outside"),
        )
        .unwrap();

        // Control: the link resolves, so the empty staging below is the
        // containment check and not a broken fixture.
        assert!(
            project.join("project-skills/outside/SKILL.md").is_file(),
            "the fixture link does not resolve"
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains(".vstack-lock.json"),
            "staging did not run at all: {staged}"
        );
        assert!(!staged.contains("SKILL.md"), "{staged}");
    });
}

#[test]
fn staging_is_scoped_to_nested_project_paths_from_git_top_level() {
    let repo = tmpdir("stage-nested-repo");
    let project = repo.join("apps").join("consumer");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&repo);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("protect", ItemKind::Hook, &["opencode"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".opencode/instructions/vstack-hook-protect.md"),
            "managed hook\n",
        );
        write_file(
            &repo.join(".opencode/instructions/vstack-hook-protect.md"),
            "outside project\n",
        );

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&repo, &["diff", "--cached", "--name-only"]);
        let staged_lines: Vec<&str> = staged.lines().collect();
        assert!(
            staged_lines.contains(&"apps/consumer/.opencode/instructions/vstack-hook-protect.md"),
            "{staged}"
        );
        assert!(
            !staged_lines.contains(&".opencode/instructions/vstack-hook-protect.md"),
            "{staged}"
        );
    });
}

#[test]
fn git_project_preserves_trailing_whitespace_in_repo_root() {
    let root = tmpdir("git-root-space");
    let project = root.join("repo ");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let git = git_project().unwrap();
        assert_eq!(git.root, std::fs::canonicalize(&project).unwrap());
        assert!(
            git.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(' '),
            "negative control: repo root must end in whitespace"
        );
    });
}

#[cfg(unix)]
#[test]
fn staging_ignores_unrelated_non_utf8_paths() {
    let project = tmpdir("stage-non-utf8");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("protect", ItemKind::Hook, &["opencode"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".opencode/instructions/vstack-hook-protect.md"),
            "managed hook\n",
        );
        let non_utf8 = project.join(PathBuf::from(OsString::from_vec(b"outside-\xff".to_vec())));
        std::fs::write(non_utf8, "ignored\n").unwrap();

        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".opencode/instructions/vstack-hook-protect.md\n"));
    });
}

#[test]
fn no_drift_without_stage_fails_when_verify_fails() {
    let project = tmpdir("no-stage-no-drift-verify-fails");
    let source = tmpdir("no-stage-no-drift-pi-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/.vstack-source.json"),
            &format!(
                r#"{{"demo-pkg":{{"sourcePath":"{}"}}}}"#,
                source.join("pi-extensions/demo-pkg").display()
            ),
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["corrupt"]}}"#,
        );

        let err = run(ScopeFilter::Project, false, false, false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("verification failed"), "{err}");
    });
}

#[test]
fn stage_mode_verifies_and_stages_when_hashes_are_current() {
    let project = tmpdir("stage-no-drift-project");
    let source = tmpdir("stage-no-drift-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_copy_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v1\n");
        // `vstack.toml` is consumer-authored project config, so it is committed
        // and clean here: a pre-existing edit to it is refused, not absorbed.
        write_file(&project.join("vstack.toml"), "[agent-skills]\n");
        git(&project, &["add", "vstack.toml"]);
        git(&project, &["commit", "-m", "project config"]);
        write_file(
            &project.join(".pi/packages/manual/package.json"),
            r#"{"name":"manual","pi":{"extensions":[]}}"#,
        );

        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".agents/skills/demo/SKILL.md"), "{staged}");
        assert!(staged.contains(".claude/skills/demo/SKILL.md"), "{staged}");
        assert!(
            !staged.contains(".pi/packages/manual/package.json"),
            "{staged}"
        );

        // A consumer edit to the project config blocks the automated commit.
        write_file(&project.join("vstack.toml"), "[agent-skills]\nrust = []\n");
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains("vstack.toml"), "{err}");
    });
}

/// `verify::run` reads a locked skill's `SKILL.md` for existence and its
/// harness path for canonical identity, and no file's contents. A replaced
/// auxiliary script, one deleted outright, one whose execute bit was cleared,
/// and a rewritten `SKILL.md` all pass it — and staging owns every one of those
/// paths, so `--stage` would commit the breakage as though propagation produced
/// it. The source never moved, so every later run reports no drift and the
/// broken skill stays committed.
#[test]
fn stage_mode_rejects_an_installed_skill_that_no_longer_matches_its_source() {
    let project = tmpdir("stage-skill-content-drift");
    let source = tmpdir("stage-skill-content-drift-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");
    write_file(
        &source.join("skills/demo/scripts/helper.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    write_file(&source.join("skills/demo/docs/note.md"), "note\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            source.join("skills/demo/scripts/helper.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_copy_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        let install = project.join(".claude/skills/demo");
        let write_faithful_install = || {
            write_installed_skill_md(&install.join("SKILL.md"), "v1\n");
            write_file(&install.join("scripts/helper.sh"), "#!/bin/sh\nexit 0\n");
            write_file(&install.join("docs/note.md"), "note\n");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    install.join("scripts/helper.sh"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
        };
        write_faithful_install();

        // Control: the fixture is an install propagation would produce, so
        // every refusal below is the edit made to it and not fixture noise.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "baseline"]);
        assert!(
            git_output(&project, &["status", "--porcelain"]).is_empty(),
            "the control run must leave a clean tree"
        );

        // A replaced auxiliary script.
        write_file(&install.join("scripts/helper.sh"), "#!/bin/sh\nrm -rf /\n");
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(
            err.contains(".claude/skills/demo/scripts/helper.sh does not match the locked source"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // An auxiliary file deleted outright: the tracked deletion staging
        // would otherwise record as a propagated removal.
        write_faithful_install();
        std::fs::remove_file(install.join("docs/note.md")).unwrap();
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(".claude/skills/demo/docs/note.md is missing from the install"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // Identical bytes with the execute bit cleared still cannot run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            write_faithful_install();
            std::fs::set_permissions(
                install.join("scripts/helper.sh"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let err = run(ScopeFilter::Project, false, false, true, true)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(".claude/skills/demo/scripts/helper.sh does not carry the file mode"),
                "{err}"
            );
            assert!(
                git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
                "nothing may be staged"
            );
        }

        // A rewritten SKILL.md body.
        write_faithful_install();
        write_installed_skill_md(&install.join("SKILL.md"), "locally rewritten\n");
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(".claude/skills/demo/SKILL.md does not match the locked source"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

/// The installed `SKILL.md` is rendered, not copied: it carries the consumer's
/// `[skill-instructions]` section and the do-not-edit notice. Holding it to the
/// raw source bytes would refuse every correctly rendered install, so the
/// expectation is rendered the same way the installer renders it.
#[test]
fn stage_mode_holds_an_installed_skill_md_to_its_rendered_project_instructions() {
    let project = tmpdir("stage-skill-rendered-instructions");
    let source = tmpdir("stage-skill-rendered-instructions-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        write_file(
            &project.join("vstack.toml"),
            "[skill-instructions]\ndemo = \"House rule.\"\n",
        );
        // Consumer-authored config is committed and clean, or the shared-config
        // guard refuses before the install is ever looked at.
        git(&project, &["add", "vstack.toml"]);
        git(&project, &["commit", "-m", "project config"]);
        let mut entry = demo_copy_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        let skill_md = project.join(".claude/skills/demo/SKILL.md");
        let rendered = crate::skill::render_installed_skill_md(
            &demo_skill_source("v1\n"),
            Some("House rule."),
        );
        // Control: the configured section really is part of the rendering, so
        // the acceptance below is not a comparison against the raw source.
        assert!(
            rendered.contains("House rule.") && !demo_skill_source("v1\n").contains("House rule."),
            "the fixture instructions must change the rendered SKILL.md"
        );
        write_file(&skill_md, &rendered);

        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "baseline"]);

        // The raw source bytes are what an install that dropped the rendering
        // carries, and it is no longer the file propagation produces.
        write_file(&skill_md, &demo_skill_source("v1\n"));
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(".claude/skills/demo/SKILL.md does not match the locked source"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_scopes_locked_skill_staging_to_source_owned_files() {
    let project = tmpdir("stage-locked-skill-owned-files");
    let source = tmpdir("stage-locked-skill-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");
    write_file(&source.join("skills/demo/owned.txt"), "owned\n");
    write_file(
        &source.join("skills/demo/new-upstream.md"),
        "new upstream\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_copy_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_file(&project.join(".agents/skills/demo/owned.txt"), "owned\n");
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v1\n");
        write_file(&project.join(".claude/skills/demo/owned.txt"), "owned\n");
        // Committed under the locked skill, and gone from the source: the file
        // an upstream removal takes out of the install on the next refresh.
        write_file(&project.join(".claude/skills/demo/retired.md"), "retired\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_installed_skill_md(
            &project.join(".agents/skills/demo/SKILL.md"),
            "managed edit\n",
        );
        write_file(
            &project.join(".agents/skills/demo/new-upstream.md"),
            "new upstream\n",
        );
        write_file(
            &project.join(".claude/skills/demo/new-upstream.md"),
            "new upstream\n",
        );
        std::fs::remove_file(project.join(".claude/skills/demo/retired.md")).unwrap();
        write_file(
            &project.join(".agents/skills/demo/consumer-secret.txt"),
            "consumer secret\n",
        );
        write_file(
            &project.join(".claude/skills/demo/consumer-note.md"),
            "consumer note\n",
        );

        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("M\t.agents/skills/demo/SKILL.md"),
            "{staged}"
        );
        assert!(
            staged.contains("A\t.agents/skills/demo/new-upstream.md"),
            "{staged}"
        );
        assert!(
            staged.contains("D\t.claude/skills/demo/retired.md"),
            "{staged}"
        );
        assert!(
            !staged.contains(".agents/skills/demo/consumer-secret.txt"),
            "{staged}"
        );
        assert!(
            !staged.contains(".claude/skills/demo/consumer-note.md"),
            "{staged}"
        );
    });
}

#[test]
fn managed_status_pathspecs_collapse_large_owned_file_sets_to_bounded_roots() {
    let mut seed_paths = BTreeSet::new();
    for idx in 0..2048 {
        seed_paths.insert(PathBuf::from(format!(".agents/skills/demo/file-{idx}.md")));
        seed_paths.insert(PathBuf::from(format!(".claude/skills/demo/file-{idx}.md")));
    }
    let owned_exact_paths = owned_exact_status_paths(&seed_paths);
    let empty = BTreeSet::new();

    let pathspecs = managed_status_pathspecs(
        &seed_paths,
        &owned_exact_paths,
        &empty,
        &empty,
        &empty,
        &empty,
        &empty,
        &empty,
    )
    .unwrap();

    assert!(
        pathspecs.len() <= 16,
        "pathspecs should stay bounded, got {}",
        pathspecs.len()
    );
    assert!(pathspecs.contains(&PathBuf::from(".agents/skills")));
    assert!(pathspecs.contains(&PathBuf::from(".claude/skills")));
    assert!(
        !pathspecs.contains(&PathBuf::from(".agents/skills/demo/file-2047.md")),
        "individual owned files must not be emitted as CLI pathspecs"
    );
}

#[test]
fn no_drift_stage_verifies_pi_from_resolved_source_when_sidecar_path_is_stale() {
    let project = tmpdir("stage-no-drift-pi-stale-sidecar");
    let source = tmpdir("stage-no-drift-pi-current-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":["extensions/demo.ts"]}}"#,
    );
    write_file(
        &source.join("pi-extensions/demo-pkg/extensions/demo.ts"),
        "export default {}\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/.vstack-source.json"),
            r#"{"demo-pkg":{"sourcePath":"/stale/machine/pi-extensions/demo-pkg"}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["extensions/demo.ts"]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/extensions/demo.ts"),
            "export default {}\n",
        );
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":["./packages/demo-pkg"]}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        run(ScopeFilter::Project, false, false, true, true).unwrap();
    });
}

#[test]
fn retry_stage_records_locked_skill_deletions_from_committed_install() {
    let project = tmpdir("stage-locked-skill-committed-delete");
    let source = tmpdir("stage-locked-skill-committed-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");
    write_file(&source.join("skills/demo/removed-upstream.md"), "removed\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_copy_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_file(
            &project.join(".agents/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v1\n");
        write_file(
            &project.join(".claude/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_skill_source(&source, "v2\n");
        std::fs::remove_file(source.join("skills/demo/removed-upstream.md")).unwrap();
        let mut lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        let entry = lock.entries.get_mut("demo").unwrap();
        entry.source_hash = config::compute_source_hash(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v2\n");
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v2\n");
        std::fs::remove_file(project.join(".agents/skills/demo/removed-upstream.md")).unwrap();
        std::fs::remove_file(project.join(".claude/skills/demo/removed-upstream.md")).unwrap();

        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("D\t.agents/skills/demo/removed-upstream.md"),
            "{staged}"
        );
        assert!(
            staged.contains("D\t.claude/skills/demo/removed-upstream.md"),
            "{staged}"
        );
    });
}

#[test]
fn retry_stage_records_vendored_skill_deletions_when_lock_is_ignored() {
    let project = tmpdir("stage-locked-skill-ignored-lock-delete");
    let source = tmpdir("stage-locked-skill-ignored-lock-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v2\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(&project.join(".gitignore"), ".vstack-lock.json\n");
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_file(
            &project.join(".agents/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        write_file(
            &project.join(".agents/skills/demo/.vstack-refreshed"),
            "1\n",
        );
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v1\n");
        write_file(
            &project.join(".claude/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        write_file(
            &project.join(".claude/skills/demo/.vstack-refreshed"),
            "1\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v2\n");
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v2\n");
        std::fs::remove_file(project.join(".agents/skills/demo/removed-upstream.md")).unwrap();
        std::fs::remove_file(project.join(".claude/skills/demo/removed-upstream.md")).unwrap();
        write_file(
            &project.join(".agents/skills/demo/consumer-secret.txt"),
            "consumer secret\n",
        );

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("D\t.agents/skills/demo/removed-upstream.md"),
            "{staged}"
        );
        assert!(
            staged.contains("D\t.claude/skills/demo/removed-upstream.md"),
            "{staged}"
        );
        assert!(
            !staged.contains(".agents/skills/demo/consumer-secret.txt"),
            "{staged}"
        );
    });
}

#[cfg(unix)]
#[test]
fn stage_mode_stages_locked_skill_symlink_root_without_absorbing_target_children() {
    let project = tmpdir("stage-locked-skill-symlink-root");
    let source = tmpdir("stage-locked-skill-symlink-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_file(
            &project.join(".agents/skills/demo/consumer-secret.txt"),
            "consumer secret\n",
        );
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        std::os::unix::fs::symlink(
            Path::new("../../.agents/skills/demo"),
            project.join(".claude/skills/demo"),
        )
        .unwrap();

        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("A\t.agents/skills/demo/SKILL.md"),
            "{staged}"
        );
        assert!(staged.contains("A\t.claude/skills/demo"), "{staged}");
        assert!(
            !staged.contains(".agents/skills/demo/consumer-secret.txt"),
            "{staged}"
        );
    });
}

#[test]
fn stage_mode_fails_before_staging_when_no_drift_verify_fails() {
    let project = tmpdir("stage-no-drift-verify-fails");
    let source = tmpdir("stage-no-drift-pi-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/.vstack-source.json"),
            &format!(
                r#"{{"demo-pkg":{{"sourcePath":"{}"}}}}"#,
                source.join("pi-extensions/demo-pkg").display()
            ),
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["corrupt"]}}"#,
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("verification failed"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn hook_registration_requires_the_exact_installer_command_under_its_event() {
    let expected = crate::installer::claude_project_hook_command("guard");
    let registration = |command: &str| {
        serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{ "type": "command", "command": command }]
                }]
            }
        })
    };
    let matcher = Some("Bash");

    assert!(hook_command_registered(
        &registration(&expected),
        &expected,
        "PreToolUse",
        matcher
    ));
    assert!(
        !hook_command_registered(&registration(&expected), &expected, "Stop", matcher),
        "a registration under another event is not a registration for this one"
    );
    assert!(
        !hook_command_registered(
            &registration(".claude/hooks/guard.sh"),
            &expected,
            "PreToolUse",
            matcher
        ),
        "a bare path is not the installer-generated command"
    );
    assert!(
        !hook_command_registered(
            &registration(&format!("echo {expected}")),
            &expected,
            "PreToolUse",
            matcher
        ),
        "a command that merely mentions the script must not pass"
    );
    assert!(
        !hook_command_registered(
            &registration(&expected.replace(".sh", ".sh.disabled")),
            &expected,
            "PreToolUse",
            matcher
        ),
        "a disabled script path must not pass"
    );
    assert!(
        !hook_command_registered(
            &serde_json::json!({ "note": expected.clone(), "hooks": {} }),
            &expected,
            "PreToolUse",
            matcher
        ),
        "a path in unrelated metadata is not a registration"
    );
    assert!(
        !hook_command_registered(
            &registration(&expected),
            &expected,
            "PreToolUse",
            Some("Edit")
        ),
        "a rewritten matcher disables the hook for its intended tool calls"
    );
    assert!(
        !hook_command_registered(&registration(&expected), &expected, "PreToolUse", None),
        "an unmatched-matcher entry is not an unmatchered registration"
    );
}

#[test]
fn hook_registration_fails_closed_when_the_locked_event_cannot_be_read() {
    let project = tmpdir("hook-registration-unknown-event");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let expected = crate::installer::claude_project_hook_command("guard");
        write_file(
            &project.join(".claude/settings.json"),
            &serde_json::json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": expected }]
                    }]
                }
            })
            .to_string(),
        );

        // The source definition could not be read: a dependency failure must
        // not be accepted as "registered under some event".
        let mut failures = Vec::new();
        require_hook_command_registration(
            Path::new(".claude/settings.json"),
            &expected,
            None,
            "Claude hook guard",
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("cannot read the locked event")),
            "{failures:?}"
        );

        // With the event and matcher known, the same config verifies.
        let mut failures = Vec::new();
        require_hook_command_registration(
            Path::new(".claude/settings.json"),
            &expected,
            Some(("PreToolUse", Some("Bash"))),
            "Claude hook guard",
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");
    });
}

#[test]
fn stage_mode_fails_before_staging_when_locked_skill_harness_path_is_missing() {
    let project = tmpdir("stage-no-drift-skill-harness-verify-fails");
    let source = tmpdir("stage-no-drift-skill-harness-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        write_installed_skill_md(&project.join(".claude/skills/demo/SKILL.md"), "v1\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".claude/skills/demo/SKILL.md")).unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("verification failed"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn stage_mode_fails_before_staging_when_claude_hook_settings_missing() {
    let project = tmpdir("stage-no-drift-claude-settings-missing");
    let source = tmpdir("stage-no-drift-claude-settings-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".claude/settings.json")).unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".claude/settings.json"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn stage_mode_fails_before_staging_when_codex_hooks_registry_missing() {
    let project = tmpdir("stage-no-drift-codex-hooks-missing");
    let source = tmpdir("stage-no-drift-codex-hooks-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["codex"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".codex/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".codex/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash \"$(git rev-parse --show-toplevel)/.codex/hooks/guard.sh\""}]}]}}"#,
        );
        write_file(
            &project.join(".codex/config.toml"),
            "[features]\nhooks = true\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".codex/hooks.json")).unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".codex/hooks.json"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn stage_mode_fails_before_staging_when_pi_settings_missing() {
    let project = tmpdir("stage-no-drift-pi-settings-missing");
    let source = tmpdir("stage-no-drift-pi-settings-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
    );
    write_file(&source.join("pi-extensions/demo-pkg/bin/tool.js"), "tool\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/.vstack-source.json"),
            &format!(
                r#"{{"demo-pkg":{{"sourcePath":"{}"}}}}"#,
                source.join("pi-extensions/demo-pkg").display()
            ),
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
        );
        write_file(&project.join(".pi/packages/demo-pkg/bin/tool.js"), "tool\n");
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":["./packages/demo-pkg"]}"#,
        );
        write_file(&project.join(".pi/bin/demo-tool"), "managed bin link\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".pi/settings.json")).unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".pi/settings.json"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
#[cfg(unix)]
fn stage_mode_fails_before_staging_when_current_pi_bin_missing() {
    let project = tmpdir("stage-no-drift-pi-bin-missing");
    let source = tmpdir("stage-no-drift-pi-bin-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
    );
    write_file(&source.join("pi-extensions/demo-pkg/bin/tool.js"), "tool\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/.vstack-source.json"),
            &format!(
                r#"{{"demo-pkg":{{"sourcePath":"{}"}}}}"#,
                source.join("pi-extensions/demo-pkg").display()
            ),
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
        );
        write_file(&project.join(".pi/packages/demo-pkg/bin/tool.js"), "tool\n");
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":["./packages/demo-pkg"]}"#,
        );
        write_file(&project.join(".pi/bin/demo-tool"), "managed bin link\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        std::fs::remove_file(project.join(".pi/bin/demo-tool")).unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".pi/bin/demo-tool"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn stage_mode_refreshes_verifies_updates_lock_and_stages_outputs() {
    let project = tmpdir("stage-real-refresh-project");
    let source = tmpdir("stage-real-refresh-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_skill_source(&source, "v1\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = demo_entry(&source);
        entry.method = InstallMethod::Copy;
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "v1\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_skill_source(&source, "v2\n");
        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        let entry = lock.entries.get("demo").unwrap();
        assert_eq!(entry.source_hash, config::compute_source_hash(entry));
        assert!(
            std::fs::read_to_string(project.join(".claude/skills/demo/SKILL.md"))
                .unwrap()
                .contains("v2")
        );
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
        assert!(
            staged.contains(".claude/skills/demo/SKILL.md\n"),
            "{staged}"
        );
    });
}

#[test]
fn git_head_marker_lookup_fails_closed_when_ls_tree_fails() {
    let project = tmpdir("git-head-marker-fail-closed");
    std::fs::create_dir_all(&project).unwrap();

    // Not a git repository: `git ls-tree HEAD` exits non-zero. Reporting that
    // as "marker absent" would silently drop managed paths from staging.
    let git = GitProject {
        root: project.clone(),
        prefix: PathBuf::new(),
    };
    let err = git_head_has_project_path(&git, Path::new(".agents/skills/demo/.vstack-refreshed"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("git ls-tree failed"), "{err}");
}

#[test]
fn stage_mode_fails_before_staging_when_codex_hooks_feature_is_off() {
    let project = tmpdir("stage-no-drift-codex-hooks-feature-off");
    let source = tmpdir("stage-no-drift-codex-hooks-feature-off-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["codex"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".codex/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".codex/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash \"$(git rev-parse --show-toplevel)/.codex/hooks/guard.sh\""}]}]}}"#,
        );
        write_file(
            &project.join(".codex/config.toml"),
            "[features]\nhooks = true\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Codex will not run the locked hook with the feature off, so this must
        // not be stageable even though hooks.json still registers the script.
        write_file(
            &project.join(".codex/config.toml"),
            "[features]\nhooks = false\n",
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".codex/config.toml"), "{err}");
        assert!(err.contains("hooks = true"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");

        // A deleted config.toml is the same broken install.
        std::fs::remove_file(project.join(".codex/config.toml")).unwrap();
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".codex/config.toml"), "{err}");
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.is_empty(), "{staged}");
    });
}

#[test]
fn staging_leaves_consumer_owned_append_system_alone_and_stages_an_owned_one() {
    let project = tmpdir("stage-pi-append-system-ownership");
    let source = tmpdir("stage-pi-append-system-ownership-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        // Installed manifest declares no appendSystem, so the prompt file below
        // is entirely the consumer's.
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
        );
        write_file(
            &project.join(".pi/APPEND_SYSTEM.md"),
            "consumer house rules\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/APPEND_SYSTEM.md"),
            "consumer house rules, revised\n",
        );
        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(!staged.contains(".pi/APPEND_SYSTEM.md"), "{staged}");

        // A manifest that owns an append-system block puts the file back in scope.
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[],"appendSystem":"APPEND.md"}}"#,
        );
        write_file(&project.join(".pi/packages/demo-pkg/APPEND.md"), "rule\n");
        stage_project_paths(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".pi/APPEND_SYSTEM.md"), "{staged}");
    });
}

/// The guard runs BEFORE the refresh, so it must read the source the refresh is
/// about to install from, not only the package already on disk. An updated
/// package that adds `pi.appendSystem` for the first time leaves both
/// on-disk signals clean — the installed manifest declares none and a
/// consumer-authored prompt carries no managed marker — while refresh is about
/// to write a managed block into that very file and stage the whole thing.
#[test]
fn pre_refresh_guard_covers_an_append_system_the_updated_source_newly_declares() {
    let project = tmpdir("stage-pi-append-system-new-upstream");
    let source = tmpdir("stage-pi-append-system-new-upstream-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    // The updated source declares an append-system block for the first time.
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[],"appendSystem":"APPEND.md"}}"#,
    );
    write_file(&source.join("pi-extensions/demo-pkg/APPEND.md"), "rule\n");

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("demo-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        // The installed package is the older one, without appendSystem.
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
        );
        write_file(
            &project.join(".pi/APPEND_SYSTEM.md"),
            "consumer house rules\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // The consumer's own uncommitted prompt edit.
        write_file(
            &project.join(".pi/APPEND_SYSTEM.md"),
            "consumer house rules, revised\n",
        );

        // Control: both signals the pre-refresh guard used to have read clean
        // here, so a covered path below can only come from the source.
        let append_system = crate::pi_extension::append_system_path(false);
        assert!(
            !crate::pi_extension::append_system_has_managed_block(&append_system).unwrap(),
            "control failed: the consumer prompt already carries a managed block"
        );
        let installed =
            std::fs::read_to_string(project.join(".pi/packages/demo-pkg/package.json")).unwrap();
        assert!(
            !installed.contains("appendSystem"),
            "control failed: the installed package already declares appendSystem"
        );

        let stage_paths = pre_refresh_project_stage_paths().unwrap();
        let dirty = dirty_shared_config_paths(&stage_paths).unwrap();
        assert!(
            dirty.contains(&PathBuf::from(".pi/APPEND_SYSTEM.md")),
            "the pre-refresh guard does not cover the prompt file the refresh is about to write: {dirty:?}"
        );
        let err = refuse_pre_existing_shared_config_edits(&dirty)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".pi/APPEND_SYSTEM.md"), "{err}");
    });
}

#[test]
#[cfg(unix)]
fn stage_mode_fails_before_staging_when_pi_bin_link_is_replaced() {
    let project = tmpdir("stage-no-drift-pi-bin-replaced");
    let source = tmpdir("stage-no-drift-pi-bin-replaced-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
    );
    write_file(&source.join("pi-extensions/demo-pkg/bin/tool.js"), "tool\n");

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
        );
        write_file(&project.join(".pi/packages/demo-pkg/bin/tool.js"), "tool\n");
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":[".pi/packages/demo-pkg"]}"#,
        );
        std::fs::create_dir_all(project.join(".pi/bin")).unwrap();
        std::os::unix::fs::symlink(
            "../packages/demo-pkg/bin/tool.js",
            project.join(".pi/bin/demo-tool"),
        )
        .unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // A consumer-owned regular file in place of the managed link.
        std::fs::remove_file(project.join(".pi/bin/demo-tool")).unwrap();
        write_file(&project.join(".pi/bin/demo-tool"), "#!/bin/sh\necho hi\n");

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".pi/bin/demo-tool"), "{err}");
        assert!(err.contains("not a symlink"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // A symlink redirected outside its own package is the same replacement.
        std::fs::remove_file(project.join(".pi/bin/demo-tool")).unwrap();
        write_file(&project.join(".pi/consumer-tool.js"), "consumer\n");
        std::os::unix::fs::symlink("../consumer-tool.js", project.join(".pi/bin/demo-tool"))
            .unwrap();

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("does not point at the target declared"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

/// The guarded set is collected after strict remote resolution, so a locked Pi
/// source that still will not resolve is no longer a cache waiting to be cloned
/// — it is a source whose manifest the guard cannot read. Skipping the item
/// there drops `.pi/APPEND_SYSTEM.md` out of the guarded set and lets the
/// staging pass absorb a consumer edit, so the collection fails closed instead.
#[test]
fn pre_refresh_stage_paths_fail_closed_on_a_pi_source_that_will_not_resolve() {
    let project = tmpdir("stage-pi-remote-source-first-run");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let mut entry = lock_entry("demo-pkg", ItemKind::PiExtension, &["pi"]);
        // A remote shorthand whose cache is not there: on a clean runner this is
        // what a source that failed to clone leaves behind.
        entry.source = "owner/repo".to_string();
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();

        // Control: the source really is unresolvable, so the failures below are
        // not some other error.
        assert!(
            config::resolve_source_path("owner/repo").is_none(),
            "control failed: the uncached remote source resolves"
        );

        let err = pre_refresh_project_stage_paths().unwrap_err().to_string();
        assert!(err.contains("demo-pkg"), "{err}");
        assert!(err.contains("source"), "{err}");

        // The post-refresh collection over the same lock fails closed too.
        let err = project_stage_paths(&lock, true).unwrap_err().to_string();
        assert!(err.contains("demo-pkg"), "{err}");
        assert!(err.contains("source"), "{err}");
    });
}

/// A Pi manifest that cannot be parsed says nothing about whether the package
/// declares `pi.appendSystem` or ships bins. Reading "it declares none" out of
/// the failure leaves `.pi/APPEND_SYSTEM.md` and the bin links outside the
/// guarded set, and the consumer's uncommitted edits to them are what the
/// staging pass then absorbs.
#[test]
fn stage_path_collection_fails_on_an_unparsable_installed_pi_manifest() {
    let project = tmpdir("stage-pi-unparsable-installed-manifest");
    let source = tmpdir("stage-pi-unparsable-installed-manifest-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(pi_entry("demo-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();

        let installed = project.join(".pi/packages/demo-pkg/package.json");
        // Control: a readable installed manifest collects without error, so the
        // failure below is the parse failure and not the fixture.
        write_file(&installed, r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#);
        project_stage_paths(&lock, true)
            .expect("control failed: a readable installed manifest must collect");

        write_file(&installed, "{ not json");
        let err = project_stage_paths(&lock, true).unwrap_err().to_string();
        assert!(
            err.contains("package.json"),
            "the unreadable installed manifest was not named: {err}"
        );

        // A package that is not installed yet has no manifest to read; that is
        // an absence, not a read failure, and still collects.
        std::fs::remove_file(&installed).unwrap();
        project_stage_paths(&lock, true)
            .expect("a package with no installed manifest must not be an error");
    });
}

/// `--stage`'s guarded path set must be collected after the drift loop, because
/// `detect_drift_for_scope` is what clones or updates the locked remote caches
/// the source manifests are read from. Collected before it, the set reads an
/// absent or revision-behind cache: an upstream package that newly declares
/// `pi.appendSystem` is invisible there, `.pi/APPEND_SYSTEM.md` stays outside
/// the guard, and the post-refresh staging pass absorbs the consumer's own edit
/// to it wholesale. No local fixture can drive a remote cache through `run`, so
/// the order is asserted over `run`'s own source; both markers are required to
/// exist, so a rename fails the test rather than passing it vacuously.
#[test]
fn run_collects_the_stage_guard_set_after_the_drift_loop_resolves_sources() {
    let source = include_str!("../propagate.rs");
    let body = source
        .split_once("pub fn run(")
        .expect("propagate::run is no longer declared under that name")
        .1
        .split_once("\n}\n")
        .expect("propagate::run has no recognizable end")
        .0;

    let resolve = body
        .find("detect_drift_for_scope(global)")
        .expect("run no longer resolves sources through detect_drift_for_scope");
    let collect = body
        .find("pre_refresh_project_stage_paths()")
        .expect("run no longer collects the pre-refresh stage paths");
    assert!(
        collect > resolve,
        "run collects the guarded stage paths at byte {collect}, ahead of the drift loop that \
         resolves sources at byte {resolve}: the set reads a stale or absent remote cache"
    );
}

#[test]
fn refreshed_staging_records_deleted_supporting_pi_files_not_named_by_the_manifest() {
    let project = tmpdir("stage-pi-supporting-delete");
    let source = tmpdir("stage-pi-supporting-delete-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    // The advanced source no longer carries `lib/helper.ts`, and no manifest
    // entrypoint ever named it.
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":["extensions/main.ts"]}}"#,
    );
    write_file(
        &source.join("pi-extensions/demo-pkg/extensions/main.ts"),
        "main\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["extensions/main.ts"]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/extensions/main.ts"),
            "main\n",
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/lib/helper.ts"),
            "helper\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Refresh cleared and re-copied the package, dropping the helper.
        std::fs::remove_file(project.join(".pi/packages/demo-pkg/lib/helper.ts")).unwrap();

        stage_project_paths_after_refresh(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("D\t.pi/packages/demo-pkg/lib/helper.ts"),
            "{staged}"
        );
    });
}

#[test]
fn stage_mode_rejects_a_codex_hooks_feature_written_as_a_string() {
    let project = tmpdir("stage-no-drift-codex-hooks-string");
    let source = tmpdir("stage-no-drift-codex-hooks-string-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["codex"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".codex/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".codex/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash \"$(git rev-parse --show-toplevel)/.codex/hooks/guard.sh\""}]}]}}"#,
        );
        write_file(
            &project.join(".codex/config.toml"),
            "[features]\nhooks = true\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // A TOML string, not a boolean: Codex leaves the runtime off.
        write_file(
            &project.join(".codex/config.toml"),
            "[features]\nhooks = \"true\"\n",
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains(".codex/config.toml"), "{err}");
        assert!(err.contains("hooks = true"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_rejects_a_hook_path_that_is_not_a_live_command_handler() {
    let project = tmpdir("stage-no-drift-hook-not-a-handler");
    let source = tmpdir("stage-no-drift-hook-not-a-handler-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        // The script path appears only in an unrelated metadata string, so the
        // harness will never invoke it.
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"note":"installed .claude/hooks/guard.sh","hooks":{}}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains("missing command registration"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
#[cfg(unix)]
fn stage_mode_rejects_a_pi_bin_link_redirected_inside_its_own_package() {
    let project = tmpdir("stage-no-drift-pi-bin-in-package-redirect");
    let source = tmpdir("stage-no-drift-pi-bin-in-package-redirect-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
    );
    write_file(&source.join("pi-extensions/demo-pkg/bin/tool.js"), "tool\n");
    // Another source-owned file inside the same package.
    write_file(
        &source.join("pi-extensions/demo-pkg/bin/other.js"),
        "other\n",
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]},"bin":{"demo-tool":"bin/tool.js"}}"#,
        );
        write_file(&project.join(".pi/packages/demo-pkg/bin/tool.js"), "tool\n");
        write_file(
            &project.join(".pi/packages/demo-pkg/bin/other.js"),
            "other\n",
        );
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":[".pi/packages/demo-pkg"]}"#,
        );
        std::fs::create_dir_all(project.join(".pi/bin")).unwrap();
        std::os::unix::fs::symlink(
            "../packages/demo-pkg/bin/other.js",
            project.join(".pi/bin/demo-tool"),
        )
        .unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("does not point at the target declared"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_refuses_when_a_shared_config_file_already_carried_consumer_edits() {
    let project = tmpdir("stage-shared-config-dirty");
    let source = tmpdir("stage-shared-config-dirty-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // An unrelated consumer edit sitting in the shared file before
        // propagation runs. `git add -A` would sweep it into the automated PR.
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"consumerSecret":"do-not-publish","hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );

        let dirty = dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap()).unwrap();
        assert_eq!(dirty, vec![PathBuf::from(".claude/settings.json")]);
        let err = refuse_pre_existing_shared_config_edits(&dirty)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains(".claude/settings.json"), "{err}");

        // A clean tree is not refused.
        git(&project, &["checkout", "--", ".claude/settings.json"]);
        assert!(
            dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap())
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn refreshed_staging_does_not_read_pi_sources_or_committed_manifests() {
    let project = tmpdir("stage-pi-refreshed-no-source-scan");
    let source = tmpdir("stage-pi-refreshed-no-source-scan-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        // The committed manifest declares an escaping path, so any attempt to
        // derive ownership from it errors.
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":["../escape.js"]}}"#,
        );
        write_file(
            &project.join(".pi/packages/demo-pkg/lib/helper.ts"),
            "old\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
        );
        std::fs::remove_file(project.join(".pi/packages/demo-pkg/lib/helper.ts")).unwrap();

        // After a refresh the tracked set alone decides ownership, so the
        // committed manifest is never parsed and staging succeeds.
        stage_project_paths_after_refresh(&[]).unwrap();
        let staged = git_output(&project, &["diff", "--cached", "--name-status"]);
        assert!(
            staged.contains("D\t.pi/packages/demo-pkg/lib/helper.ts"),
            "{staged}"
        );

        // Without a refresh the same tree still consults it, and reports why.
        let err = stage_project_paths(&[]).unwrap_err().to_string();
        assert!(err.contains("unsafe committed Pi package path"), "{err}");
    });
}

/// Baseline for the Pi auxiliary checks: a package whose settings registration,
/// source index, append-system block, and bin link are all valid.
fn write_valid_pi_install(project: &Path, source: &Path) {
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[],"appendSystem":"APPEND.md"}}"#,
    );
    write_file(
        &source.join("pi-extensions/demo-pkg/APPEND.md"),
        "house rule\n",
    );
    write_file(
        &project.join(".pi/packages/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[],"appendSystem":"APPEND.md"}}"#,
    );
    write_file(
        &project.join(".pi/packages/demo-pkg/APPEND.md"),
        "house rule\n",
    );
    write_file(
        &project.join(".pi/settings.json"),
        r#"{"packages":["./packages/demo-pkg"]}"#,
    );
    write_file(
        &project.join(".pi/.vstack-source.json"),
        r#"{"demo-pkg":{"sourcePath":"/somewhere"}}"#,
    );
    write_file(
        &project.join(".pi/APPEND_SYSTEM.md"),
        "<!-- vstack:append-system demo-pkg begin -->\nhouse rule\n<!-- vstack:append-system demo-pkg end -->\n",
    );
}

#[test]
fn stage_mode_verifies_the_pi_append_system_block_and_source_index() {
    let project = tmpdir("stage-pi-append-block-and-index");
    let source = tmpdir("stage-pi-append-block-and-index-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_valid_pi_install(&project, &source);

    crate::test_util::with_project_root(&project, || {
        let mut entry = pi_entry("demo-pkg", &source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Control: the valid install passes auxiliary verification.
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(failures.is_empty(), "{failures:?}");

        // The managed block was edited away from the package content.
        write_file(
            &project.join(".pi/APPEND_SYSTEM.md"),
            "<!-- vstack:append-system demo-pkg begin -->\ntampered\n<!-- vstack:append-system demo-pkg end -->\n",
        );
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not match the package content")),
            "{failures:?}"
        );

        // The block is gone entirely.
        write_file(&project.join(".pi/APPEND_SYSTEM.md"), "consumer prose\n");
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures.iter().any(|f| f.contains("missing the block")),
            "{failures:?}"
        );

        // The package stopped declaring appendSystem but its block is still
        // installed: the prompt disagrees with the package.
        write_valid_pi_install(&project, &source);
        write_file(
            &project.join(".pi/packages/demo-pkg/package.json"),
            r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
        );
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures.iter().any(|f| f.contains("still carries a block")),
            "{failures:?}"
        );

        // The source-index record carries no locator at all.
        write_valid_pi_install(&project, &source);
        write_file(
            &project.join(".pi/.vstack-source.json"),
            r#"{"demo-pkg":{}}"#,
        );
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures
                .iter()
                .any(|f| f.contains("records no source repo or path")),
            "{failures:?}"
        );

        // The source-index sidecar lost its entry.
        write_file(&project.join(".pi/.vstack-source.json"), r#"{"other":{}}"#);
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".pi/.vstack-source.json missing the entry")),
            "{failures:?}"
        );

        // ...and is malformed.
        write_file(&project.join(".pi/.vstack-source.json"), "{ not json");
        let mut failures = Vec::new();
        verify_pi_auxiliary_install("demo-pkg", &mut failures).unwrap();
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".pi/.vstack-source.json")),
            "{failures:?}"
        );
    });
}

#[test]
fn stage_mode_requires_the_opencode_instruction_to_be_registered() {
    let project = tmpdir("stage-opencode-instruction-registration");
    let source = tmpdir("stage-opencode-instruction-registration-source");
    std::fs::create_dir_all(&project).unwrap();
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["opencode"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let instruction = crate::installer::opencode_hook_instruction_path(false, "guard");
        write_file(
            &instruction,
            &crate::installer::opencode_hook_instruction_contents(&registration.hook),
        );
        let expected = crate::installer::opencode_hook_instruction_ref(false, "guard");

        // Registered: passes.
        write_file(
            &project.join("opencode.json"),
            &format!(r#"{{"instructions":["{expected}"]}}"#),
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        // Present but no longer referenced: OpenCode never loads it.
        write_file(&project.join("opencode.json"), r#"{"instructions":[]}"#);
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("missing instructions entry")),
            "{failures:?}"
        );

        // A registration that lives only in the inactive spelling is not
        // loaded: `opencode.json` wins when both exist.
        write_file(
            &project.join("opencode.jsonc"),
            &format!(r#"{{"instructions":["{expected}"]}}"#),
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("opencode.json missing instructions entry")),
            "{failures:?}"
        );

        // With only the .jsonc spelling present it becomes the active config.
        std::fs::remove_file(project.join("opencode.json")).unwrap();
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        // A locally replaced instruction body is not stageable, registered or not.
        write_file(&instruction, "# Safety: guard\n\ndisabled\n");
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not match the locked OpenCode hook guard content")),
            "{failures:?}"
        );

        // No config at all.
        write_file(
            &instruction,
            &crate::installer::opencode_hook_instruction_contents(&registration.hook),
        );
        std::fs::remove_file(project.join("opencode.jsonc")).unwrap();
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures.iter().any(|f| f.contains("missing registration")),
            "{failures:?}"
        );
    });
}

#[test]
fn stage_mode_rejects_a_locally_replaced_cursor_safety_rule() {
    let project = tmpdir("stage-cursor-rule-replaced");
    let source = tmpdir("stage-cursor-rule-replaced-source");
    std::fs::create_dir_all(&project).unwrap();
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["cursor"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let rule = crate::installer::cursor_hook_rule_path(false, "guard");
        write_file(
            &rule,
            &crate::installer::cursor_hook_rule_contents(&registration.hook),
        );

        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Cursor,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        write_file(
            &rule,
            "---\ndescription: \"Safety: guard\"\n---\n\ndisabled\n",
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Cursor,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not match the locked Cursor hook guard content")),
            "{failures:?}"
        );
    });
}

#[test]
#[cfg(unix)]
fn stage_mode_refuses_an_untracked_shared_config_and_guards_the_no_drift_path() {
    let project = tmpdir("stage-untracked-shared-config");
    let source = tmpdir("stage-untracked-shared-config-source");
    std::fs::create_dir_all(&project).unwrap();
    write_file(
        &source.join("pi-extensions/demo-pkg/package.json"),
        r#"{"name":"demo-pkg","pi":{"extensions":[]}}"#,
    );
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        // A locked Pi package is what makes `.pi/settings.json` a file staging
        // would touch; without one it is purely the consumer's and staging
        // never reaches it.
        let mut lock = LockFile::default();
        lock.add(pi_entry("demo-pkg", &source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(&project.join("README.md"), "hi\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        assert!(
            dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap())
                .unwrap()
                .is_empty()
        );

        // A shared config the consumer wrote but never committed is still theirs.
        write_file(
            &project.join(".pi/settings.json"),
            r#"{"consumerSecret":"do-not-publish"}"#,
        );
        let dirty = dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap()).unwrap();
        assert_eq!(dirty, vec![PathBuf::from(".pi/settings.json")]);
        assert!(refuse_pre_existing_shared_config_edits(&dirty).is_err());
    });
}

#[test]
fn no_drift_stage_path_refuses_pre_existing_shared_config_edits() {
    let project = tmpdir("stage-no-drift-shared-config-guard");
    let source = tmpdir("stage-no-drift-shared-config-guard-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        let command = crate::installer::claude_project_hook_command("guard");
        write_file(
            &project.join(".claude/settings.json"),
            &serde_json::json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": command }]
                    }]
                }
            })
            .to_string(),
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Source hashes match, so this takes the no-drift staging branch — the
        // one a scheduled propagation hits most often.
        write_file(
            &project.join(".claude/settings.json"),
            &serde_json::json!({
                "consumerSecret": "do-not-publish",
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": command }]
                    }]
                }
            })
            .to_string(),
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains(".claude/settings.json"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_refuses_pre_existing_edits_to_project_owned_skills() {
    let project = tmpdir("stage-project-owned-skill-dirty");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        // Not in the lock: a project-owned skill, whose file vstack only owns a
        // marker-delimited block inside.
        write_file(
            &project.join(".agents/skills/house-rules/SKILL.md"),
            "---\nname: house-rules\ndescription: House rules\n---\n\nconsumer prose\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        assert!(
            dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap())
                .unwrap()
                .is_empty()
        );

        write_file(
            &project.join(".agents/skills/house-rules/SKILL.md"),
            "---\nname: house-rules\ndescription: House rules\n---\n\nconsumer prose, revised\n",
        );
        let dirty = dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap()).unwrap();
        assert_eq!(
            dirty,
            vec![PathBuf::from(".agents/skills/house-rules/SKILL.md")]
        );
        assert!(refuse_pre_existing_shared_config_edits(&dirty).is_err());
    });
}

#[test]
fn stage_mode_rejects_a_locally_edited_managed_hook_script() {
    let project = tmpdir("stage-edited-hook-script");
    let source = tmpdir("stage-edited-hook-script-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        let command = crate::installer::claude_project_hook_command("guard");
        write_file(
            &project.join(".claude/settings.json"),
            &serde_json::json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": command }]
                    }]
                }
            })
            .to_string(),
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Control: the untouched install stages fine.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["reset"]);

        // The body is neutered while the source and lock hash are unchanged and
        // the registration still points at it.
        write_file(
            &project.join(".claude/hooks/guard.sh"),
            &hook_script_contents("guard").replace("exit 0", "exit 0 # disabled"),
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains("does not match the locked script"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // Identical text with the execute bit cleared is equally broken.
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                project.join(".claude/hooks/guard.sh"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("is not executable"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_rejects_a_hook_entry_with_no_harnesses() {
    let project = tmpdir("stage-hook-no-harnesses");
    let source = tmpdir("stage-hook-no-harnesses-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &[]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Nothing was checked, because there is nothing recorded to check.
        let err = verify_project_auxiliary_installs_before_stage(&[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("records no harnesses"), "{err}");
    });
}

#[test]
fn drift_stage_path_refuses_shared_config_edits_before_refresh_rewrites_them() {
    let project = tmpdir("stage-drift-shared-config-guard");
    let source = tmpdir("stage-drift-shared-config-guard-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        // No recorded hash: the entry reads as drifted, so this takes the
        // refresh branch rather than the no-drift one.
        lock.add(hook_entry("guard", &source, &["claude-code"]));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(&project.join(".claude/settings.json"), r#"{"hooks":{}}"#);
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        let consumer = r#"{"consumerSecret":"do-not-publish","hooks":{}}"#;
        write_file(&project.join(".claude/settings.json"), consumer);

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains(".claude/settings.json"), "{err}");
        // The refusal must land before refresh rewrote the file the consumer is
        // being told to stash.
        assert_eq!(
            std::fs::read_to_string(project.join(".claude/settings.json")).unwrap(),
            consumer,
            "the consumer edit must still be there to stash"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

#[test]
fn stage_mode_rejects_a_replaced_codex_prose_safety_block() {
    let project = tmpdir("stage-codex-prose-replaced");
    let source = tmpdir("stage-codex-prose-replaced-source");
    std::fs::create_dir_all(&project).unwrap();
    write_prose_fallback_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["codex"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let block = crate::installer::codex_hook_safety_block(&registration.hook);
        let agent = project.join(".codex/agents/rust.toml");
        let mut lock = LockFile::default();
        lock.add(lock_entry("rust", ItemKind::Agent, &["codex"]));
        lock.add(lock_entry("analyst", ItemKind::Agent, &["codex"]));
        let carriers = codex_prose_carrier_paths(&lock);
        write_file(&agent, &format!("instructions = '''\n{block}\n'''\n"));

        // No `.codex/hooks/guard.sh`: this is the prose-fallback path.
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        // The marker survives while the advisory it carries is gone.
        write_file(
            &agent,
            "instructions = '''\n## Safety: guard\n\ndisabled\n'''\n",
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not carry the locked safety prose")),
            "{failures:?}"
        );

        // Deleted outright from every agent: marker presence would have read as
        // "no prose fallback here" and passed.
        write_file(&agent, "instructions = '''\nnothing here\n'''\n");
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not carry the locked safety prose")),
            "{failures:?}"
        );

        // Prose sitting outside the instructions literal is prose Codex never
        // reads, so it must not satisfy the check.
        write_file(
            &agent,
            &format!("# {block}\\ninstructions = '''\\nnothing here\\n'''\\n"),
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("does not carry the locked safety prose")),
            "{failures:?}"
        );

        // The block deleted from one agent while a sibling still carries it.
        write_file(&agent, &format!("instructions = '''\n{block}\n'''\n"));
        let sibling = project.join(".codex/agents/analyst.toml");
        write_file(&sibling, "instructions = '''\nno safety here\n'''\n");
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("analyst.toml") && f.contains("locked safety prose")),
            "{failures:?}"
        );
    });
}

#[test]
fn stage_mode_rejects_a_native_codex_hook_downgraded_to_prose() {
    let project = tmpdir("stage-codex-native-downgraded");
    let source = tmpdir("stage-codex-native-downgraded-source");
    std::fs::create_dir_all(&project).unwrap();
    // PreToolUse maps natively, so prose is not a valid install shape for it.
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["codex"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let block = crate::installer::codex_hook_safety_block(&registration.hook);
        write_file(
            &project.join(".codex/agents/rust.toml"),
            &format!("instructions = '''\n{block}\n'''\n"),
        );

        // Script and registration removed, advisory prose left in their place.
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures.iter().any(|f| f.contains("installs natively")),
            "{failures:?}"
        );
    });
}

/// The guard queried every `SHARED_CONFIG_PATHS` entry regardless of what the
/// lock holds, so a dirty harness config propagation never touches — a
/// consumer's own `.pi/settings.json` when the lock holds only Claude assets —
/// refused `--stage`. Ownership is lock-dependent, exactly as
/// `project_stage_paths` derives it.
#[test]
fn stage_guard_ignores_a_dirty_shared_config_no_locked_asset_owns() {
    let project = tmpdir("stage-shared-config-unowned");
    let source = tmpdir("stage-shared-config-unowned-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        // A consumer's own Pi config. No locked entry is a Pi package, so
        // propagation never writes it and staging never adds it.
        write_file(&project.join(".pi/settings.json"), r#"{"packages":[]}"#);
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".pi/settings.json"),
            r#"{"packages":["../their-own-thing"]}"#,
        );

        let stage_paths = pre_refresh_project_stage_paths().unwrap();
        assert!(
            !stage_paths.contains(&PathBuf::from(".pi/settings.json")),
            "staging does not own this file: {stage_paths:?}"
        );
        assert!(
            dirty_shared_config_paths(&stage_paths).unwrap().is_empty(),
            "an unowned dirty config must not block staging"
        );

        // Control: the shared file a locked asset does own still blocks.
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"consumerSecret":"do-not-publish","hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        let stage_paths = pre_refresh_project_stage_paths().unwrap();
        assert_eq!(
            dirty_shared_config_paths(&stage_paths).unwrap(),
            vec![PathBuf::from(".claude/settings.json")]
        );
    });
}

/// The prose check swept every `.codex/agents/*.toml`, but
/// `install_hook_codex_prose` only ever writes to the agents it is handed and
/// staging owns only lock-listed agent paths. A consumer's own Codex agent
/// therefore failed `--stage` for not carrying a safety block nothing installed
/// there.
#[test]
fn stage_mode_ignores_a_codex_agent_no_lock_entry_claims() {
    let project = tmpdir("stage-codex-prose-unmanaged");
    let source = tmpdir("stage-codex-prose-unmanaged-source");
    std::fs::create_dir_all(&project).unwrap();
    write_prose_fallback_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["codex"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let block = crate::installer::codex_hook_safety_block(&registration.hook);
        write_file(
            &project.join(".codex/agents/rust.toml"),
            &format!("instructions = '''\n{block}\n'''\n"),
        );
        // A Codex agent the consumer wrote themselves. No lock entry names it,
        // so no vstack install ever spliced the block into it.
        write_file(
            &project.join(".codex/agents/their-own.toml"),
            "instructions = '''\ntheir own agent\n'''\n",
        );

        let mut lock = LockFile::default();
        lock.add(lock_entry("rust", ItemKind::Agent, &["codex"]));
        let carriers = codex_prose_carrier_paths(&lock);

        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        // Control: the same file, once the lock records it as a Codex agent,
        // must carry the block.
        lock.add(lock_entry("their-own", ItemKind::Agent, &["codex"]));
        let carriers = codex_prose_carrier_paths(&lock);
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("their-own.toml") && f.contains("locked safety prose")),
            "{failures:?}"
        );
    });
}

/// `git status -z` frames a rename as destination-then-source, and the guard
/// read only the destination. A rename between two owned shared paths therefore
/// named the destination while staying silent about the original file whose
/// deletion `git add -A` would stage — the operator is pointed at the wrong
/// file. Renaming an owned path to an *unowned* one cannot slip past at all:
/// rename detection needs both endpoints inside the pathspec, so the scoped
/// query reports the plain deletion of the owned source (pinned below).
#[test]
fn stage_guard_reports_both_sides_of_a_rename_between_owned_shared_configs() {
    let project = tmpdir("stage-shared-config-renamed");
    let source = tmpdir("stage-shared-config-renamed-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // Control: with no rename in the tree the guard reads clean, so a
        // non-empty result below is the rename and not fixture noise.
        let stage_paths = pre_refresh_project_stage_paths().unwrap();
        for owned in [".claude/settings.json", "vstack.settings.toml"] {
            assert!(
                stage_paths.contains(&PathBuf::from(owned)),
                "the guard must own both endpoints of the probed rename: {stage_paths:?}"
            );
        }
        assert!(
            dirty_shared_config_paths(&stage_paths).unwrap().is_empty(),
            "clean control"
        );

        // Both endpoints are owned, so rename detection pairs them and git
        // emits the two-record rename form.
        git(
            &project,
            &["mv", ".claude/settings.json", "vstack.settings.toml"],
        );
        let dirty = dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap()).unwrap();
        assert_eq!(
            dirty,
            vec![
                PathBuf::from(".claude/settings.json"),
                PathBuf::from("vstack.settings.toml"),
            ],
            "the renamed-away original is the file at risk and must be named"
        );
    });
}

/// The rename form only appears when both endpoints match the scoped
/// pathspecs; a rename to an unowned path leaves the owned source as a plain
/// deletion, which the guard already refuses. Pinned so a future widening of
/// the pathspecs (a directory rather than an exact file) cannot quietly turn
/// this into a way around the guard.
#[test]
fn stage_guard_refuses_a_shared_config_renamed_to_an_unowned_path() {
    let project = tmpdir("stage-shared-config-renamed-away");
    let source = tmpdir("stage-shared-config-renamed-away-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let mut entry = hook_entry("guard", &source, &["claude-code"]);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_hook_script(&project.join(".claude/hooks/guard.sh"), "guard");
        write_file(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""}]}]}}"#,
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);
        assert!(
            dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap())
                .unwrap()
                .is_empty(),
            "clean control"
        );

        git(
            &project,
            &["mv", ".claude/settings.json", "consumer-settings.json"],
        );
        let dirty = dirty_shared_config_paths(&pre_refresh_project_stage_paths().unwrap()).unwrap();
        assert_eq!(dirty, vec![PathBuf::from(".claude/settings.json")]);
        assert!(
            refuse_pre_existing_shared_config_edits(&dirty)
                .unwrap_err()
                .to_string()
                .contains(".claude/settings.json")
        );
    });
}

/// Refresh creates and retargets `.agents/skills/<name>` as a link into
/// `project-skills-dir`. That link is a vstack-managed artifact like every
/// other managed link staging records, so a commit that omits it leaves the
/// relocated skill unwired for anyone who checks the tree out.
#[cfg(unix)]
#[test]
fn staging_records_the_managed_relocated_project_skill_link() {
    let project = tmpdir("stage-relocated-link-itself");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("project-skills/local/SKILL.md"),
            "---\nname: local\ndescription: Local skill\n---\n\n# Local\n",
        );
        std::fs::create_dir_all(project.join(".agents/skills")).unwrap();
        std::os::unix::fs::symlink(
            "../../project-skills/local",
            project.join(".agents/skills/local"),
        )
        .unwrap();

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        // Control: the target is staged, so a run that staged nothing at all
        // could not pass the assertion below.
        assert!(
            staged.contains("project-skills/local/SKILL.md\n"),
            "{staged}"
        );
        assert!(staged.contains(".agents/skills/local\n"), "{staged}");
    });
}

/// A link a consumer wrote inside `project-skills-dir` is their own file, not
/// something refresh maintains, so staging must leave it to them.
#[cfg(unix)]
#[test]
fn staging_leaves_a_consumer_written_project_skills_link_alone() {
    let project = tmpdir("stage-consumer-skill-link");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("vendor-skills/shared/SKILL.md"),
            "---\nname: shared\ndescription: Shared skill\n---\n\n# Shared\n",
        );
        std::fs::create_dir_all(project.join("project-skills")).unwrap();
        std::os::unix::fs::symlink(
            "../vendor-skills/shared",
            project.join("project-skills/shared"),
        )
        .unwrap();

        stage_project_paths(&[]).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(
            staged.contains("vendor-skills/shared/SKILL.md\n"),
            "{staged}"
        );
        assert!(!staged.contains("project-skills/shared\n"), "{staged}");
    });
}

/// A managed skill link vstack cannot resolve hides whatever it names. Treating
/// that as "nothing to stage" is the fail-open shape: the commit silently
/// misses a managed path. Only a link with no target at all is not an error —
/// there is no file behind it to omit.
#[cfg(unix)]
#[test]
fn staging_refuses_a_project_skill_link_it_cannot_resolve() {
    let project = tmpdir("stage-unresolvable-skill-link");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        std::fs::create_dir_all(project.join("project-skills")).unwrap();

        // Control: a link with no target names no file, and staging carries on.
        std::os::unix::fs::symlink("nowhere", project.join("project-skills/absent")).unwrap();
        stage_project_paths(&[]).unwrap();

        // A self-referential link is a resolution FAILURE, not an absence.
        std::os::unix::fs::symlink("loop", project.join("project-skills/loop")).unwrap();
        let err = stage_project_paths(&[])
            .expect_err("a link that cannot be resolved must not be passed over")
            .to_string();
        assert!(err.contains("project-skills/loop"), "{err}");
    });
}

/// A generated agent whose body or frontmatter was replaced locally still
/// exists, so `verify::run` passes it, and a no-source-drift `--stage` then
/// commits the neutered file as though propagation produced it.
#[test]
fn stage_mode_refuses_a_generated_agent_that_no_longer_matches_its_source() {
    let project = tmpdir("stage-agent-content");
    let source = tmpdir("stage-agent-content-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_agent_skill_source(&source, true);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(agent_entry("rust", &source));
        lock.add(demo_entry(&source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "# Demo\n");
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "../../.agents/skills/demo",
            project.join(".claude/skills/demo"),
        )
        .unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // The drift branch: refresh generates the agent, then staging records it.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "propagated"]);

        let agent_path = project.join(".claude/agents/rust.md");
        let generated = std::fs::read_to_string(&agent_path).unwrap();
        assert!(
            generated.contains("skills: demo"),
            "fixture did not generate a skill requirement: {generated}"
        );

        // Control: the untouched install passes the no-drift staging path.
        run(ScopeFilter::Project, false, false, true, true).unwrap();

        // The declared skill requirement is stripped while the source, the lock
        // hash and the file's existence are all unchanged.
        write_file(&agent_path, &generated.replace("skills: demo\n", ""));
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".claude/agents/rust.md"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

/// The pre-refresh guard exists for files whose content is partly the
/// consumer's. A managed skill link holds none of it, and refresh creates it
/// untracked — counting it as a consumer edit refused staging over vstack's
/// own work.
#[cfg(unix)]
#[test]
fn shared_config_guard_ignores_the_managed_skill_link_but_not_the_skill() {
    let project = tmpdir("guard-managed-skill-link");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        LockFile::default()
            .save(&config::lock_file_path(false))
            .unwrap();
        write_file(
            &project.join("vstack.toml"),
            "project-skills-dir = \"project-skills\"\n",
        );
        write_file(
            &project.join("project-skills/local/SKILL.md"),
            "---\nname: local\ndescription: Local skill\n---\n\n# Local\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // What refresh does: link the tracked skill into `.agents/skills`.
        std::fs::create_dir_all(project.join(".agents/skills")).unwrap();
        std::os::unix::fs::symlink(
            "../../project-skills/local",
            project.join(".agents/skills/local"),
        )
        .unwrap();

        let stage_paths = pre_refresh_project_stage_paths().unwrap();
        assert!(
            stage_paths.contains(&PathBuf::from(".agents/skills/local")),
            "the link must be in the staged set for this test to constrain the guard: {stage_paths:?}"
        );
        assert!(
            dirty_shared_config_paths(&stage_paths).unwrap().is_empty(),
            "a link refresh just created is not a consumer edit"
        );

        // Control: the consumer's own edit to the skill is still refused.
        write_file(
            &project.join("project-skills/local/SKILL.md"),
            "---\nname: local\ndescription: Local skill\n---\n\n# Local\n\nMine.\n",
        );
        assert_eq!(
            dirty_shared_config_paths(&stage_paths).unwrap(),
            vec![PathBuf::from("project-skills/local/SKILL.md")]
        );
    });
}

/// Every generated agent body points at `.agents/skill-failure-reporting.md`.
/// Nothing regenerates it when there is no source drift, and staging owns the
/// path, so a locally replaced or deleted copy is what the propagation commit
/// records — with every agent still pointing at it.
#[test]
fn stage_mode_refuses_a_replaced_failure_reporting_reference() {
    let project = tmpdir("stage-failure-reference");
    let source = tmpdir("stage-failure-reference-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_agent_skill_source(&source, true);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(agent_entry("rust", &source));
        lock.add(demo_entry(&source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "# Demo\n");
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "../../.agents/skills/demo",
            project.join(".claude/skills/demo"),
        )
        .unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "propagated"]);

        let reference = project.join(".agents/skill-failure-reporting.md");
        assert!(
            std::fs::read_to_string(&reference).unwrap() == crate::agent::FAILURE_REPORTING_DOC,
            "fixture did not install the reference agents point at"
        );

        // Control: the untouched install passes the no-drift staging path.
        run(ScopeFilter::Project, false, false, true, true).unwrap();

        // Replaced in place, tracked and committed, so nothing reads as dirty
        // and no source hash moved.
        write_file(&reference, "# gutted\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "reference replaced"]);

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains("skill-failure-reporting.md"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // Deleted is the same shortfall: the agents point at a missing file.
        std::fs::remove_file(&reference).unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "reference deleted"]);
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains("skill-failure-reporting.md"), "{err}");
    });
}

/// Refresh lifts a generated agent's launch/additional-instructions section
/// into `vstack.toml` whenever the config does not already record it. That is
/// the migration path for an edit the consumer made and committed on purpose —
/// but an *uncommitted* one would be persisted, regenerated back into the
/// agent, and then pass the post-refresh content check, because the config it
/// is checked against is the one the edit just wrote. Both files would ride
/// into the automated propagation commit as vstack's own work.
#[cfg(unix)]
#[test]
fn drift_stage_path_refuses_uncommitted_agent_edits_refresh_would_absorb() {
    let project = tmpdir("stage-agent-extract-guard");
    let source = tmpdir("stage-agent-extract-guard-source");
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);
    write_agent_skill_source(&source, true);

    let redrift = |body: &str| {
        std::fs::write(
            source.join("agents/rust.md"),
            format!("---\nname: rust\ndescription: Rust\nmodel: sonnet\nrole: engineer\n---\n\n# Rust\n\n{body}\n"),
        )
        .unwrap();
    };

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(agent_entry("rust", &source));
        lock.add(demo_entry(&source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "# Demo\n");
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        std::os::unix::fs::symlink(
            "../../.agents/skills/demo",
            project.join(".claude/skills/demo"),
        )
        .unwrap();
        // A consumer config that records the agent somewhere other than the
        // instruction tables. `ensure_project_config` seeds a placeholder key
        // only for an agent the file does not mention at all, so this is the
        // ordinary state in which `[agent-additional-instructions]` has no
        // entry for `rust` and refresh's extraction is live.
        write_file(
            &project.join("vstack.toml"),
            "[agent-frontmatter.claude-code]\nrust = { model = \"sonnet\" }\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // The drift branch: refresh generates the agent, then staging records it.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "propagated"]);

        let agent_path = project.join(".claude/agents/rust.md");
        let generated = std::fs::read_to_string(&agent_path).unwrap();
        assert!(
            !generated.contains("## Additional Instructions"),
            "the fixture already carries the section it is about to smuggle: {generated}"
        );

        // Control: an agent dirtied in a way refresh simply overwrites carries
        // nothing into the config, and must still propagate. Without this the
        // guard below could be refusing on dirt alone.
        write_file(&agent_path, &format!("{generated}\nLOCAL SCRATCH\n"));
        redrift("Revision two.");
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "propagated again"]);
        assert!(
            !std::fs::read_to_string(&agent_path)
                .unwrap()
                .contains("LOCAL SCRATCH"),
            "refresh must have overwritten the scratch edit"
        );

        // The same file, edited where refresh extracts from instead.
        let regenerated = std::fs::read_to_string(&agent_path).unwrap();
        write_file(
            &agent_path,
            &format!("{regenerated}\n## Additional Instructions\n\nSMUGGLED\n"),
        );
        redrift("Revision three.");

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains(".claude/agents/rust.md"), "{err}");
        assert!(err.contains("agent-additional-instructions"), "{err}");
        // The refusal must land before refresh rewrote the file the consumer is
        // being told to stash, and before the config absorbed the edit.
        assert!(
            std::fs::read_to_string(&agent_path)
                .unwrap()
                .contains("SMUGGLED"),
            "the consumer edit must still be there to stash"
        );
        assert!(
            !std::fs::read_to_string(project.join("vstack.toml"))
                .unwrap_or_default()
                .contains("SMUGGLED"),
            "vstack.toml must not have absorbed the edit"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );

        // The no-drift branch runs no refresh, so nothing extracts there — and
        // the same edit is caught by the pre-stage content check instead, with
        // its own cause. Undoing the drift leaves the edit in place.
        redrift("Revision two.");
        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(err.contains(".claude/agents/rust.md"), "{err}");
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

/// `git add` records a symlink as the link, not as the bytes it resolves to. A
/// managed agent replaced by a link into an external file holding exactly the
/// bytes the source generates therefore passed every content check, and the
/// propagation commit carried the link — leaving other checkouts with an agent
/// that is dangling, nonportable, or mutable from outside the repository.
#[cfg(unix)]
#[test]
fn stage_mode_refuses_a_generated_agent_replaced_by_a_symlink() {
    let project = tmpdir("stage-agent-symlink");
    let source = tmpdir("stage-agent-symlink-source");
    let outside = tmpdir("stage-agent-symlink-outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    init_git_project(&project);
    write_agent_skill_source(&source, true);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(agent_entry("rust", &source));
        lock.add(demo_entry(&source));
        lock.save(&config::lock_file_path(false)).unwrap();
        write_installed_skill_md(&project.join(".agents/skills/demo/SKILL.md"), "# Demo\n");
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        std::os::unix::fs::symlink(
            "../../.agents/skills/demo",
            project.join(".claude/skills/demo"),
        )
        .unwrap();
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        // The drift branch: refresh generates the agent, then staging records it.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["commit", "-m", "propagated"]);

        let agent_path = project.join(".claude/agents/rust.md");
        let generated = std::fs::read_to_string(&agent_path).unwrap();

        // Control: the untouched install passes the no-drift staging path.
        run(ScopeFilter::Project, false, false, true, true).unwrap();
        git(&project, &["reset"]);

        // Same bytes, but they live outside the checkout and outside vstack's
        // control. The source, the lock hash, and the resolved content are all
        // unchanged.
        let external = outside.join("rust.md");
        std::fs::write(&external, &generated).unwrap();
        std::fs::remove_file(&agent_path).unwrap();
        std::os::unix::fs::symlink(&external, &agent_path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&agent_path).unwrap(),
            generated,
            "the fixture must resolve to the generated bytes for this test to constrain anything"
        );

        let err = run(ScopeFilter::Project, false, false, true, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("auxiliary verification failed"), "{err}");
        assert!(
            err.contains(".claude/agents/rust.md is not a regular file"),
            "{err}"
        );
        assert!(
            git_output(&project, &["diff", "--cached", "--name-only"]).is_empty(),
            "nothing may be staged"
        );
    });
}

/// The same hazard for a native hook script: an executable link whose target
/// carries the locked script passed both the byte comparison and the execute
/// bit check, because each followed the link. A dangling link was worse still —
/// the `exists()` gate reported nothing to check while staging, which stats the
/// path without following it, recorded the broken link.
#[cfg(unix)]
#[test]
fn stage_mode_refuses_a_hook_script_replaced_by_a_symlink() {
    let project = tmpdir("stage-hook-script-symlink");
    let source = tmpdir("stage-hook-script-symlink-source");
    let outside = tmpdir("stage-hook-script-symlink-outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["claude-code"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let script = project.join(".claude/hooks/guard.sh");
        write_installed_hook_script(&script, "guard");
        let command = crate::installer::claude_project_hook_command("guard");
        write_file(
            &project.join(".claude/settings.json"),
            &serde_json::json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": command }]
                    }]
                }
            })
            .to_string(),
        );

        // Control: the real file passes.
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::ClaudeCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        // An executable link whose target is byte-identical to the locked
        // script: every check that follows the link passes.
        let external = outside.join("guard.sh");
        write_installed_hook_script(&external, "guard");
        std::fs::remove_file(&script).unwrap();
        std::os::unix::fs::symlink(&external, &script).unwrap();
        assert_eq!(
            std::fs::read_to_string(&script).unwrap(),
            hook_script_contents("guard"),
            "the fixture must resolve to the locked script for this test to constrain anything"
        );
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::ClaudeCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".claude/hooks/guard.sh is not a regular file")),
            "{failures:?}"
        );

        // Dangling: nothing to follow, and staging records the broken link.
        std::fs::remove_file(&external).unwrap();
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::ClaudeCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".claude/hooks/guard.sh is not a regular file")),
            "{failures:?}"
        );
    });
}

/// A Codex agent carrying the prose fallback is an installed artifact too, and
/// staging owns its path: a link whose target carries the safety block passed,
/// and a dangling one was filtered out of the carrier set entirely.
#[cfg(unix)]
#[test]
fn stage_mode_refuses_a_codex_prose_carrier_replaced_by_a_symlink() {
    let project = tmpdir("stage-codex-prose-symlink");
    let source = tmpdir("stage-codex-prose-symlink-source");
    let outside = tmpdir("stage-codex-prose-symlink-outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    write_prose_fallback_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["codex"]);
        let registration = locked_hook_registration(&entry).unwrap();
        let block = crate::installer::codex_hook_safety_block(&registration.hook);
        let agent = project.join(".codex/agents/rust.toml");
        let mut lock = LockFile::default();
        lock.add(lock_entry("rust", ItemKind::Agent, &["codex"]));
        let carriers = codex_prose_carrier_paths(&lock);
        let body = format!("instructions = '''\n{block}\n'''\n");
        write_file(&agent, &body);

        // Control: the real file passes.
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(failures.is_empty(), "{failures:?}");

        let external = outside.join("rust.toml");
        std::fs::write(&external, &body).unwrap();
        std::fs::remove_file(&agent).unwrap();
        std::os::unix::fs::symlink(&external, &agent).unwrap();
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".codex/agents/rust.toml is not a regular file")),
            "{failures:?}"
        );

        // Dangling: the carrier filter used to drop it, and an empty carrier
        // set reads as "no installed agent to hold to the block".
        std::fs::remove_file(&external).unwrap();
        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Codex,
            Some(&registration),
            &carriers,
            &mut failures,
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains(".codex/agents/rust.toml is not a regular file")),
            "{failures:?}"
        );
    });
}

/// Translated safety artifacts are rendered from the hook and fully
/// installer-owned, so the same rule applies to them.
#[cfg(unix)]
#[test]
fn stage_mode_refuses_translated_hook_artifacts_replaced_by_symlinks() {
    let project = tmpdir("stage-translated-hook-symlink");
    let source = tmpdir("stage-translated-hook-symlink-source");
    let outside = tmpdir("stage-translated-hook-symlink-outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    write_hook_source(&source, "guard");

    crate::test_util::with_project_root(&project, || {
        let entry = hook_entry("guard", &source, &["opencode", "cursor"]);
        let registration = locked_hook_registration(&entry).unwrap();

        let instruction = crate::installer::opencode_hook_instruction_path(false, "guard");
        let instruction_body =
            crate::installer::opencode_hook_instruction_contents(&registration.hook);
        write_file(&instruction, &instruction_body);
        let expected = crate::installer::opencode_hook_instruction_ref(false, "guard");
        write_file(
            &project.join("opencode.json"),
            &format!(r#"{{"instructions":["{expected}"]}}"#),
        );
        let rule = crate::installer::cursor_hook_rule_path(false, "guard");
        let rule_body = crate::installer::cursor_hook_rule_contents(&registration.hook);
        write_file(&rule, &rule_body);

        // Control: the real files pass.
        for harness in [Harness::OpenCode, Harness::Cursor] {
            let mut failures = Vec::new();
            verify_hook_auxiliary_install(
                "guard",
                harness,
                Some(&registration),
                &[],
                &mut failures,
            );
            assert!(failures.is_empty(), "{harness:?}: {failures:?}");
        }

        let external_instruction = outside.join("opencode-guard.md");
        std::fs::write(&external_instruction, &instruction_body).unwrap();
        std::fs::remove_file(&instruction).unwrap();
        std::os::unix::fs::symlink(&external_instruction, &instruction).unwrap();
        let external_rule = outside.join("cursor-guard.mdc");
        std::fs::write(&external_rule, &rule_body).unwrap();
        std::fs::remove_file(&rule).unwrap();
        std::os::unix::fs::symlink(&external_rule, &rule).unwrap();

        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::OpenCode,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures.iter().any(|f| f.contains("is not a regular file")),
            "{failures:?}"
        );

        let mut failures = Vec::new();
        verify_hook_auxiliary_install(
            "guard",
            Harness::Cursor,
            Some(&registration),
            &[],
            &mut failures,
        );
        assert!(
            failures.iter().any(|f| f.contains("is not a regular file")),
            "{failures:?}"
        );
    });
}
