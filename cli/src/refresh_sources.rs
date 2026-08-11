use crate::agent::Agent;
use crate::config::{self, ItemKind};
use crate::hook::Hook;
use crate::mapping::MappingConfig;
use crate::pi_extension::PiExtension;
use crate::skill::Skill;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedSource {
    pub root: PathBuf,
    pub aliases: Vec<String>,
    pub source_repo: Option<String>,
}

#[derive(Clone)]
pub struct RefreshSource {
    pub root: PathBuf,
    pub aliases: Vec<String>,
    pub source_repo: Option<String>,
    pub mapping: MappingConfig,
    pub agents: Vec<Agent>,
    pub skills: Vec<Skill>,
    pub hooks: Vec<Hook>,
    pub pi_extensions: Vec<PiExtension>,
}

impl RefreshSource {
    pub(crate) fn load(record: &ResolvedSource) -> Self {
        Self {
            root: record.root.clone(),
            aliases: record.aliases.clone(),
            source_repo: record.source_repo.clone(),
            mapping: MappingConfig::load(&record.root),
            agents: crate::catalog::discover_agents(&record.root).unwrap_or_default(),
            skills: crate::catalog::discover_skills(&record.root).unwrap_or_default(),
            hooks: crate::catalog::discover_hooks(&record.root).unwrap_or_default(),
            pi_extensions: crate::catalog::discover_pi_extensions(&record.root).unwrap_or_default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_root(root: &Path) -> Self {
        Self::load(&ResolvedSource {
            root: root.to_path_buf(),
            aliases: vec![root.to_string_lossy().into_owned()],
            source_repo: config::source_repo_for_source(Some(root), &root.to_string_lossy()),
        })
    }
}

/// Resolve source directories from lock file entries.
/// Handles absolute local paths, "." (walks up from CWD), and remote shorthand (cached clones).
pub(crate) fn resolve_sources(lock: &config::LockFile) -> Vec<PathBuf> {
    resolve_source_records(lock)
        .into_iter()
        .map(|source| source.root)
        .collect()
}

pub(crate) fn resolve_source_records(lock: &config::LockFile) -> Vec<ResolvedSource> {
    resolve_source_records_with(lock, resolve_recorded_source)
}

pub(crate) fn resolve_source_records_strict_remote(
    lock: &config::LockFile,
) -> Result<Vec<ResolvedSource>> {
    let mut seen = std::collections::BTreeSet::new();
    for entry in lock.entries.values() {
        if seen.insert(entry.source.clone()) {
            clone_or_update_remote_source(&entry.source)?;
        }
    }
    Ok(resolve_source_records_with(
        lock,
        resolve_recorded_source_without_remote_update,
    ))
}

fn resolve_source_records_with(
    lock: &config::LockFile,
    mut resolver: impl FnMut(&str) -> Option<PathBuf>,
) -> Vec<ResolvedSource> {
    let mut sources: Vec<ResolvedSource> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for entry in lock.entries.values() {
        if !seen.insert(entry.source.clone()) {
            continue;
        }
        if let Some(dir) = resolver(&entry.source) {
            push_resolved_source(&mut sources, dir, entry.source.clone());
        }
    }

    // Fallback: walk up from CWD to find a vstack source repo.
    if sources.is_empty()
        && let Ok(mut dir) = std::env::current_dir()
    {
        loop {
            if crate::resolve::is_vstack_source(&dir) {
                let alias = dir.to_string_lossy().into_owned();
                push_resolved_source(&mut sources, dir, alias);
                break;
            }
            if !dir.pop() {
                break;
            }
        }
    }

    // Fallback: try the source registry (cached remote repos).
    if sources.is_empty() {
        let reg_path = config::source_registry_path();
        if let Ok(registry) = config::SourceRegistry::load(&reg_path) {
            for entry in registry.current.iter().chain(registry.entries.iter()) {
                if let Some(dir) = resolver(entry) {
                    push_resolved_source(&mut sources, dir, entry.clone());
                }
            }
        }
    }

    sources
}

fn push_resolved_source(sources: &mut Vec<ResolvedSource>, root: PathBuf, alias: String) {
    let source_repo = config::source_repo_for_source(Some(&root), &alias);
    if let Some(existing) = sources
        .iter_mut()
        .find(|source| same_path(&source.root, &root))
    {
        if !existing.aliases.iter().any(|known| known == &alias) {
            existing.aliases.push(alias);
        }
        if existing.source_repo.is_none() {
            existing.source_repo = source_repo;
        }
    } else {
        sources.push(ResolvedSource {
            root,
            aliases: vec![alias],
            source_repo,
        });
    }
}

pub(crate) fn load_refresh_sources(records: &[ResolvedSource]) -> Vec<RefreshSource> {
    records.iter().map(RefreshSource::load).collect()
}

fn canonicalish(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same_path(a: &Path, b: &Path) -> bool {
    canonicalish(a) == canonicalish(b)
}

pub(crate) fn refresh_source_for_entry<'a>(
    sources: &'a [RefreshSource],
    entry: &config::LockEntry,
) -> Option<&'a RefreshSource> {
    if let Some(source) = sources
        .iter()
        .find(|source| source.aliases.iter().any(|alias| alias == &entry.source))
    {
        return Some(source);
    }

    let entry_path = Path::new(&entry.source);
    if entry_path.is_absolute()
        && let Some(source) = sources
            .iter()
            .find(|source| same_path(&source.root, entry_path))
    {
        return Some(source);
    }

    // Legacy/moved-source fallback: an old lock may record a source string
    // that no longer names anything on disk (a renamed repo, or a pre-1.0 lock
    // with no meaningful source). Those may fall back to the sole loaded
    // source. An entry whose own recorded source still exists must never be
    // rebound to a different one — that silently reinstalled such entries from
    // the wrong repo and masked the real source's edits.
    if sources.len() == 1 && !recorded_source_exists(&entry.source) {
        sources.first()
    } else {
        None
    }
}

pub(crate) fn all_source_hooks(sources: &[RefreshSource]) -> Vec<Hook> {
    sources
        .iter()
        .flat_map(|source| source.hooks.iter().cloned())
        .collect()
}

pub(crate) fn all_source_pi_extensions(sources: &[RefreshSource]) -> Vec<PiExtension> {
    sources
        .iter()
        .flat_map(|source| source.pi_extensions.iter().cloned())
        .collect()
}

pub(crate) fn resolve_skill_pairs_from_sources(
    names: &[String],
    lock: &config::LockFile,
    sources: &[RefreshSource],
) -> Vec<(String, String)> {
    names
        .iter()
        .map(|name| {
            let description = lock
                .entries
                .get(name)
                .filter(|entry| entry.kind == ItemKind::Skill)
                .and_then(|entry| refresh_source_for_entry(sources, entry))
                .and_then(|source| source.skills.iter().find(|skill| &skill.name == name))
                .or_else(|| {
                    sources
                        .iter()
                        .flat_map(|source| source.skills.iter())
                        .find(|skill| &skill.name == name)
                })
                .map(|skill| skill.description.clone())
                .unwrap_or_else(|| name.clone());
            (name.clone(), description)
        })
        .collect()
}

pub(crate) fn source_pi_extension_for_lock_name<'a>(
    pi_extensions: &'a [PiExtension],
    name: &str,
) -> Option<&'a PiExtension> {
    pi_extensions.iter().find(|e| e.name == name).or_else(|| {
        pi_extensions
            .iter()
            .find(|e| crate::pi_extension::legacy_names_for(&e.name).contains(&name))
    })
}

