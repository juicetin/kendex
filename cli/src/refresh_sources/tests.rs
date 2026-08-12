use super::*;
use crate::config::{InstallMethod, LockEntry};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "vstack-refresh-source-{label}-{}-{nanos}",
                std::process::id()
            )),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn make_vstack_source(root: &Path, name: &str) -> PathBuf {
    let source = root.join(name);
    std::fs::create_dir_all(source.join("agents")).unwrap();
    std::fs::create_dir_all(source.join("skills")).unwrap();
    source
}

fn lock_entry(name: &str, source: &str) -> LockEntry {
    LockEntry {
        name: name.into(),
        kind: ItemKind::Agent,
        source: source.into(),
        source_repo: None,
        harnesses: vec!["claude-code".into()],
        method: InstallMethod::Copy,
        installed_at: "2026-07-03T00:00:00Z".into(),
        source_hash: String::new(),
    }
}

fn assert_windows_safe_path_component(component: &str) {
    let invalid = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    assert!(
        !component.chars().any(|ch| invalid.contains(&ch)),
        "test fixture path component is not Windows-safe: {component}"
    );
}

#[test]
fn resolve_single_source_accepts_absolute_vstack_source() {
    let root = TempDir::new("absolute");
    let source = root.path().join("source");
    std::fs::create_dir_all(source.join("agents")).unwrap();
    std::fs::create_dir_all(source.join("hooks")).unwrap();

    assert_eq!(
        resolve_single_source(&source.to_string_lossy()),
        Some(source.clone())
    );
    assert!(resolve_single_source(&root.path().to_string_lossy()).is_none());
}

/// `vstack add <SOURCE>` accepts any directory holding the asset, so a lock
/// entry may record one that the discovery heuristic rejects — a dot-named
/// dir, or one carrying only `skills/`. Dropping it here is what made
/// refresh fall back to the majority source and stop propagating edits.
#[test]
fn resolve_source_records_keeps_a_source_the_layout_heuristic_rejects() {
    let root = TempDir::new("recorded-alternate");
    let alternate = root.path().join(".agents");
    std::fs::create_dir_all(alternate.join("skills/demo")).unwrap();
    assert!(
        !crate::resolve::is_vstack_source(&alternate),
        "fixture must exercise the heuristic-rejected case"
    );
    assert_eq!(resolve_single_source(&alternate.to_string_lossy()), None);

    assert_eq!(
        resolve_recorded_source(&alternate.to_string_lossy()),
        Some(alternate.clone())
    );

    let mut lock = config::LockFile::default();
    lock.add(lock_entry("demo", &alternate.to_string_lossy()));
    let records = resolve_source_records(&lock);

    assert_eq!(
        records.iter().map(|r| r.root.clone()).collect::<Vec<_>>(),
        vec![alternate]
    );
}

#[test]
fn resolve_source_records_resolves_relative_sources_from_project_root() {
    let root = TempDir::new("recorded-relative");
    let project = root.path().join("project");
    let relative_source = project.join("vendor").join("vstack");
    std::fs::create_dir_all(relative_source.join("skills/demo")).unwrap();

    let mut lock = config::LockFile::default();
    lock.add(lock_entry("demo", "./vendor/vstack"));

    let records = crate::test_util::with_project_root(&project, || {
        assert_eq!(
            resolve_recorded_source("./vendor/vstack"),
            Some(std::fs::canonicalize(&relative_source).unwrap())
        );
        assert!(recorded_source_exists("./vendor/vstack"));
        resolve_source_records(&lock)
    });

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].root,
        std::fs::canonicalize(&relative_source).unwrap()
    );
    assert_eq!(records[0].aliases, vec!["./vendor/vstack".to_string()]);
}

#[test]
fn resolve_source_records_records_remote_shorthand_repo_identity() {
    let root = TempDir::new("remote-identity");
    let source = make_vstack_source(root.path(), "source");
    let mut lock = config::LockFile::default();
    lock.add(lock_entry("demo", "vanillagreencom/vstack"));

    let records = resolve_source_records_with(&lock, |source_name| {
        if source_name == "vanillagreencom/vstack" {
            Some(source.clone())
        } else {
            None
        }
    });

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].source_repo.as_deref(),
        Some("vanillagreencom/vstack")
    );
}

