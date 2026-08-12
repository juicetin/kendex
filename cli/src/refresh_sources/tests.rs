//! Source resolution: which on-disk root a lock entry refreshes from.

use super::*;
use crate::config::{InstallMethod, LockEntry};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod remote_cache;

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
