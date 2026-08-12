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
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("worker", ItemKind::Agent, &["opencode"]));
        lock.add(lock_entry("protect", ItemKind::Hook, &["opencode"]));
        lock.add(lock_entry("@scope/pkg", ItemKind::PiExtension, &["pi"]));
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
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("ignored-pkg", ItemKind::PiExtension, &["pi"]));
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
    std::fs::create_dir_all(&project).unwrap();
    init_git_project(&project);

    crate::test_util::with_project_root(&project, || {
        let mut lock = LockFile::default();
        lock.add(lock_entry("dep-pkg", ItemKind::PiExtension, &["pi"]));
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
