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
    let cached = existing_remote_cache_dir(source)?;
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
        && !source.contains('/')
        && !source.contains('\\')
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
    is_plaintext_http_remote(source) || remote_git_url(source).is_some()
}

pub(crate) fn clone_or_update_remote_source(source: &str) -> Result<Option<PathBuf>> {
    reject_plaintext_http_remote(source)?;
    let Some(git_url) = remote_git_url(source) else {
        return Ok(None);
    };
    let display = remote_source_display(source);
    let cache_dir = remote_cache_dir(source).expect("remote source has cache dir");
    clone_or_update_remote_source_at(source, &display, &git_url, &cache_dir).map(Some)
}

pub(crate) fn refresh_remote_cache_best_effort(source: &str) {
    if let Err(err) = clone_or_update_remote_source(source) {
        eprintln!("  Warning: {err}; using cached version if available");
    }
}

pub(crate) fn refresh_remote_cache_update_only_best_effort(source: &str) {
    let Some(cache_dir) = remote_cache_dir(source) else {
        return;
    };
    if cache_dir.join(".git").exists() {
        if let Err(err) = update_existing_remote_cache(source, &cache_dir) {
            eprintln!("  Warning: {err}; using cached version");
        }
        return;
    }
    if let Some(legacy_dir) = legacy_remote_cache_dir(source, &cache_dir)
        && legacy_dir.join(".git").exists()
        && let Err(err) = update_existing_remote_cache(source, &legacy_dir)
    {
        eprintln!("  Warning: {err}; using cached version");
    }
}

fn clone_or_update_remote_source_at(
    source: &str,
    display: &str,
    git_url: &str,
    cache_dir: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating source cache {}", parent.display()))?;
    }

    if cache_dir.join(".git").exists() {
        validate_cached_repo_origin(display, git_url, cache_dir)?;
        update_cached_repo_strict(&display, &cache_dir)?;
        return Ok(cache_dir.to_path_buf());
    }

    if let Some(legacy_dir) = legacy_remote_cache_dir(source, cache_dir)
        && legacy_dir.join(".git").exists()
    {
        match validate_cached_repo_origin(display, git_url, &legacy_dir) {
            Ok(()) => {
                update_cached_repo_strict(display, &legacy_dir)?;
                return Ok(legacy_dir);
            }
            Err(err) => {
                eprintln!(
                    "  Warning: ignoring legacy vstack source cache {}: {err}",
                    legacy_dir.display()
                );
            }
        }
    }

    eprintln!("Cloning {display} into vstack source cache...");
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &git_url])
        .arg(cache_dir)
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git clone for {display}"))?;
    if !output.status.success() {
        bail!(
            "git clone failed while caching source {display}: {}. For private repos, verify Git access with gh auth login or SSH credentials.",
            git_output_summary(&output)
        );
    }
    Ok(cache_dir.to_path_buf())
}

fn remote_git_url(source: &str) -> Option<String> {
    if source.starts_with("https://")
        || source.starts_with("ssh://")
        || source.starts_with("git+ssh://")
        || source.starts_with("git@")
    {
        return Some(source.to_string());
    }
    let slug = config::parse_github_slug(source)?;
    Some(format!("https://github.com/{slug}.git"))
}

