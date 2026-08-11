use crate::config::{self, LockFile};
use crate::scope::ScopeFilter;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriftStatus {
    Changed,
    LegacyHash,
    SourceUnavailable,
}

impl DriftStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::LegacyHash => "legacy-lock",
            Self::SourceUnavailable => "source-unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DriftRow {
    kind: &'static str,
    name: String,
    old_hash: String,
    current_hash: String,
    status: DriftStatus,
}

#[derive(Debug, Default)]
struct ScopeDrift {
    global: bool,
    checked: usize,
    rows: Vec<DriftRow>,
}

impl ScopeDrift {
    fn has_unavailable_sources(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.status == DriftStatus::SourceUnavailable)
    }
}

pub fn run(scope: ScopeFilter, check: bool, verbose: bool, stage: bool) -> Result<()> {
    if check && stage {
        bail!("--stage cannot be combined with --check");
    }
    if stage && scope != ScopeFilter::Project {
        bail!("--stage is only supported with --scope project");
    }

    let mut checked_any = false;
    let mut drift_any = false;
    let mut unavailable_sources = false;

    for &global in scope.globals() {
        let drift = detect_drift_for_scope(global)?;
        if drift.checked == 0 {
            continue;
        }
        checked_any = true;
        drift_any |= !drift.rows.is_empty();
        unavailable_sources |= drift.has_unavailable_sources();
        print_scope_report(&drift);
    }

    if !checked_any {
        eprintln!("Nothing installed in selected scope(s).");
        return Ok(());
    }

    if unavailable_sources {
        bail!("cannot propagate while one or more locked sources are unavailable");
    }

    if !drift_any {
        eprintln!("No propagation needed.");
        return Ok(());
    }

    if check {
        eprintln!(
            "Propagation needed. Run `vstack propagate --scope {}` to refresh.",
            scope.label()
        );
        return Ok(());
    }

    eprintln!("\nRunning refresh for {} scope...", scope.label());
    crate::commands::refresh::run(scope, verbose)?;

    eprintln!("\nVerifying refreshed install...");
    crate::commands::verify::run(scope, &[])?;

    if stage {
        stage_project_paths()?;
    }

    Ok(())
}

fn detect_drift_for_scope(global: bool) -> Result<ScopeDrift> {
    let lock_path = config::lock_file_path(global);
    if !lock_path.exists() {
        return Ok(ScopeDrift {
            global,
            ..ScopeDrift::default()
        });
    }

    let lock = LockFile::load(&lock_path)?;
    if lock.entries.is_empty() {
        return Ok(ScopeDrift {
            global,
            ..ScopeDrift::default()
        });
    }

    ensure_remote_sources_cached(&lock)?;

    // Resolve once up front. For remote sources this updates the cached clone
    // to origin/HEAD before per-entry hashes are compared.
    let _source_records = crate::refresh_sources::resolve_source_records(&lock);

    let mut rows = Vec::new();
    for entry in lock.entries.values() {
        let current_hash = config::compute_source_hash(entry);
        let status = if current_hash.is_empty() {
            Some(DriftStatus::SourceUnavailable)
        } else if entry.source_hash.is_empty() {
            Some(DriftStatus::LegacyHash)
        } else if current_hash != entry.source_hash {
            Some(DriftStatus::Changed)
        } else {
            None
        };

        if let Some(status) = status {
            rows.push(DriftRow {
                kind: entry.kind.label_short(),
                name: entry.name.clone(),
                old_hash: entry.source_hash.clone(),
                current_hash,
                status,
            });
        }
    }

    rows.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.kind.cmp(b.kind)));

    Ok(ScopeDrift {
        global,
        checked: lock.entries.len(),
        rows,
    })
}