pub(crate) fn resolve_single_source(source: &str) -> Option<PathBuf> {
    resolve_single_source_with(source, true, true)
}

/// Resolve a source string that a lock entry recorded at install time.
///
/// Discovery (`resolve_single_source`) applies the [`crate::resolve::is_vstack_source`]
/// layout heuristic so that walking up from CWD does not mistake an arbitrary
/// directory for a package source. A recorded source needs no such guess: the
/// user named it explicitly on `vstack add`, which accepts any directory
/// holding the asset. Applying the heuristic here silently dropped alternate
/// sources that the heuristic rejects — a dot-named dir, or one carrying only
/// `skills/` — after which the entry fell back to whatever other source was
/// loaded and edits to the real source stopped propagating.
pub(crate) fn resolve_recorded_source(source: &str) -> Option<PathBuf> {
    let path = Path::new(source);
    if path.is_absolute() && path.is_dir() {
        return Some(path.to_path_buf());
    }
    if let Some(path) = resolve_recorded_local_source(source) {
        return Some(path);
    }
    resolve_single_source(source)
}

fn resolve_recorded_source_without_remote_update(source: &str) -> Option<PathBuf> {
    let path = Path::new(source);
    if path.is_absolute() && path.is_dir() {
        return Some(path.to_path_buf());
    }
    if let Some(path) = resolve_recorded_local_source(source) {
        return Some(path);
    }
    resolve_single_source_with(source, false, true)
}

