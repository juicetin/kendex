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
    let Some(git_url) = remote_git_url_for_subprocess(source)? else {
        return Ok(None);
    };
    let display = remote_source_display(source);
    let cache_dir = remote_cache_dir(source).expect("remote source has cache dir");
    clone_or_update_remote_source_at(source, &display, &git_url, &cache_dir).map(Some)
}

pub(crate) fn clone_or_update_remote_source_best_effort(source: &str) -> Result<Option<PathBuf>> {
    reject_plaintext_http_remote(source)?;
    let Some(git_url) = remote_git_url_for_subprocess(source)? else {
        return Ok(None);
    };
    let display = remote_source_display(source);
    let cache_dir = remote_cache_dir(source).expect("remote source has cache dir");

    if cache_dir.join(".git").exists() {
        validate_cached_repo_origin(&display, &git_url, &cache_dir)?;
        update_cached_repo_best_effort(source, &cache_dir);
        return Ok(Some(cache_dir));
    }

    for legacy_dir in legacy_remote_cache_dirs(source, &cache_dir) {
        if !legacy_dir.join(".git").exists() {
            continue;
        }
        match validate_cached_repo_origin(&display, &git_url, &legacy_dir) {
            Ok(()) => {
                update_cached_repo_best_effort(source, &legacy_dir);
                return Ok(Some(legacy_dir));
            }
            Err(err) => {
                eprintln!("  Warning: ignoring legacy vstack source cache for {display}: {err}");
            }
        }
    }

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
    let git_url = match remote_git_url_for_subprocess(source) {
        Ok(Some(git_url)) => git_url,
        Ok(None) => return,
        Err(err) => {
            eprintln!("  Warning: {err}");
            return;
        }
    };
    let display = remote_source_display(source);
    for legacy_dir in legacy_remote_cache_dirs(source, &cache_dir) {
        if !legacy_dir.join(".git").exists() {
            continue;
        }
        match validate_cached_repo_origin(&display, &git_url, &legacy_dir) {
            Ok(()) => {
                update_cached_repo_best_effort(source, &legacy_dir);
                return;
            }
            Err(err) => {
                eprintln!("  Warning: ignoring legacy vstack source cache for {display}: {err}");
            }
        }
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

    for legacy_dir in legacy_remote_cache_dirs(source, cache_dir) {
        if !legacy_dir.join(".git").exists() {
            continue;
        }
        match validate_cached_repo_origin(display, git_url, &legacy_dir) {
            Ok(()) => {
                update_cached_repo_strict(display, &legacy_dir)?;
                return Ok(legacy_dir);
            }
            Err(err) => {
                eprintln!("  Warning: ignoring legacy vstack source cache for {display}: {err}");
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

fn remote_git_url_for_subprocess(source: &str) -> Result<Option<String>> {
    let Some(url) = remote_git_url(source) else {
        return Ok(None);
    };
    reject_credential_bearing_git_url(&url)?;
    Ok(Some(git_subprocess_url(&url)))
}

fn git_subprocess_url(url: &str) -> String {
    url.strip_prefix("git+ssh://")
        .map(|rest| format!("ssh://{rest}"))
        .unwrap_or_else(|| url.to_string())
}

pub(crate) fn remote_source_display(source: &str) -> String {
    if let Some(slug) = config::parse_github_slug(source)
        && !source.contains("://")
        && !source.contains('@')
    {
        return slug;
    }
    redact_remote_query(&redact_remote_userinfo(source))
}

/// Replace a URL query/fragment with a marker. Git clone URLs have no
/// legitimate use for either, and both are places a token gets carried
/// (`...?access_token=secret`), so diagnostics must never echo them.
fn redact_remote_query(url: &str) -> String {
    match url.find(['?', '#']) {
        Some(index) => format!("{}<redacted>", &url[..=index]),
        None => url.to_string(),
    }
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
    let git_url = remote_git_url_for_subprocess(source).ok()??;
    let display = remote_source_display(source);
    if cache_dir.join(".git").exists() {
        if validate_cached_repo_origin(&display, &git_url, &cache_dir).is_ok() {
            return Some(cache_dir);
        }
        return None;
    }
    for legacy_dir in legacy_remote_cache_dirs(source, &cache_dir) {
        if legacy_dir.join(".git").exists()
            && validate_cached_repo_origin(&display, &git_url, &legacy_dir).is_ok()
        {
            return Some(legacy_dir);
        }
    }
    None
}

pub(crate) fn remote_cache_key(source: &str) -> String {
    let identity = remote_cache_identity(source);
    let prefix = config::parse_github_slug(source)
        .map(|slug| sanitize_cache_component(&slug.replace('/', "_")))
        .unwrap_or_else(|| "remote".to_string());
    format!("{}_{}", prefix, fnv64_hex(&identity))
}

fn remote_cache_identity(source: &str) -> String {
    let source = source.trim().trim_end_matches('/');
    if let Some(slug) = config::parse_github_slug(source) {
        if source.starts_with("git@") {
            return format!("github+scp:{slug}");
        }
        if source.starts_with("ssh://") || source.starts_with("git+ssh://") {
            return format!("github+ssh:{slug}");
        }
        return format!("github+https:{slug}");
    }
    remote_identity_without_secret_userinfo(source)
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

fn reject_credential_bearing_git_url(url: &str) -> Result<()> {
    // A query or fragment is not part of any legitimate git clone URL, and it
    // is where a token rides when it is not in the userinfo
    // (`https://host/repo.git?access_token=...`). Reject the whole form rather
    // than trying to classify which parameters are secret.
    if url.contains('?') || url.contains('#') {
        bail!(
            "remote source URLs with a query or fragment are not supported for managed-code refresh: {}. Use SSH keys, gh auth login, or a Git credential helper instead.",
            remote_source_display(url)
        );
    }
    let Some(parts) = split_url_userinfo(url) else {
        return Ok(());
    };
    if parts.userinfo.is_empty() {
        return Ok(());
    }
    let secret_bearing =
        !is_ssh_like_scheme(parts.scheme) || parts.userinfo.split_once(':').is_some();
    if secret_bearing {
        bail!(
            "credential-bearing remote source URLs are not supported for managed-code refresh: {}. Use SSH keys, gh auth login, or a Git credential helper instead.",
            remote_source_display(url)
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

/// `update_cached_repo_strict` runs `reset --hard` and `clean -ffdx`, which
/// delete uncommitted work and ignored files wherever they land. A cache entry
/// that is a symlink lands somewhere vstack does not own — a user checkout with
/// the same `origin` passes origin validation — so it must never be the target
/// of those commands. Only a real directory may be updated in place.
fn reject_unsafe_cache_dir(display: &str, repo_dir: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(repo_dir)
        .with_context(|| format!("inspecting cached source {display}"))?;
    // The path is never printed: a legacy cache key is reconstructed from the
    // recorded source and can embed URL userinfo, which the surrounding
    // redaction would otherwise not reach. `display` is already redacted.
    if meta.file_type().is_symlink() {
        bail!(
            "refusing to update cached source {display}: its cache entry is a symlink, and updating it would run destructive git commands outside the cache"
        );
    }
    if !meta.is_dir() {
        bail!("refusing to update cached source {display}: its cache entry is not a directory");
    }
    // `git clone` always leaves a real `.git` directory. A symlink or a
    // `gitdir:` file there redirects the repository metadata elsewhere, so
    // `reset --hard`/`clean -ffdx` would act on a worktree vstack does not own
    // even though the entry itself is a plain directory.
    let git_meta = std::fs::symlink_metadata(repo_dir.join(".git"))
        .with_context(|| format!("inspecting git metadata for cached source {display}"))?;
    if !git_meta.is_dir() || git_meta.file_type().is_symlink() {
        bail!(
            "refusing to update cached source {display}: its cache entry does not own its git metadata"
        );
    }
    Ok(())
}

fn legacy_remote_cache_dirs(source: &str, cache_dir: &Path) -> Vec<PathBuf> {
    let Some(parent) = cache_dir.parent() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    let mut push_key = |key: String| {
        // A legacy key is a single directory name under the cache root. Refuse
        // anything that could still traverse: either separator form, or any
        // relative component. Windows treats a backslash as a separator too,
        // so a recorded source carrying one must not escape the cache root.
        if key.is_empty()
            || key.contains('/')
            || key.contains('\\')
            || Path::new(&key).components().count() != 1
        {
            return;
        }
        let path = parent.join(key);
        if path != cache_dir && !paths.contains(&path) {
            paths.push(path);
        }
    };

    if source.contains('/') && !source.starts_with('.') && !source.starts_with('/') {
        push_key(source.replace(['/', '\\'], "_"));
    }
    if let Some(key) = legacy_add_cache_key(source) {
        push_key(key);
    }
    if let Some(slug) = config::parse_github_slug(source) {
        push_key(sanitize_cache_component(&slug.replace('/', "_")));
    }
    paths
}

/// The cache key `vstack add` used to mint for full URL sources: trim a
/// trailing slash and `.git`, then join the last two slash-separated segments
/// with `_`. Caches created by that implementation are still on disk, and
/// neither the raw-source key (which keeps `.git` and URL prefixes) nor the
/// GitHub-slug key (absent for non-GitHub hosts) reproduces it, so probe it
/// too before falling through to a fresh clone.
fn legacy_add_cache_key(source: &str) -> Option<String> {
    if !source.starts_with("https://") && !source.starts_with("git@") {
        return None;
    }
    let trimmed = source.trim_end_matches('/').trim_end_matches(".git");
    let mut tail: Vec<&str> = trimmed.rsplit('/').take(2).collect();
    tail.reverse();
    if tail.is_empty() {
        return None;
    }
    Some(tail.join("_"))
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
    if remote_cache_identity(&git_subprocess_url(&actual))
        != remote_cache_identity(&git_subprocess_url(expected_url))
    {
        bail!(
            "cached source {display} has origin {}, expected {}; remove the cache directory and retry",
            remote_source_display(&actual),
            remote_source_display(expected_url)
        );
    }
    // A matching identity is not a clean origin: identity comparison normalizes
    // userinfo and query away, so a cache whose own origin carries a token
    // passes the check above and then hands that token to the very next
    // `git fetch origin`. Hold it to the same bar as an input URL.
    reject_credential_bearing_git_url(&git_subprocess_url(&actual)).with_context(|| {
        format!(
            "cached source {display} has a credential-bearing origin; remove the cache directory and retry"
        )
    })?;
    Ok(())
}

fn update_existing_remote_cache(source: &str, repo_dir: &Path) -> Result<()> {
    let git_url = remote_git_url_for_subprocess(source)?.context("not a remote source")?;
    let display = remote_source_display(source);
    validate_cached_repo_origin(&display, &git_url, repo_dir)?;
    update_cached_repo_strict(&display, repo_dir)
}

/// Git's repository- and worktree-locating environment variables. Every one of
/// them overrides the working directory, so an inherited value — vstack invoked
/// from a hook, or from a shell that exported one — would point `reset --hard`
/// and `clean -ffdx` at a repository that is not the cache.
const GIT_LOCATION_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// The ssh command for a cache git invocation, in batch mode. An inherited
/// `GIT_SSH_COMMAND` is extended rather than replaced, so a caller's own ssh
/// binary and options keep working; git appends the host and remote command
/// after this string, so the added option still lands before them.
fn batch_mode_ssh_command(inherited: Option<&str>) -> String {
    let base = inherited
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ssh");
    format!("{base} -o BatchMode=yes")
}

/// A `git` invocation pinned to the cache entry: the working directory decides
/// the repository, with every inherited override cleared.
fn cache_git_command(repo_dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command.current_dir(repo_dir);
    for key in GIT_LOCATION_ENV_VARS {
        command.env_remove(key);
    }
    // A cache whose origin needs credentials would otherwise block on git's
    // username prompt — `update_cached_repo_strict` runs unattended, so that is
    // a hang, not a question. Refuse the prompt in both transports and let the
    // command fail with a message instead.
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env(
        "GIT_SSH_COMMAND",
        batch_mode_ssh_command(std::env::var("GIT_SSH_COMMAND").ok().as_deref()),
    );
    command
}

/// Require the repository reachable from the cache entry to have that entry as
/// its work tree. The environment is sanitized above, but the cache's own
/// `config` can still carry a `core.worktree` pointing at a user checkout, and
/// no check on the entry or its `.git` sees that — `reset --hard` would then
/// overwrite the user's copies of the tracked files and `clean -ffdx` would
/// delete their untracked ones. Ask git where it would act, and refuse unless
/// the answer is the cache.
fn require_cache_is_git_toplevel(display: &str, repo_dir: &Path) -> Result<()> {
    let output = cache_git_command(repo_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .with_context(|| format!("resolving the work tree of cached source {display}"))?;
    if !output.status.success() {
        bail!(
            "refusing to update cached source {display}: its work tree could not be resolved: {}",
            git_output_summary(&output)
        );
    }
    let toplevel = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    // Canonicalized on both sides: the cache root is routinely reached through
    // a symlinked home or a symlinked temp directory, and git reports the
    // resolved path. A path that will not canonicalize fails closed.
    let resolved = std::fs::canonicalize(&toplevel).map_err(|err| {
        anyhow::anyhow!(
            "refusing to update cached source {display}: its work tree could not be resolved: {err}"
        )
    })?;
    let expected = std::fs::canonicalize(repo_dir)
        .map_err(|err| anyhow::anyhow!("refusing to update cached source {display}: {err}"))?;
    if resolved != expected {
        // Neither path is printed: a legacy cache key is reconstructed from the
        // recorded source and can embed URL userinfo, and the work tree is the
        // user location this refuses to touch.
        bail!(
            "refusing to update cached source {display}: its git work tree does not resolve to its cache entry, and updating it would run destructive git commands outside the cache"
        );
    }
    Ok(())
}

fn update_cached_repo_strict(display: &str, repo_dir: &Path) -> Result<()> {
    reject_unsafe_cache_dir(display, repo_dir)?;
    require_cache_is_git_toplevel(display, repo_dir)?;
    eprintln!("Updating cached repo {display}...");
    let fetch = cache_git_command(repo_dir)
        .args([
            "fetch",
            "origin",
            "--quiet",
            "--prune",
            "--depth",
            "1",
            "+refs/heads/*:refs/remotes/origin/*",
        ])
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git fetch for cached source {display}"))?;
    if !fetch.status.success() {
        bail!(
            "git fetch failed for cached source {display}: {}",
            git_output_summary(&fetch)
        );
    }
    let head = cache_git_command(repo_dir)
        .args(["remote", "set-head", "origin", "--auto"])
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("refreshing origin/HEAD for cached source {display}"))?;
    if !head.status.success() {
        bail!(
            "git remote set-head failed for cached source {display}: {}",
            git_output_summary(&head)
        );
    }
    let reset = cache_git_command(repo_dir)
        .args(["reset", "--hard", "origin/HEAD"])
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git reset for cached source {display}"))?;
    if !reset.status.success() {
        bail!(
            "git reset failed for cached source {display}: {}",
            git_output_summary(&reset)
        );
    }
    let clean = cache_git_command(repo_dir)
        .args(["clean", "-ffdx", "--", "."])
        .stdout(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running git clean for cached source {display}"))?;
    if !clean.status.success() {
        bail!(
            "git clean failed for cached source {display}: {}",
            git_output_summary(&clean)
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

struct UrlUserInfo<'a> {
    scheme: &'a str,
    prefix: &'a str,
    userinfo: &'a str,
    host: &'a str,
    suffix: &'a str,
}

fn split_url_userinfo(input: &str) -> Option<UrlUserInfo<'_>> {
    let scheme_end = input.find("://")?;
    let authority_start = scheme_end + 3;
    let authority_end = input[authority_start..]
        .find(['/', ' ', '\t', '\n', '\r'])
        .map(|idx| authority_start + idx)
        .unwrap_or(input.len());
    let authority = &input[authority_start..authority_end];
    let at = authority.rfind('@')?;
    Some(UrlUserInfo {
        scheme: &input[..scheme_end],
        prefix: &input[..authority_start],
        userinfo: &authority[..at],
        host: &authority[at + 1..],
        suffix: &input[authority_end..],
    })
}

fn is_ssh_like_scheme(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("ssh") || scheme.eq_ignore_ascii_case("git+ssh")
}

fn remote_identity_without_secret_userinfo(input: &str) -> String {
    let Some(parts) = split_url_userinfo(input) else {
        return input.to_string();
    };
    let username = parts
        .userinfo
        .split_once(':')
        .map(|(username, _)| username)
        .or_else(|| is_ssh_like_scheme(parts.scheme).then_some(parts.userinfo));
    match username.filter(|username| !username.is_empty()) {
        Some(username) => format!("{}{username}@{}{}", parts.prefix, parts.host, parts.suffix),
        None => format!("{}{}{}", parts.prefix, parts.host, parts.suffix),
    }
}

fn redact_remote_userinfo(input: &str) -> String {
    let Some(parts) = split_url_userinfo(input) else {
        return input.to_string();
    };
    let redacted_userinfo = if let Some((username, _)) = parts.userinfo.split_once(':') {
        if username.is_empty() {
            "<redacted>".to_string()
        } else {
            format!("{username}:<redacted>")
        }
    } else if is_ssh_like_scheme(parts.scheme) {
        parts.userinfo.to_string()
    } else {
        "<redacted>".to_string()
    };
    format!(
        "{}{}@{}{}",
        parts.prefix, redacted_userinfo, parts.host, parts.suffix
    )
}

#[cfg(test)]
mod tests;