pub(crate) fn remote_source_display(source: &str) -> String {
    if let Some(slug) = config::parse_github_slug(source)
        && !source.contains("://")
        && !source.contains('@')
    {
        return slug;
    }
    redact_remote_userinfo(source)
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

fn existing_remote_cache_dir(source: &str) -> Option<PathBuf> {
    let cache_dir = remote_cache_dir(source)?;
    let git_url = remote_git_url(source)?;
    let display = remote_source_display(source);
    if cache_dir.join(".git").exists() {
        if validate_cached_repo_origin(&display, &git_url, &cache_dir).is_ok() {
            return Some(cache_dir);
        }
        return None;
    }
    let legacy_dir = legacy_remote_cache_dir(source, &cache_dir)?;
    if legacy_dir.join(".git").exists()
        && validate_cached_repo_origin(&display, &git_url, &legacy_dir).is_ok()
    {
        Some(legacy_dir)
    } else {
        None
    }
}

pub(crate) fn remote_cache_key(source: &str) -> String {
    let identity = remote_cache_identity(source);
    let prefix = config::parse_github_slug(source)
        .map(|slug| sanitize_cache_component(&slug.replace('/', "_")))
        .unwrap_or_else(|| "remote".to_string());
    format!("{}_{}", prefix, fnv64_hex(&identity))
}

fn remote_cache_identity(source: &str) -> String {
    if let Some(slug) = config::parse_github_slug(source) {
        if source.starts_with("git@") {
            return format!("github+scp:{slug}");
        }
        if source.starts_with("ssh://") || source.starts_with("git+ssh://") {
            return format!("github+ssh:{slug}");
        }
        return format!("github+https:{slug}");
    }
    redact_remote_userinfo(source.trim().trim_end_matches('/'))
        .trim_end_matches(".git")
        .to_string()
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

fn reject_plaintext_http_remote(source: &str) -> Result<()> {
    if is_plaintext_http_remote(source) {
        bail!(
            "plaintext HTTP remote sources are not supported for managed-code refresh: {}",
            remote_source_display(source)
        );
    }
    Ok(())
}

fn is_plaintext_http_remote(source: &str) -> bool {
    source
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
}

fn update_cached_repo_best_effort(source: &str, repo_dir: &Path) {
    let display = remote_source_display(source);
    if let Err(err) = update_cached_repo_strict(&display, repo_dir) {
        eprintln!("  Warning: {err}; using cached version");
    }
}

fn legacy_remote_cache_dir(source: &str, cache_dir: &Path) -> Option<PathBuf> {
    let slug = config::parse_github_slug(source)?;
    let legacy_key = sanitize_cache_component(&slug.replace('/', "_"));
    if legacy_key == remote_cache_key(source) {
        return None;
    }
    Some(cache_dir.parent()?.join(legacy_key))
}

fn validate_cached_repo_origin(display: &str, expected_url: &str, repo_dir: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_dir)
        .output()
        .with_context(|| format!("reading origin for cached source {display}"))?;
    if !output.status.success() {
        bail!(
            "failed to read origin for cached source {display}: {}",
            git_output_summary(&output)
        );
    }
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if remote_cache_identity(&actual) != remote_cache_identity(expected_url) {
        bail!(
            "cached source {display} at {} has origin {}, expected {}; remove the cache directory and retry",
            repo_dir.display(),
            remote_source_display(&actual),
            remote_source_display(expected_url)
        );
    }
    Ok(())
}

fn update_existing_remote_cache(source: &str, repo_dir: &Path) -> Result<()> {
    let git_url = remote_git_url(source).context("not a remote source")?;
    let display = remote_source_display(source);
    validate_cached_repo_origin(&display, &git_url, repo_dir)?;
    update_cached_repo_strict(&display, repo_dir)
}

fn update_cached_repo_strict(display: &str, repo_dir: &Path) -> Result<()> {
    eprintln!("Updating cached repo {display}...");
    let fetch = std::process::Command::new("git")
        .args([
            "fetch",
            "origin",
            "--quiet",
            "--prune",
            "--depth",
            "1",
            "+refs/heads/*:refs/remotes/origin/*",
        ])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git fetch for cached source {display}"))?;
    if !fetch.status.success() {
        bail!(
            "git fetch failed for cached source {display}: {}",
            git_output_summary(&fetch)
        );
    }
    let head = std::process::Command::new("git")
        .args(["remote", "set-head", "origin", "--auto"])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("refreshing origin/HEAD for cached source {display}"))?;
    if !head.status.success() {
        bail!(
            "git remote set-head failed for cached source {display}: {}",
            git_output_summary(&head)
        );
    }
    let reset = std::process::Command::new("git")
        .args(["reset", "--hard", "origin/HEAD"])
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git reset for cached source {display}"))?;
    if !reset.status.success() {
        bail!(
            "git reset failed for cached source {display}: {}",
            git_output_summary(&reset)
        );
    }
    Ok(())
}

fn git_output_summary(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    let combined = match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => format!("{stderr}\n{stdout}"),
        (false, true) => stderr.to_string(),
        (true, false) => stdout.to_string(),
        (true, true) => String::new(),
    };
    let sanitized = redact_remote_userinfo_in_text(combined.trim());
    if sanitized.is_empty() {
        "git exited without stderr".to_string()
    } else {
        sanitized
    }
}

fn redact_remote_userinfo_in_text(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_remote_userinfo)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_remote_userinfo(input: &str) -> String {
    let Some(scheme_end) = input.find("://") else {
        return input.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = input[authority_start..]
        .find(['/', ' ', '\t', '\n', '\r'])
        .map(|idx| authority_start + idx)
        .unwrap_or(input.len());
    let authority = &input[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return input.to_string();
    };
    format!(
        "{}{}{}",
        &input[..authority_start],
        format!("<redacted>@{}", &authority[at + 1..]),
        &input[authority_end..]
    )
}

#[cfg(test)]
mod tests;