/// Whether an entry's recorded source still names a usable directory on disk.
///
/// Deliberately side-effect free (no remote fetch): callers use it in per-entry
/// loops to decide whether an entry may fall back to a different source.
pub(crate) fn recorded_source_exists(source: &str) -> bool {
    let path = Path::new(source);
    if path.is_absolute() {
        return path.is_dir();
    }
    resolve_recorded_local_source(source).is_some()
}

pub(crate) fn resolve_source_path(source: &str) -> Option<PathBuf> {
    resolve_single_source_with(source, false, false)
}

fn resolve_single_source_with(
    source: &str,
    update_remote: bool,
    require_vstack_source: bool,
) -> Option<PathBuf> {
    // Absolute local path that exists.
    let p = std::path::Path::new(source);
    if p.is_absolute()
        && p.is_dir()
        && (!require_vstack_source || crate::resolve::is_vstack_source(p))
    {
        return Some(p.to_path_buf());
    }

    let looks_like_remote = looks_like_remote_source(source);

    // Explicit relative local source tokens in locks/registries are
    // project-scoped. Treating them as "walk upward to any vstack source" can
    // rebind a live ./source entry to the checkout running the command from a
    // linked worktree, then repair the lock to the wrong source.
    if is_explicit_relative_local_source(source) {
        return resolve_relative_local_source(source, require_vstack_source);
    }

    // Legacy pure hash/reconcile paths accepted bare placeholders such as
    // "source" by falling back to the nearest vstack checkout from CWD. Keep
    // that compatibility only after trying the project-relative path, and only
    // for non-discovery calls where the historical fallback existed.
    if !require_vstack_source && is_bare_local_source(source, looks_like_remote) {
        if let Some(path) = resolve_relative_local_source(source, false) {
            return Some(path);
        }
        return find_vstack_source_from_cwd();
    }

    // Remote shorthand/URL: update once during top-level source resolution,
    // then use the cached clone without side effects from pure attribution/hash paths.
    let cached = remote_cache_dir(source)?;
    if cached.join(".git").exists() {
        if update_remote {
            update_cached_repo_best_effort(source, &cached);
        }
        return Some(cached);
    }

    None
}

fn is_explicit_relative_local_source(source: &str) -> bool {
    source == "." || source.starts_with("./") || source.starts_with("../")
}

fn is_bare_local_source(source: &str, looks_like_remote: bool) -> bool {
    !source.is_empty()
        && !source.starts_with('~')
        && !Path::new(source).is_absolute()
        && !looks_like_remote
}

fn resolve_recorded_local_source(source: &str) -> Option<PathBuf> {
    let looks_like_remote = looks_like_remote_source(source);
    if !is_explicit_relative_local_source(source)
        && !is_bare_local_source(source, looks_like_remote)
    {
        return None;
    }
    resolve_relative_local_source(source, false)
}

fn resolve_relative_local_source(source: &str, require_vstack_source: bool) -> Option<PathBuf> {
    if source.starts_with('~') {
        return None;
    }
    let candidate = config::project_root().join(source);
    if !candidate.is_dir() {
        return None;
    }
    if require_vstack_source && !crate::resolve::is_vstack_source(&candidate) {
        return None;
    }
    Some(canonicalish(&candidate))
}

