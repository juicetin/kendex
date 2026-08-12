//! Remote source cache: cloning, updating, and validating the cached
//! checkout a remote source refreshes from.

use super::*;

fn assert_windows_safe_path_component(component: &str) {
    let invalid = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    assert!(
        !component.chars().any(|ch| invalid.contains(&ch)),
        "test fixture path component is not Windows-safe: {component}"
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

#[test]
fn cached_repo_origin_with_matching_identity_is_still_rejected_when_it_carries_a_credential() {
    let root = TempDir::new("cached-origin-credential");
    let cache = root.path().join("cache").join("remote");
    init_git_repo(&cache);
    // Identity-equal to the clean expected URL: userinfo normalizes away, so the
    // mismatch check alone would accept this and then fetch with the token.
    git(
        &cache,
        &[
            "remote",
            "add",
            "origin",
            "https://cache-token@github.com/Owner/Repo.git",
        ],
    );

    let err = validate_cached_repo_origin(
        &remote_source_display("Owner/Repo"),
        "https://github.com/Owner/Repo.git",
        &cache,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("credential-bearing origin"), "{err}");
    assert!(!err.contains("cache-token"), "{err}");

    // A clean origin with the same identity still validates.
    git(
        &cache,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/Owner/Repo.git",
        ],
    );
    validate_cached_repo_origin(
        &remote_source_display("Owner/Repo"),
        "https://github.com/Owner/Repo.git",
        &cache,
    )
    .unwrap();
}

#[test]
#[cfg(unix)]
fn cached_repo_update_refuses_a_cache_whose_git_metadata_is_redirected() {
    let root = TempDir::new("cache-redirected-gitdir");
    let checkout = root.path().join("user-checkout");
    init_git_repo(&checkout);
    std::fs::write(checkout.join("uncommitted.txt"), "precious\n").unwrap();

    // A plain directory, so the entry check passes, whose `.git` points at the
    // user's real repository.
    let cache = root.path().join("cache").join("owner_repo");
    std::fs::create_dir_all(&cache).unwrap();
    std::os::unix::fs::symlink(checkout.join(".git"), cache.join(".git")).unwrap();

    let err = update_cached_repo_strict("owner/repo", &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not own its git metadata"), "{err}");
    assert!(
        checkout.join("uncommitted.txt").exists(),
        "the linked checkout must be untouched"
    );

    // A `gitdir:` file is the same redirection by another spelling.
    std::fs::remove_file(cache.join(".git")).unwrap();
    std::fs::write(
        cache.join(".git"),
        format!("gitdir: {}\n", checkout.join(".git").display()),
    )
    .unwrap();
    let err = update_cached_repo_strict("owner/repo", &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not own its git metadata"), "{err}");
}

#[test]
#[cfg(unix)]
fn cached_repo_update_refuses_a_cache_whose_worktree_points_outside_it() {
    let root = TempDir::new("cache-redirected-worktree");
    let origin = root.path().join("origin");
    init_git_repo(&origin);
    std::fs::write(origin.join("README.md"), "upstream\n").unwrap();
    git(&origin, &["add", "README.md"]);
    git(&origin, &["commit", "-m", "add"]);

    // The user's own directory, holding both a file the upstream repo also
    // tracks and an untracked one. `reset --hard` overwrites the first and
    // `clean -ffdx` deletes the second.
    let victim = root.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("README.md"), "precious\n").unwrap();
    std::fs::write(victim.join("notes.txt"), "precious\n").unwrap();

    // A real directory owning a real `.git` directory, so every existing entry
    // check passes — only the repository's own `core.worktree` redirects the
    // destructive commands out of the cache.
    let cache = root.path().join("cache").join("owner_repo");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    git(
        root.path(),
        &["clone", origin.to_str().unwrap(), cache.to_str().unwrap()],
    );
    git(
        &cache,
        &["config", "core.worktree", victim.to_str().unwrap()],
    );

    let err = update_cached_repo_strict("owner/repo", &cache)
        .unwrap_err()
        .to_string();
    assert!(err.contains("refusing to update"), "{err}");
    assert!(err.contains("does not resolve to its cache entry"), "{err}");
    assert!(
        !err.contains(&cache.display().to_string()),
        "a legacy cache path can embed URL userinfo and must not be printed: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(victim.join("README.md")).unwrap(),
        "precious\n",
        "the redirected worktree must be untouched"
    );
    assert!(
        victim.join("notes.txt").exists(),
        "the redirected worktree must be untouched"
    );
}

#[test]
fn cache_git_commands_drop_inherited_repository_and_worktree_variables() {
    let command = cache_git_command(Path::new("/vstack/cache/owner_repo"));
    let removed: Vec<String> = command
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();
    // An inherited `GIT_DIR`/`GIT_WORK_TREE` redirects `reset --hard` and
    // `clean -ffdx` at whatever the caller's environment names, so every
    // repository- and worktree-locating variable must be cleared.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        assert!(
            removed.iter().any(|name| name == key),
            "{key} is not cleared: {removed:?}"
        );
    }
}