#[test]
fn resolve_source_records_preserves_git_ssh_repo_identity() {
    let root = TempDir::new("remote-git-ssh-identity");
    let source = make_vstack_source(root.path(), "source");
    let recorded = "git+ssh://git@github.com/VanillaGreenCom/VStack.git";
    let mut lock = config::LockFile::default();
    lock.add(lock_entry("demo", recorded));

    let records = resolve_source_records_with(&lock, |source_name| {
        if source_name == recorded {
            Some(source.clone())
        } else {
            None
        }
    });

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].source_repo.as_deref(),
        Some("vanillagreencom/vstack")
    );
}

#[test]
fn resolve_source_records_does_not_infer_identity_from_local_layout() {
    let root = TempDir::new("local-layout-identity");
    let source = make_vstack_source(root.path(), "source");
    let mut lock = config::LockFile::default();
    lock.add(lock_entry("demo", &source.to_string_lossy()));

    let records = resolve_source_records(&lock);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_repo, None);
}

#[test]
fn relative_parent_source_uses_current_worktree_lexical_neighbor() {
    let root = TempDir::new("recorded-relative-parent");
    let main_project = root.path().join("dev").join("consumer");
    let main_checkout_neighbor = root.path().join("dev").join("vstack");
    let linked_worktree = root
        .path()
        .join("dev")
        .join(".worktrees")
        .join("consumer")
        .join("issue-1");
    let worktree_neighbor = root
        .path()
        .join("dev")
        .join(".worktrees")
        .join("consumer")
        .join("vstack");
    std::fs::create_dir_all(&main_project).unwrap();
    std::fs::create_dir_all(main_checkout_neighbor.join("skills/demo")).unwrap();
    std::fs::create_dir_all(&linked_worktree).unwrap();
    std::fs::create_dir_all(worktree_neighbor.join("skills/demo")).unwrap();

    let resolved = crate::test_util::with_project_root(&linked_worktree, || {
        resolve_recorded_source("../vstack")
    });

    assert_eq!(
        resolved,
        Some(std::fs::canonicalize(&worktree_neighbor).unwrap()),
        "copied relative lock sources are resolved from the current worktree root"
    );
    assert_ne!(
        resolved,
        Some(std::fs::canonicalize(&main_checkout_neighbor).unwrap()),
        "../vstack must not silently keep pointing at the main checkout after a lock is copied"
    );
}

#[test]
fn recorded_remote_shorthand_does_not_bind_to_project_local_shadow_dir() {
    let root = TempDir::new("remote-shadow");
    let project = root.path().join("project");
    let shadow = project.join("owner").join("repo");
    std::fs::create_dir_all(&shadow).unwrap();

    crate::test_util::with_project_root(&project, || {
        assert!(resolve_recorded_local_source("owner/repo").is_none());
        assert!(!recorded_source_exists("owner/repo"));
    });
}

#[test]
fn multi_segment_remote_like_source_does_not_bind_to_project_local_shadow_dir() {
    let root = TempDir::new("remote-shadow-three-segment");
    let project = root.path().join("project");
    let shadow = project.join("owner").join("repo").join("extra");
    std::fs::create_dir_all(&shadow).unwrap();

    crate::test_util::with_project_root(&project, || {
        assert!(resolve_recorded_local_source("owner/repo/extra").is_none());
        assert!(!recorded_source_exists("owner/repo/extra"));
        assert!(resolve_source_path("owner/repo/extra").is_none());
    });
}

#[test]
fn resolve_source_path_uses_validated_legacy_remote_cache_when_canonical_is_absent() {
    let root = TempDir::new("legacy-cache-resolution");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let legacy_cache = home.join(".vstack").join("cache").join("owner_repo");
    init_git_repo(&legacy_cache);
    git(
        &legacy_cache,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/owner/repo.git",
        ],
    );

    crate::test_util::with_home_and_config(&home, &config_home, || {
        assert!(
            remote_cache_dir("owner/repo").is_some_and(|canonical| !canonical.exists()),
            "negative control: canonical hashed cache is absent"
        );
        assert_eq!(resolve_source_path("owner/repo"), Some(legacy_cache));
    });
}