fn find_vstack_source_from_cwd() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if crate::resolve::is_vstack_source(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub(crate) fn looks_like_remote_source(source: &str) -> bool {
    (source.contains('/') && !source.starts_with('.') && !source.starts_with('/'))
        || source.starts_with("https://")
        || source.starts_with("git@")
}

pub(crate) fn clone_or_update_remote_source(source: &str) -> Result<Option<PathBuf>> {
    let Some(git_url) = remote_git_url(source) else {
        return Ok(None);
    };
    let display = remote_source_display(source);
    let cache_dir = remote_cache_dir(source).expect("remote source has cache dir");
    clone_or_update_remote_source_at(&display, &git_url, &cache_dir).map(Some)
}

fn clone_or_update_remote_source_at(
    display: &str,
    git_url: &str,
    cache_dir: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating source cache {}", parent.display()))?;
    }

    if cache_dir.join(".git").exists() {
        update_cached_repo_strict(&display, &cache_dir)?;
        return Ok(cache_dir.to_path_buf());
    }

    eprintln!("Cloning {display} into vstack source cache...");
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &git_url])
        .arg(cache_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("running git clone for {display}"))?;
    if !status.success() {
        bail!(
            "git clone failed while caching source {display}. For private repos, verify Git access with gh auth login or SSH credentials."
        );
    }
    Ok(cache_dir.to_path_buf())
}

fn remote_git_url(source: &str) -> Option<String> {
    if source.starts_with("https://") || source.starts_with("git@") {
        return Some(source.to_string());
    }
    let slug = config::parse_github_slug(source)?;
    Some(format!("https://github.com/{slug}.git"))
}

pub(crate) fn remote_source_display(source: &str) -> String {
    if let Some(slug) = config::parse_github_slug(source) {
        return slug;
    }
    if source.starts_with("https://") {
        return "https://<redacted>".to_string();
    }
    if source.starts_with("git@") {
        return "git@<redacted>".to_string();
    }
    source.to_string()
}

pub(crate) fn remote_cache_dir(source: &str) -> Option<PathBuf> {
    if remote_git_url(source).is_none() {
        return None;
    }
    Some(
        config::global_base_dir()
            .join(".vstack")
            .join("cache")
            .join(remote_cache_key(source)),
    )
}

pub(crate) fn remote_cache_key(source: &str) -> String {
    if let Some(slug) = config::parse_github_slug(source) {
        return sanitize_cache_component(&slug.replace('/', "_"));
    }
    format!("remote_{}", fnv64_hex(source))
}

fn sanitize_cache_component(input: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in input.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' {
            Some(ch)
        } else {
            Some('_')
        };
        if let Some(next) = next {
            if next == '_' {
                if last_was_sep {
                    continue;
                }
                last_was_sep = true;
            } else {
                last_was_sep = false;
            }
            out.push(next);
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() || is_windows_reserved_name(&trimmed) {
        "source".to_string()
    } else {
        trimmed
    }
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn fnv64_hex(input: &str) -> String {
    let mut state = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100000001b3);
    }
    format!("{state:016x}")
}

fn update_cached_repo_best_effort(source: &str, repo_dir: &Path) {
    let display = remote_source_display(source);
    if let Err(err) = update_cached_repo_strict(&display, repo_dir) {
        eprintln!("  Warning: {err}; using cached version");
    }
}