#[test]
fn cache_git_commands_refuse_interactive_credential_prompts() {
    let command = cache_git_command(Path::new("/vstack/cache/owner_repo"));
    let env: HashMap<String, Option<String>> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    // A cache whose origin needs credentials would otherwise stop the refresh
    // dead on a username prompt with no terminal to answer it.
    assert_eq!(
        env.get("GIT_TERMINAL_PROMPT").cloned().flatten().as_deref(),
        Some("0"),
        "terminal prompting is not disabled: {env:?}"
    );
    let ssh = env.get("GIT_SSH_COMMAND").cloned().flatten();
    assert!(
        ssh.as_deref().is_some_and(|v| v.contains("BatchMode=yes")),
        "ssh prompting is not disabled: {ssh:?}"
    );
}

#[test]
fn batch_mode_ssh_command_extends_an_inherited_command() {
    assert_eq!(batch_mode_ssh_command(None), "ssh -o BatchMode=yes");
    assert_eq!(batch_mode_ssh_command(Some("   ")), "ssh -o BatchMode=yes");
    // A caller's own ssh binary and options keep working; only prompting is
    // taken away.
    assert_eq!(
        batch_mode_ssh_command(Some("ssh -i /keys/id_ed25519")),
        "ssh -i /keys/id_ed25519 -o BatchMode=yes"
    );
}

fn git_command_environment(
    command: &std::process::Command,
) -> std::collections::BTreeMap<String, Option<String>> {
    command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

#[test]
fn every_cache_git_invocation_carries_the_same_non_interactive_environment() {
    let cache_dir = Path::new("/vstack/cache/owner_repo");
    let update = git_command_environment(&cache_git_command(cache_dir));
    // The control: an unhardened `git` carries none of it, so an equality that
    // holds is a claim about the hardening and not about two empty maps.
    assert_ne!(
        git_command_environment(&std::process::Command::new("git")),
        update,
        "the comparison would pass against a bare git command"
    );
    assert!(
        !update.is_empty(),
        "the update path carries no environment to compare against"
    );
    // Cloning is as unattended as updating: a credential prompt there is a
    // hang in the same propagation run, so the clone must be built by the same
    // hardened constructor.
    assert_eq!(
        git_command_environment(&cache_clone_command(
            "https://github.com/owner/repo.git",
            cache_dir
        )),
        update,
        "the clone path does not match the update path's environment"
    );
}

/// A best-effort update must distinguish the two failures it can meet: a fetch
/// that failed leaves a cache vstack owns, whose stale contents are still the
/// requested source at an older revision, while an ownership refusal means the
/// entry's contents belong to some other checkout.
#[test]
#[cfg(unix)]
fn best_effort_cache_update_tolerates_a_failed_fetch_and_refuses_an_unowned_entry() {
    let root = TempDir::new("best-effort-update-classes");

    // Control: a cache vstack owns whose fetch cannot succeed stays usable, so
    // the refusal asserted below is specific to ownership and not to failure.
    let stale = root.path().join("cache").join("stale");
    init_git_repo(&stale);
    git(
        &stale,
        &[
            "remote",
            "add",
            "origin",
            root.path().join("missing.git").to_str().unwrap(),
        ],
    );
    update_cached_repo_best_effort("owner/repo", &stale)
        .expect("a cache vstack owns must stay usable when only its fetch failed");

    let checkout = root.path().join("user-checkout");
    init_git_repo(&checkout);
    std::fs::write(checkout.join("uncommitted.txt"), "precious\n").unwrap();
    let linked = root.path().join("cache").join("linked");
    std::os::unix::fs::symlink(&checkout, &linked).unwrap();

    let err = update_cached_repo_best_effort("owner/repo", &linked)
        .unwrap_err()
        .to_string();
    assert!(err.contains("refusing to update"), "{err}");
    assert!(err.contains("symlink"), "{err}");
    assert!(
        checkout.join("uncommitted.txt").exists(),
        "the linked checkout must be untouched"
    );
}

/// Fixture: a cache entry that is a symlink to a user checkout carrying the
/// expected `origin`, so every check except the ownership gate accepts it.
#[cfg(unix)]
fn link_cache_entry_at(cache: &Path, checkout: &Path) {
    init_git_repo(checkout);
    git(
        checkout,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/owner/repo.git",
        ],
    );
    std::fs::write(checkout.join("uncommitted.txt"), "precious\n").unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(checkout, cache).unwrap();
}