#[test]
// The legacy cache spelling this fixture reproduces contains ':', which is not a
// legal Windows path component, so the directory can never exist there.
#[cfg(unix)]
fn resolve_source_path_uses_validated_git_at_legacy_remote_cache() {
    let root = TempDir::new("legacy-git-at-cache-resolution");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let source = "git@github.com:owner/repo";
    let legacy_cache = home
        .join(".vstack")
        .join("cache")
        .join("git@github.com:owner_repo");
    init_git_repo(&legacy_cache);
    git(&legacy_cache, &["remote", "add", "origin", source]);

    crate::test_util::with_home_and_config(&home, &config_home, || {
        assert!(
            remote_cache_dir(source).is_some_and(|canonical| !canonical.exists()),
            "negative control: canonical hashed cache is absent"
        );
        assert_eq!(resolve_source_path(source), Some(legacy_cache));
    });
}

#[test]
// The legacy cache spelling this fixture reproduces contains ':', which is not a
// legal Windows path component, so the directory can never exist there.
#[cfg(unix)]
fn resolve_source_path_uses_validated_non_github_legacy_remote_cache() {
    let root = TempDir::new("legacy-non-github-cache-resolution");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let source = "ssh://git@example.com/owner/repo.git";
    let legacy_cache = home
        .join(".vstack")
        .join("cache")
        .join("ssh:__git@example.com_owner_repo.git");
    init_git_repo(&legacy_cache);
    git(&legacy_cache, &["remote", "add", "origin", source]);

    crate::test_util::with_home_and_config(&home, &config_home, || {
        assert!(
            remote_cache_dir(source).is_some_and(|canonical| !canonical.exists()),
            "negative control: canonical hashed cache is absent"
        );
        assert_eq!(resolve_source_path(source), Some(legacy_cache));
    });
}

/// An entry whose own source still exists must never be silently rebound to
/// the sole other loaded source; that reinstalled it from the wrong repo.
/// The fallback stays available for a source that has genuinely gone away.
#[test]
fn refresh_source_for_entry_only_falls_back_when_the_recorded_source_is_gone() {
    let root = TempDir::new("no-rebind");
    let alternate = root.path().join(".agents");
    std::fs::create_dir_all(alternate.join("skills/demo")).unwrap();
    let only_source = make_vstack_source(root.path(), "other");
    let sources = vec![RefreshSource::from_root(&only_source)];

    let live = lock_entry("demo", &alternate.to_string_lossy());
    assert!(
        refresh_source_for_entry(&sources, &live).is_none(),
        "an entry whose recorded source exists must not bind to a different source"
    );

    let vanished = lock_entry("demo", &root.path().join("deleted-repo").to_string_lossy());
    assert_eq!(
        refresh_source_for_entry(&sources, &vanished).map(|s| s.root.clone()),
        Some(only_source),
        "legacy lock with a missing source keeps the single-source fallback"
    );
}

#[test]
fn refresh_source_for_entry_does_not_fallback_for_live_relative_source() {
    let root = TempDir::new("relative-no-rebind");
    let project = root.path().join("project");
    let relative_source = project.join("vendor").join("vstack");
    std::fs::create_dir_all(relative_source.join("skills/demo")).unwrap();
    let only_source = make_vstack_source(root.path(), "other");
    let sources = vec![RefreshSource::from_root(&only_source)];
    let live_relative = lock_entry("demo", "./vendor/vstack");

    crate::test_util::with_project_root(&project, || {
        assert!(
            refresh_source_for_entry(&sources, &live_relative).is_none(),
            "a live relative source must not rebind to the sole loaded source"
        );
    });
}

#[test]
fn resolve_source_records_calls_resolver_once_per_unique_lock_source() {
    let root = TempDir::new("resolver-count");
    let source_a = root.path().join("source-a");
    let source_b = root.path().join("source-b");
    let mut lock = config::LockFile::default();
    lock.add(lock_entry("rust", "owner/repo"));
    lock.add(LockEntry {
        name: "dev".into(),
        kind: ItemKind::Skill,
        source: "owner/repo".into(),
        source_repo: None,
        harnesses: vec!["claude-code".into()],
        method: InstallMethod::Copy,
        installed_at: "2026-07-03T00:00:00Z".into(),
        source_hash: String::new(),
    });
    lock.add(lock_entry("scout", "other/repo"));

    let counts: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    let records = resolve_source_records_with(&lock, |source| {
        *counts.borrow_mut().entry(source.to_string()).or_default() += 1;
        match source {
            "owner/repo" => Some(source_a.clone()),
            "other/repo" => Some(source_b.clone()),
            _ => None,
        }
    });

    assert_eq!(records.len(), 2);
    assert_eq!(counts.borrow().get("owner/repo"), Some(&1));
    assert_eq!(counts.borrow().get("other/repo"), Some(&1));
}