fn update_cached_repo_strict(display: &str, repo_dir: &Path) -> Result<()> {
    eprintln!("Updating cached repo {display}...");
    let fetch = std::process::Command::new("git")
        .args(["fetch", "origin", "--quiet"])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("running git fetch for cached source {display}"))?;
    if !fetch.success() {
        bail!("git fetch failed for cached source {display}");
    }
    let reset = std::process::Command::new("git")
        .args(["reset", "--hard", "origin/HEAD"])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("running git reset for cached source {display}"))?;
    if !reset.success() {
        bail!("git reset failed for cached source {display}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallMethod, LockEntry};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmpdir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vstack-refresh-source-{label}-{}-{nanos}",
            std::process::id()
        ))
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

    #[test]
    fn resolve_single_source_accepts_absolute_vstack_source() {
        let root = tmpdir("absolute");
        let source = root.join("source");
        std::fs::create_dir_all(source.join("agents")).unwrap();
        std::fs::create_dir_all(source.join("hooks")).unwrap();

        assert_eq!(
            resolve_single_source(&source.to_string_lossy()),
            Some(source.clone())
        );
        assert!(resolve_single_source(&root.to_string_lossy()).is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    /// `vstack add <SOURCE>` accepts any directory holding the asset, so a lock
    /// entry may record one that the discovery heuristic rejects — a dot-named
    /// dir, or one carrying only `skills/`. Dropping it here is what made
    /// refresh fall back to the majority source and stop propagating edits.
    #[test]
    fn resolve_source_records_keeps_a_source_the_layout_heuristic_rejects() {
        let root = tmpdir("recorded-alternate");
        let alternate = root.join(".agents");
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

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_source_records_resolves_relative_sources_from_project_root() {
        let root = tmpdir("recorded-relative");
        let project = root.join("project");
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

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_source_records_records_remote_shorthand_repo_identity() {
        let root = tmpdir("remote-identity");
        let source = make_vstack_source(&root, "source");
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
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_source_records_does_not_infer_identity_from_local_layout() {
        let root = tmpdir("local-layout-identity");
        let source = make_vstack_source(&root, "source");
        let mut lock = config::LockFile::default();
        lock.add(lock_entry("demo", &source.to_string_lossy()));

        let records = resolve_source_records(&lock);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_repo, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn relative_parent_source_uses_current_worktree_lexical_neighbor() {
        let root = tmpdir("recorded-relative-parent");
        let main_project = root.join("dev").join("consumer");
        let main_checkout_neighbor = root.join("dev").join("vstack");
        let linked_worktree = root
            .join("dev")
            .join(".worktrees")
            .join("consumer")
            .join("issue-1");
        let worktree_neighbor = root
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

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recorded_remote_shorthand_does_not_bind_to_project_local_shadow_dir() {
        let root = tmpdir("remote-shadow");
        let project = root.join("project");
        let shadow = project.join("owner").join("repo");
        std::fs::create_dir_all(&shadow).unwrap();

        crate::test_util::with_project_root(&project, || {
            assert!(resolve_recorded_local_source("owner/repo").is_none());
            assert!(!recorded_source_exists("owner/repo"));
        });

        let _ = std::fs::remove_dir_all(root);
    }

    /// An entry whose own source still exists must never be silently rebound to
    /// the sole other loaded source; that reinstalled it from the wrong repo.
    /// The fallback stays available for a source that has genuinely gone away.
    #[test]
    fn refresh_source_for_entry_only_falls_back_when_the_recorded_source_is_gone() {
        let root = tmpdir("no-rebind");
        let alternate = root.join(".agents");
        std::fs::create_dir_all(alternate.join("skills/demo")).unwrap();
        let only_source = make_vstack_source(&root, "other");
        let sources = vec![RefreshSource::from_root(&only_source)];

        let live = lock_entry("demo", &alternate.to_string_lossy());
        assert!(
            refresh_source_for_entry(&sources, &live).is_none(),
            "an entry whose recorded source exists must not bind to a different source"
        );

        let vanished = lock_entry("demo", &root.join("deleted-repo").to_string_lossy());
        assert_eq!(
            refresh_source_for_entry(&sources, &vanished).map(|s| s.root.clone()),
            Some(only_source),
            "legacy lock with a missing source keeps the single-source fallback"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refresh_source_for_entry_does_not_fallback_for_live_relative_source() {
        let root = tmpdir("relative-no-rebind");
        let project = root.join("project");
        let relative_source = project.join("vendor").join("vstack");
        std::fs::create_dir_all(relative_source.join("skills/demo")).unwrap();
        let only_source = make_vstack_source(&root, "other");
        let sources = vec![RefreshSource::from_root(&only_source)];
        let live_relative = lock_entry("demo", "./vendor/vstack");

        crate::test_util::with_project_root(&project, || {
            assert!(
                refresh_source_for_entry(&sources, &live_relative).is_none(),
                "a live relative source must not rebind to the sole loaded source"
            );
        });

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_source_records_calls_resolver_once_per_unique_lock_source() {
        let root = tmpdir("resolver-count");
        let source_a = root.join("source-a");
        let source_b = root.join("source-b");
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

        let _ = std::fs::remove_dir_all(root);
    }

    fn git(repo: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
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
        assert!(git(repo, &["init"]));
        assert!(git(repo, &["checkout", "-B", "main"]));
        assert!(git(repo, &["config", "user.email", "test@example.com"]));
        assert!(git(repo, &["config", "user.name", "VStack Test"]));
    }

    #[test]
    fn remote_helpers_redact_urls_and_use_windows_safe_keys() {
        assert_eq!(
            remote_git_url("owner/repo").unwrap(),
            "https://github.com/owner/repo.git"
        );
        assert_eq!(
            remote_git_url("https://token@github.com/Owner/Repo.git").unwrap(),
            "https://token@github.com/Owner/Repo.git"
        );
        assert!(remote_git_url("../local-source").is_none());

        assert_eq!(
            remote_source_display("https://token@github.com/Owner/Repo.git"),
            "owner/repo"
        );
        assert_eq!(
            remote_source_display("https://token@example.com/Owner/Repo.git"),
            "https://<redacted>"
        );

        let github_key = remote_cache_key("https://token@github.com/Owner/Repo.git");
        assert_eq!(github_key, "owner_repo");

        let opaque_key = remote_cache_key("https://token@example.com/Owner/Repo.git");
        assert!(opaque_key.starts_with("remote_"));
        assert!(!opaque_key.contains("token"));
        assert!(!opaque_key.contains(':'));
        assert!(!opaque_key.contains('/'));
        assert!(!opaque_key.contains('\\'));
    }

    #[test]
    fn clone_or_update_remote_source_at_clones_updates_and_fails_closed() {
        let root = tmpdir("remote-clone");
        let remote = root.join("remote.git");
        let source = root.join("source");
        let cache = root.join("cache").join("owner_repo");
        std::fs::create_dir_all(&root).unwrap();
        assert!(git(&root, &["init", "--bare", remote.to_str().unwrap()]));

        init_git_repo(&source);
        write_skill(&source, "v1\n");
        assert!(git(&source, &["add", "."]));
        assert!(git(&source, &["commit", "-m", "initial"]));
        assert!(git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()]
        ));
        assert!(git(&source, &["push", "origin", "main"]));
        assert!(git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]));

        let remote_url = remote.to_string_lossy().to_string();
        let cloned = clone_or_update_remote_source_at("owner/repo", &remote_url, &cache).unwrap();
        assert_eq!(cloned, cache);
        assert!(cache.join(".git").is_dir());
        assert!(
            std::fs::read_to_string(cache.join("skills/demo/SKILL.md"))
                .unwrap()
                .contains("v1")
        );

        write_skill(&source, "v2\n");
        assert!(git(&source, &["add", "."]));
        assert!(git(&source, &["commit", "-m", "update"]));
        assert!(git(&source, &["push", "origin", "main"]));
        clone_or_update_remote_source_at("owner/repo", &remote_url, &cache).unwrap();
        assert!(
            std::fs::read_to_string(cache.join("skills/demo/SKILL.md"))
                .unwrap()
                .contains("v2")
        );

        assert!(git(
            &cache,
            &["remote", "set-url", "origin", "/missing/vstack.git"]
        ));
        let err = clone_or_update_remote_source_at("owner/repo", &remote_url, &cache)
            .unwrap_err()
            .to_string();
        assert!(err.contains("git fetch failed"));

        let log = git_output(&cache, &["log", "--oneline", "-1"]);
        assert!(log.contains("update"));
    }
}
