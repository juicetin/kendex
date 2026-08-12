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

fn write_skill_source(root: &Path, body: &str) {
    let skill_dir = root.join("skills").join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n{body}"),
    )
    .unwrap();
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

fn write_hook_source(root: &Path, name: &str) {
    write_file(
        &root.join("hooks").join(format!("{name}.sh")),
        &format!(
            "# ---\n# name: {name}\n# event: PreToolUse\n# matcher: Bash\n# description: Guard shell commands\n# safety: Keep shell commands safe\n# ---\n#!/bin/sh\nexit 0\n"
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
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
        );
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
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(&project.join("vstack.toml"), "[agent-skills]\n");
        write_file(
            &project.join(".pi/packages/manual/package.json"),
            r#"{"name":"manual","pi":{"extensions":[]}}"#,
        );

        run(ScopeFilter::Project, false, false, true, true).unwrap();

        let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
        assert!(staged.contains("vstack.toml\n"), "{staged}");
        assert!(
            !staged.contains(".pi/packages/manual/package.json"),
            "{staged}"
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
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(&project.join(".agents/skills/demo/owned.txt"), "owned\n");
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(&project.join(".claude/skills/demo/owned.txt"), "owned\n");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nmanaged edit\n",
        );
        write_file(
            &project.join(".agents/skills/demo/new-upstream.md"),
            "new upstream\n",
        );
        std::fs::remove_file(project.join(".claude/skills/demo/owned.txt")).unwrap();
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
            staged.contains("D\t.claude/skills/demo/owned.txt"),
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
        let mut entry = demo_entry(&source);
        entry.source_hash = config::compute_source_hash(&entry);
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(
            &project.join(".agents/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(
            &project.join(".claude/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-m", "baseline"]);

        write_skill_source(&source, "v2\n");
        let mut lock = LockFile::load(&config::lock_file_path(false)).unwrap();
        let entry = lock.entries.get_mut("demo").unwrap();
        entry.source_hash = config::compute_source_hash(entry);
        lock.save(&config::lock_file_path(false)).unwrap();
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv2\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv2\n",
        );
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
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(
            &project.join(".agents/skills/demo/removed-upstream.md"),
            "removed\n",
        );
        write_file(
            &project.join(".agents/skills/demo/.vstack-refreshed"),
            "1\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
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

        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv2\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv2\n",
        );
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
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
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
fn hook_script_wire_path_matches_json_registration_slashes() {
    let registration = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{
                    "type": "command",
                    "command": "bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\""
                }]
            }]
        }
    });
    let windows_spelling = Path::new(r".claude\hooks\guard.sh");
    let raw_fragment = windows_spelling.to_string_lossy();
    assert!(
        !hook_command_registered(&registration, &raw_fragment, Some("PreToolUse")),
        "negative control: backslash spelling must not match slash JSON"
    );
    assert!(
        hook_command_registered(
            &registration,
            &hook_script_wire_path(windows_spelling),
            Some("PreToolUse")
        ),
        "wire-normalized hook path should match harness registration JSON"
    );
    assert!(
        !hook_command_registered(
            &registration,
            &hook_script_wire_path(windows_spelling),
            Some("Stop")
        ),
        "a registration under another event is not a registration for this one"
    );
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
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
        write_file(
            &project.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
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
        write_file(
            &project.join(".claude/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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
        write_file(
            &project.join(".codex/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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
        write_file(
            &project.join(".agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\nv1\n",
        );
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
        write_file(
            &project.join(".codex/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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

#[test]
fn pre_refresh_stage_paths_tolerate_a_remote_pi_source_with_no_cache_yet() {
    let project = tmpdir("stage-pi-remote-source-first-run");
    std::fs::create_dir_all(&project).unwrap();

    crate::test_util::with_project_root(&project, || {
        let mut entry = lock_entry("demo-pkg", ItemKind::PiExtension, &["pi"]);
        // A remote shorthand whose cache has not been cloned yet, exactly as on
        // a clean CI runner before `detect_drift_for_scope` resolves sources.
        entry.source = "owner/repo".to_string();
        let mut lock = LockFile::default();
        lock.add(entry);
        lock.save(&config::lock_file_path(false)).unwrap();

        // Pre-refresh collection runs before resolution and must not abort.
        pre_refresh_project_stage_paths().unwrap();

        // Once sources are meant to be resolved, the same lock still fails closed.
        let err = project_stage_paths(&lock, true).unwrap_err().to_string();
        assert!(err.contains("demo-pkg"), "{err}");
        assert!(err.contains("source"), "{err}");
    });
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
        write_file(
            &project.join(".codex/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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
        write_file(
            &project.join(".claude/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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
        write_file(
            &project.join(".claude/hooks/guard.sh"),
            "#!/bin/sh\nexit 0\n",
        );
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

        let dirty = dirty_shared_config_paths().unwrap();
        assert_eq!(dirty, vec![PathBuf::from(".claude/settings.json")]);
        let err = refuse_pre_existing_shared_config_edits(&dirty)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to stage"), "{err}");
        assert!(err.contains(".claude/settings.json"), "{err}");

        // A clean tree is not refused.
        git(&project, &["checkout", "--", ".claude/settings.json"]);
        assert!(dirty_shared_config_paths().unwrap().is_empty());
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