fn git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        repo.display()
    );
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap()
}

fn write_skill(root: &Path, body: &str) {
    let skill_dir = root.join("skills").join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: demo\ndescription: Demo\n---\n\n{body}"),
    )
    .unwrap();
}

fn init_git_repo(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init"]);
    git(repo, &["checkout", "-B", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "VStack Test"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    git(repo, &["config", "core.hooksPath", "/dev/null"]);
}

#[test]
fn remote_helpers_redact_urls_and_use_collision_resistant_safe_keys() {
    assert_eq!(
        remote_git_url("owner/repo").unwrap(),
        "https://github.com/owner/repo.git"
    );
    assert_eq!(
        remote_git_url("ssh://git@github.com/Owner/Repo.git").unwrap(),
        "ssh://git@github.com/Owner/Repo.git"
    );
    assert_eq!(
        remote_git_url("git+ssh://git@github.com/Owner/Repo.git").unwrap(),
        "git+ssh://git@github.com/Owner/Repo.git"
    );
    assert_eq!(
        remote_git_url_for_subprocess("git+ssh://git@github.com/Owner/Repo.git")
            .unwrap()
            .unwrap(),
        "ssh://git@github.com/Owner/Repo.git",
        "git subprocesses must not see the unsupported git+ssh scheme"
    );
    assert_ne!(
        remote_cache_key("git+ssh://git@example.com/Owner/Repo.git"),
        remote_cache_key("ssh://git@example.com/Owner/Repo.git"),
        "normalizing the Git invocation URL must not collapse canonical cache identity"
    );
    assert!(remote_git_url("http://token@example.com/Owner/Repo.git").is_none());
    assert!(looks_like_remote_source(
        "http://token@example.com/Owner/Repo.git"
    ));
    assert!(remote_git_url("../local-source").is_none());

    assert_eq!(
        remote_source_display("https://token@github.com/Owner/Repo.git"),
        "https://<redacted>@github.com/Owner/Repo.git"
    );
    assert_eq!(
        remote_source_display("https://token@example.com/Owner/Repo.git"),
        "https://<redacted>@example.com/Owner/Repo.git"
    );
    assert_eq!(
        remote_source_display("ssh://alice@example.com/Owner/Repo.git"),
        "ssh://alice@example.com/Owner/Repo.git"
    );
    assert_eq!(
        remote_source_display("ssh://alice:secret@example.com/Owner/Repo.git"),
        "ssh://alice:<redacted>@example.com/Owner/Repo.git"
    );

    let github_key = remote_cache_key("https://token@github.com/Owner/Repo.git");
    assert!(github_key.starts_with("owner_repo_"));
    assert!(!github_key.contains("token"));
    assert!(!github_key.contains(':'));
    assert!(!github_key.contains('/'));
    assert!(!github_key.contains('\\'));

    assert_ne!(
        remote_cache_key("foo/bar_baz"),
        remote_cache_key("foo_bar/baz")
    );
    assert_ne!(
        remote_cache_key("https://github.com/Owner/Repo.git"),
        remote_cache_key("ssh://git@github.com/Owner/Repo.git"),
        "explicit SSH transport keeps a distinct cache identity"
    );
    assert_ne!(
        remote_cache_key("ssh://alice@example.com/Owner/Repo.git"),
        remote_cache_key("ssh://bob@example.com/Owner/Repo.git"),
        "SSH usernames affect routing and keep distinct cache identities"
    );
    assert_eq!(
        remote_cache_identity("ssh://alice:secret@example.com/Owner/Repo.git"),
        remote_cache_identity("ssh://alice:other-secret@example.com/Owner/Repo.git"),
        "passwords do not affect cache identity"
    );
    assert!(
        !remote_cache_identity("ssh://alice:secret@example.com/Owner/Repo.git").contains("secret")
    );

    let err = clone_or_update_remote_source("http://token@example.com/Owner/Repo.git")
        .unwrap_err()
        .to_string();
    assert!(err.contains("plaintext HTTP"), "{err}");
    assert!(!err.contains("token"), "{err}");
}

#[test]
fn remote_git_url_for_subprocess_does_not_expose_https_userinfo() {
    let err = remote_git_url_for_subprocess("https://token@example.com/Owner/Repo.git")
        .unwrap_err()
        .to_string();
    assert!(err.contains("credential-bearing"), "{err}");
    assert!(!err.contains("token"), "{err}");
    assert!(
        err.contains("https://<redacted>@example.com/Owner/Repo.git"),
        "{err}"
    );
}

#[test]
fn cached_repo_origin_validation_accepts_git_ssh_source_after_git_url_normalization() {
    let root = TempDir::new("git-ssh-origin-normalization");
    let cache = root.path().join("cache").join("remote");
    init_git_repo(&cache);
    git(
        &cache,
        &[
            "remote",
            "add",
            "origin",
            "ssh://git@example.com/Owner/Repo.git",
        ],
    );

    validate_cached_repo_origin(
        &remote_source_display("git+ssh://git@example.com/Owner/Repo.git"),
        &remote_git_url_for_subprocess("git+ssh://git@example.com/Owner/Repo.git")
            .unwrap()
            .unwrap(),
        &cache,
    )
    .unwrap();
}

#[test]
fn cached_repo_origin_validation_distinguishes_ssh_usernames() {
    let root = TempDir::new("remote-cache-ssh-user-mismatch");
    let cache = root.path().join("cache").join("remote");
    init_git_repo(&cache);
    git(
        &cache,
        &[
            "remote",
            "add",
            "origin",
            "ssh://bob:actual-secret@example.com/Owner/Repo.git",
        ],
    );

    let err = validate_cached_repo_origin(
        &remote_source_display("ssh://alice:display-secret@example.com/Owner/Repo.git"),
        "ssh://alice:expected-secret@example.com/Owner/Repo.git",
        &cache,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("ssh://bob:<redacted>@example.com/Owner/Repo.git"),
        "{err}"
    );
    assert!(
        err.contains("ssh://alice:<redacted>@example.com/Owner/Repo.git"),
        "{err}"
    );
    assert!(!err.contains("actual-secret"), "{err}");
    assert!(!err.contains("display-secret"), "{err}");
    assert!(!err.contains("expected-secret"), "{err}");
}

#[test]
fn cached_repo_origin_mismatch_does_not_leak_legacy_path_userinfo() {
    let root = TempDir::new("legacy-cache-redaction");
    let legacy_cache_key = "https___token@example.com_owner_repo";
    assert_windows_safe_path_component(legacy_cache_key);
    let cache = root.path().join("cache").join(legacy_cache_key);
    init_git_repo(&cache);
    git(
        &cache,
        &[
            "remote",
            "add",
            "origin",
            "https://other-secret@example.com/Other/Repo.git",
        ],
    );

    let err = validate_cached_repo_origin(
        &remote_source_display("https://token@example.com/Owner/Repo.git"),
        "https://expected-secret@example.com/Owner/Repo.git",
        &cache,
    )
    .unwrap_err()
    .to_string();

    assert!(!err.contains("token"), "{err}");
    assert!(!err.contains("expected-secret"), "{err}");
    assert!(!err.contains("other-secret"), "{err}");
    assert!(
        err.contains("https://<redacted>@example.com/Owner/Repo.git"),
        "{err}"
    );
    assert!(
        err.contains("https://<redacted>@example.com/Other/Repo.git"),
        "{err}"
    );
}

#[test]
fn clone_or_update_remote_source_at_clones_updates_and_fails_closed() {
    let root = TempDir::new("remote-clone");
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let cache = root.path().join("cache").join("owner_repo");
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let remote_url = remote.to_string_lossy().to_string();
    let cloned =
        clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache).unwrap();
    assert_eq!(cloned, cache);
    assert!(cache.join(".git").is_dir());
    assert!(
        std::fs::read_to_string(cache.join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("v1")
    );

    write_skill(&source, "v2\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "update"]);
    git(&source, &["push", "origin", "main"]);
    clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache).unwrap();
    assert!(
        std::fs::read_to_string(cache.join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("v2")
    );

    git(&source, &["checkout", "-B", "next"]);
    write_skill(&source, "v3\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "default branch update"]);
    git(&source, &["push", "origin", "next"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/next"]);
    clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache).unwrap();
    assert!(
        std::fs::read_to_string(cache.join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("v3")
    );

    let other_remote = root.path().join("other.git");
    git(
        root.path(),
        &["init", "--bare", other_remote.to_str().unwrap()],
    );
    git(
        &cache,
        &[
            "remote",
            "set-url",
            "origin",
            other_remote.to_str().unwrap(),
        ],
    );
    let err = clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("has origin"), "{err}");

    let log = git_output(&cache, &["log", "--oneline", "-1"]);
    assert!(log.contains("update"));
}

#[test]
fn cached_repo_update_removes_untracked_worktree_files() {
    let root = TempDir::new("remote-cache-clean");
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let cache = root.path().join("cache").join("owner_repo");
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let remote_url = remote.to_string_lossy().to_string();
    clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache).unwrap();
    let stale = cache.join("skills/stale/SKILL.md");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, "---\nname: stale\ndescription: Stale\n---\n").unwrap();
    assert!(stale.exists(), "negative control: stale file must exist");

    clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &cache).unwrap();

    assert!(
        !stale.exists(),
        "refreshed cache kept untracked source file"
    );
    assert!(
        cache.join(".git").is_dir(),
        "git metadata must be preserved"
    );
    assert!(cache.join("skills/demo/SKILL.md").exists());
}

#[test]
fn legacy_remote_cache_is_used_without_moving_after_origin_validation() {
    let root = TempDir::new("remote-cache-migration");
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let cache_root = root.path().join("cache");
    let legacy_cache = cache_root.join("owner_repo");
    let canonical_cache = cache_root.join(remote_cache_key("owner/repo"));
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(
        root.path(),
        &[
            "clone",
            "--depth",
            "1",
            remote.to_str().unwrap(),
            legacy_cache.to_str().unwrap(),
        ],
    );

    let remote_url = remote.to_string_lossy().to_string();
    let resolved =
        clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &canonical_cache)
            .unwrap();

    assert_eq!(resolved, legacy_cache);
    assert!(legacy_cache.join(".git").is_dir());
    assert!(!canonical_cache.exists());
}

#[test]
fn mismatched_legacy_remote_cache_does_not_block_canonical_clone() {
    let root = TempDir::new("remote-cache-mismatch");
    let remote = root.path().join("remote.git");
    let other_remote = root.path().join("other.git");
    let source = root.path().join("source");
    let other_source = root.path().join("other-source");
    let cache_root = root.path().join("cache");
    let legacy_cache = cache_root.join("owner_repo");
    let canonical_cache = cache_root.join(remote_cache_key("owner/repo"));
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        root.path(),
        &["init", "--bare", other_remote.to_str().unwrap()],
    );

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    init_git_repo(&other_source);
    write_skill(&other_source, "wrong\n");
    git(&other_source, &["add", "."]);
    git(&other_source, &["commit", "-m", "initial"]);
    git(
        &other_source,
        &["remote", "add", "origin", other_remote.to_str().unwrap()],
    );
    git(&other_source, &["push", "origin", "main"]);
    git(&other_remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(
        root.path(),
        &[
            "clone",
            "--depth",
            "1",
            other_remote.to_str().unwrap(),
            legacy_cache.to_str().unwrap(),
        ],
    );

    let remote_url = remote.to_string_lossy().to_string();
    let resolved =
        clone_or_update_remote_source_at("owner/repo", "owner/repo", &remote_url, &canonical_cache)
            .unwrap();

    assert_eq!(resolved, canonical_cache);
    assert!(legacy_cache.join(".git").is_dir());
    assert!(canonical_cache.join(".git").is_dir());
    assert!(
        std::fs::read_to_string(canonical_cache.join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("v1")
    );
}

#[test]
fn update_only_remote_refresh_skips_missing_cache_clone() {
    let root = TempDir::new("remote-cache-update-only");
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    crate::test_util::with_home_and_config(&home, &config_home, || {
        refresh_remote_cache_update_only_best_effort("owner/repo");
        assert!(
            remote_cache_dir("owner/repo").is_some_and(|path| !path.exists()),
            "update-only refresh must not clone a missing cache"
        );
    });
}

#[test]
fn cached_repo_fetch_failure_reports_git_cause_without_reusing_stale_cache() {
    let root = TempDir::new("remote-fetch-failure");
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let cache = root.path().join("cache").join("repo");
    std::fs::create_dir_all(root.path()).unwrap();
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);

    init_git_repo(&source);
    write_skill(&source, "v1\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "origin", "main"]);
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let remote_url = remote.to_string_lossy().to_string();
    clone_or_update_remote_source_at("local remote", "local remote", &remote_url, &cache).unwrap();
    std::fs::remove_dir_all(&remote).unwrap();

    let err = clone_or_update_remote_source_at("local remote", "local remote", &remote_url, &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("git fetch failed"), "{err}");
    assert!(
        err.contains("fatal") || err.contains("does not appear to be a git repository"),
        "git cause should be present: {err}"
    );
}

#[test]
fn remote_diagnostics_redact_url_userinfo_only() {
    let redacted = redact_remote_userinfo_in_text(
        "fatal: could not read https://token@example.com/Owner/Repo.git",
    );
    assert_eq!(
        redacted,
        "fatal: could not read https://<redacted>@example.com/Owner/Repo.git"
    );
    assert!(!redacted.contains("token"));
}

#[test]
fn legacy_remote_cache_keys_never_carry_a_path_separator() {
    let cache_dir = Path::new("/home/u/.vstack/cache/hashed_key");

    let escaping = legacy_remote_cache_dirs("git@github.com:owner/repo\\..\\..\\target", cache_dir);
    for dir in &escaping {
        assert!(
            dir.starts_with("/home/u/.vstack/cache"),
            "legacy dir escaped the cache root: {}",
            dir.display()
        );
        assert!(
            !dir.to_string_lossy().contains('\\'),
            "legacy key retained a backslash separator: {}",
            dir.display()
        );
    }

    assert_eq!(
        legacy_remote_cache_dirs("owner/repo", cache_dir),
        vec![PathBuf::from("/home/u/.vstack/cache/owner_repo")],
        "ordinary slug sources keep their legacy key"
    );
}

#[test]
fn legacy_remote_cache_dirs_reproduce_the_previous_add_cache_key() {
    let cache_dir = Path::new("/home/u/.vstack/cache/hashed_key");

    // `vstack add` minted keys by trimming `.git` and joining the last two
    // slash segments; caches created that way must still be found.
    let ssh = legacy_remote_cache_dirs("git@github.com:owner/repo.git", cache_dir);
    assert!(
        ssh.contains(&PathBuf::from(
            "/home/u/.vstack/cache/git@github.com:owner_repo"
        )),
        "{ssh:?}"
    );

    let https = legacy_remote_cache_dirs("https://example.com/owner/repo.git", cache_dir);
    assert!(
        https.contains(&PathBuf::from("/home/u/.vstack/cache/owner_repo")),
        "{https:?}"
    );
}

#[test]
#[cfg(unix)]
fn cached_repo_update_refuses_a_symlinked_cache_entry() {
    let root = TempDir::new("symlinked-cache-entry");
    let checkout = root.path().join("user-checkout");
    init_git_repo(&checkout);
    // Destructive git commands here would wipe the user's uncommitted work.
    std::fs::write(checkout.join("uncommitted.txt"), "precious\n").unwrap();

    let cache = root.path().join("cache").join("owner_repo");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&checkout, &cache).unwrap();

    let err = update_cached_repo_strict("owner/repo", &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("refusing to update"), "{err}");
    assert!(err.contains("symlink"), "{err}");
    assert!(
        !err.contains(&cache.display().to_string()),
        "a legacy cache path can embed URL userinfo and must not be printed: {err}"
    );
    assert!(
        checkout.join("uncommitted.txt").exists(),
        "the linked checkout must be untouched"
    );
}

#[test]
fn remote_urls_carrying_a_query_or_fragment_are_rejected_and_redacted() {
    for source in [
        "https://example.com/Owner/Repo.git?access_token=secret",
        "https://example.com/Owner/Repo.git#secret",
    ] {
        let err = remote_git_url_for_subprocess(source)
            .unwrap_err()
            .to_string();
        assert!(err.contains("query or fragment"), "{err}");
        assert!(!err.contains("secret"), "{err}");
        assert!(err.contains("<redacted>"), "{err}");
    }

    // Diagnostics for such a source never echo it either.
    let display = remote_source_display("https://example.com/Owner/Repo.git?access_token=secret");
    assert!(!display.contains("secret"), "{display}");
    assert_eq!(display, "https://example.com/Owner/Repo.git?<redacted>");

    // A userinfo token and a query token are both hidden at once.
    let display = remote_source_display("https://token@example.com/Owner/Repo.git?k=secret");
    assert!(!display.contains("token"), "{display}");
    assert!(!display.contains("secret"), "{display}");

    // Ordinary remotes are untouched.
    assert_eq!(
        remote_source_display("https://example.com/Owner/Repo.git"),
        "https://example.com/Owner/Repo.git"
    );
}