#[test]
#[cfg(unix)]
fn remote_source_best_effort_refuses_a_symlinked_cache_instead_of_returning_it() {
    let root = TempDir::new("best-effort-symlinked-cache");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let checkout = root.path().join("user-checkout");

    crate::test_util::with_home_and_config(&home, &config_home, || {
        let cache = remote_cache_dir("owner/repo").unwrap();
        link_cache_entry_at(&cache, &checkout);

        // Control: the fixture passes the origin check, so the refusal below is
        // the ownership gate and not a cache this source never matched.
        validate_cached_repo_origin(
            "owner/repo",
            "https://github.com/owner/repo.git",
            &cache,
        )
        .expect("fixture cache must validate as this source's cache");

        let err = clone_or_update_remote_source_best_effort("owner/repo")
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to update"), "{err}");
        assert!(err.contains("symlink"), "{err}");
        assert!(
            checkout.join("uncommitted.txt").exists(),
            "the linked checkout must be untouched"
        );
    });
}

#[test]
#[cfg(unix)]
fn remote_source_best_effort_refuses_a_symlinked_legacy_cache_instead_of_returning_it() {
    let root = TempDir::new("best-effort-symlinked-legacy-cache");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let checkout = root.path().join("user-checkout");

    crate::test_util::with_home_and_config(&home, &config_home, || {
        let canonical = remote_cache_dir("owner/repo").unwrap();
        let legacy = legacy_remote_cache_dirs("owner/repo", &canonical)
            .into_iter()
            .next()
            .expect("owner/repo has a legacy cache key");
        link_cache_entry_at(&legacy, &checkout);

        // Control: the canonical cache is absent, so the legacy branch is the
        // one under test, and the legacy entry passes the origin check.
        assert!(
            !canonical.join(".git").exists(),
            "the canonical cache must be absent for the legacy branch to run"
        );
        validate_cached_repo_origin(
            "owner/repo",
            "https://github.com/owner/repo.git",
            &legacy,
        )
        .expect("fixture legacy cache must validate as this source's cache");

        let err = clone_or_update_remote_source_best_effort("owner/repo")
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to update"), "{err}");
        assert!(err.contains("symlink"), "{err}");
        assert!(
            checkout.join("uncommitted.txt").exists(),
            "the linked checkout must be untouched"
        );
    });
}

/// A cache whose `core.worktree` points at a user checkout passes every check
/// on the entry itself; only the work-tree gate inside the updater catches it.
/// Resolution must drop such a source rather than hand back the checkout.
#[test]
#[cfg(unix)]
fn resolving_a_remote_source_drops_a_cache_whose_work_tree_is_redirected() {
    let root = TempDir::new("resolve-redirected-worktree-cache");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let checkout = root.path().join("user-checkout");
    init_git_repo(&checkout);
    std::fs::write(checkout.join("uncommitted.txt"), "precious\n").unwrap();

    crate::test_util::with_home_and_config(&home, &config_home, || {
        let cache = remote_cache_dir("owner/repo").unwrap();
        init_git_repo(&cache);
        git(
            &cache,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/repo.git",
            ],
        );
        git(
            &cache,
            &["config", "core.worktree", checkout.to_str().unwrap()],
        );

        // Control: the entry itself is a plain owned directory that resolution
        // finds and the origin check accepts, so the `None` below is the
        // updater's refusal rather than a cache that was never a candidate.
        assert_eq!(
            existing_remote_cache_dir("owner/repo"),
            Some(cache.clone()),
            "the fixture must be a cache resolution otherwise accepts"
        );

        assert!(
            resolve_single_source("owner/repo").is_none(),
            "a cache the updater refuses must not be handed back as the source"
        );
        assert!(
            checkout.join("uncommitted.txt").exists(),
            "the redirected checkout must be untouched"
        );
    });
}

/// The same refusal on the read-only resolution path: a cache entry vstack does
/// not own is some other checkout's working tree, and installing from it would
/// treat that tree's uncommitted contents as the remote source.
#[test]
#[cfg(unix)]
fn resolving_a_remote_source_drops_a_symlinked_cache_entry() {
    let root = TempDir::new("resolve-symlinked-cache");
    let home = root.path().join("home");
    let config_home = root.path().join("config");
    let checkout = root.path().join("user-checkout");

    crate::test_util::with_home_and_config(&home, &config_home, || {
        let cache = remote_cache_dir("owner/repo").unwrap();
        link_cache_entry_at(&cache, &checkout);

        assert!(
            resolve_source_path("owner/repo").is_none(),
            "a symlinked cache entry must not resolve as the remote source"
        );

        // Control: with a real cache directory at the same key, carrying the
        // same origin, the source resolves — so the `None` above is the
        // symlink and not a fixture that could never resolve.
        std::fs::remove_file(&cache).unwrap();
        init_git_repo(&cache);
        git(
            &cache,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/repo.git",
            ],
        );
        assert_eq!(resolve_source_path("owner/repo"), Some(cache));
    });
}