fn print_scope_report(drift: &ScopeDrift) {
    let scope_label = if drift.global { "global" } else { "project" };
    eprintln!("\n{scope_label} scope: {} item(s) checked", drift.checked);
    if drift.rows.is_empty() {
        eprintln!("  ok - recorded sources match installed lock hashes");
        return;
    }

    let kind_w = drift
        .rows
        .iter()
        .map(|row| row.kind.len())
        .max()
        .unwrap_or(0);
    let name_w = drift
        .rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or(0);
    for row in &drift.rows {
        eprintln!(
            "  ! {:kw$}  {:nw$}  {} -> {}  ({})",
            row.kind,
            row.name,
            short_hash(&row.old_hash),
            short_hash(&row.current_hash),
            row.status.label(),
            kw = kind_w,
            nw = name_w,
        );
    }
}

fn short_hash(hash: &str) -> String {
    if hash.is_empty() {
        "-".to_string()
    } else {
        hash.chars().take(8).collect()
    }
}

fn ensure_remote_sources_cached(lock: &LockFile) -> Result<()> {
    let mut seen = BTreeSet::new();
    for entry in lock.entries.values() {
        if !seen.insert(entry.source.clone()) {
            continue;
        }
        let Some(git_url) = remote_git_url(&entry.source) else {
            continue;
        };
        let cache_dir = remote_cache_dir_for_source(&entry.source);
        if cache_dir.join(".git").exists() {
            continue;
        }
        if let Some(parent) = cache_dir.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating source cache {}", parent.display()))?;
        }
        eprintln!("Cloning {} into vstack source cache...", entry.source);
        let status = Command::new("git")
            .args(["clone", "--depth", "1", &git_url])
            .arg(&cache_dir)
            .status()
            .with_context(|| format!("running git clone for {}", entry.source))?;
        if !status.success() {
            bail!("git clone failed while caching source {}", entry.source);
        }
    }
    Ok(())
}

fn remote_git_url(source: &str) -> Option<String> {
    if source.starts_with("https://") || source.starts_with("git@") {
        return Some(source.to_string());
    }
    let slug = config::parse_github_slug(source)?;
    Some(format!("https://github.com/{slug}.git"))
}

fn remote_cache_dir_for_source(source: &str) -> PathBuf {
    config::global_base_dir()
        .join(".vstack")
        .join("cache")
        .join(source.replace('/', "_"))
}

fn stage_project_paths() -> Result<()> {
    let project_root = config::project_root();
    let paths: Vec<&str> = [
        ".vstack-lock.json",
        "vstack.toml",
        "vstack.settings.toml",
        ".agents",
        ".claude",
        ".cursor",
        ".codex",
        ".opencode",
        ".pi",
    ]
    .into_iter()
    .filter(|path| project_root.join(path).exists())
    .collect();

    if paths.is_empty() {
        eprintln!("No vstack-managed project paths exist to stage.");
        return Ok(());
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(&project_root)
        .args(["add", "-A", "--"])
        .args(&paths)
        .status()
        .context("running git add for vstack-managed paths")?;
    if !status.success() {
        bail!("git add failed while staging vstack-managed paths");
    }

    eprintln!("Staged vstack-managed paths: {}", paths.join(", "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallMethod, ItemKind, LockEntry};
    use std::path::Path;
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
    fn remote_git_url_accepts_shorthand_and_github_urls() {
        assert_eq!(
            remote_git_url("owner/repo").unwrap(),
            "https://github.com/owner/repo.git"
        );
        assert_eq!(
            remote_git_url("https://github.com/owner/repo.git").unwrap(),
            "https://github.com/owner/repo.git"
        );
        assert_eq!(
            remote_git_url("git@github.com:owner/repo.git").unwrap(),
            "git@github.com:owner/repo.git"
        );
        assert!(remote_git_url("../local-source").is_none());
    }

    #[test]
    fn remote_cache_dir_uses_the_resolver_key_shape() {
        let home = tmpdir("home");
        let config = tmpdir("config");
        crate::test_util::with_home_and_config(&home, &config, || {
            assert_eq!(
                remote_cache_dir_for_source("owner/repo"),
                home.join(".vstack").join("cache").join("owner_repo")
            );
            assert_eq!(
                remote_cache_dir_for_source("https://github.com/owner/repo.git"),
                home.join(".vstack")
                    .join("cache")
                    .join("https:__github.com_owner_repo.git")
            );
        });
    }
}
