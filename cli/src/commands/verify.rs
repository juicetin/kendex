//! Verify the live install matches its source on disk.
//!
//! Two checks per item:
//!
//! 1. **Source vs lock hash.** Compares the current source hash against the
//!    hash recorded in the lock at install time. A mismatch means the
//!    source dir has been edited since the last `add`/`refresh` — the lock
//!    is stale.
//!
//! 2. **Install vs source bytes** (Pi packages only). Walks both the source
//!    package dir and the installed package dir, hashing identical
//!    relative-path/content pairs. A mismatch means refresh didn't fully
//!    copy, or something modified the install. Skills, agents, and hooks
//!    have per-harness translation, so they aren't directly byte-comparable
//!    — we just confirm the expected install path exists for each harness
//!    the lock claims it was installed into.
//!
//! This command is the answer to "did my last refresh actually take?" — a
//! gap that previously required `md5sum` plumbing by hand.
//!
//! Exit code is non-zero if any item fails verification, so this composes
//! with shell pipelines (`vstack verify -g && pi`).

use crate::config::{self, ItemKind, LockEntry};
use crate::refresh_sources::{RefreshSource, ResolvedSource};
use crate::scope::ScopeFilter;
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Per-item verification result.
struct VerifyRow {
    kind: &'static str,
    name: String,
    /// Matches lock hash?
    source_ok: bool,
    /// Install matches source on disk? `None` for items we don't byte-compare.
    install_ok: Option<bool>,
    /// Human-readable note (e.g. "install path missing").
    note: Option<String>,
}

pub fn run(scope: ScopeFilter, names: &[String]) -> Result<()> {
    run_with_source_records(scope, names, &[])
}

pub(crate) fn run_with_source_records(
    scope: ScopeFilter,
    names: &[String],
    resolved_records: &[(bool, Vec<ResolvedSource>)],
) -> Result<()> {
    let mut total_failed = 0usize;
    let mut total_checked = 0usize;
    for &global in scope.globals() {
        let lock_path = config::lock_file_path(global);
        if !lock_path.exists() {
            continue;
        }
        let lock = config::LockFile::load(&lock_path)?;
        if lock.entries.is_empty() {
            continue;
        }
        let scope_label = if global { "GLOBAL" } else { "PROJECT" };
        eprintln!("\n─ verify ({scope_label}) ─");
        // Catalog discovery walks the whole source tree, so load each scope's
        // sources once here rather than per lock entry.
        let refresh_sources = resolved_records
            .iter()
            .find(|(scope_global, _)| *scope_global == global)
            .map(|(_, records)| crate::refresh_sources::load_refresh_sources(records));
        let refresh_sources = refresh_sources.as_deref();

        let mut rows: Vec<VerifyRow> = Vec::new();
        for (entry_name, entry) in &lock.entries {
            if !names.is_empty() && !names.iter().any(|n| n == entry_name) {
                continue;
            }
            rows.push(verify_entry(entry, global, refresh_sources));
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));

        let kind_w = rows.iter().map(|r| r.kind.len()).max().unwrap_or(0);
        let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
        for row in &rows {
            total_checked += 1;
            let source_mark = if row.source_ok { "✓" } else { "!" };
            let install_mark = match row.install_ok {
                Some(true) => "✓",
                Some(false) => "!",
                None => "·",
            };
            let ok = row.source_ok && row.install_ok.unwrap_or(true) && row.note.is_none();
            if !ok {
                total_failed += 1;
            }
            let note = row
                .note
                .as_deref()
                .map(|s| format!("  ({s})"))
                .unwrap_or_default();
            eprintln!(
                "  src:{} install:{}  {:kw$}  {:nw$}{}",
                source_mark,
                install_mark,
                row.kind,
                row.name,
                note,
                kw = kind_w,
                nw = name_w,
            );
        }
    }

    if total_checked == 0 {
        eprintln!("Nothing installed in selected scope(s).");
        return Ok(());
    }

    eprintln!(
        "\n{} checked, {} OK, {} failed",
        total_checked,
        total_checked - total_failed,
        total_failed
    );
    if total_failed > 0 {
        bail!("verification failed for {total_failed} item(s)");
    }
    Ok(())
}

fn verify_entry(entry: &LockEntry, global: bool, sources: Option<&[RefreshSource]>) -> VerifyRow {
    let kind = entry.kind.label_short();
    let name = entry.name.clone();

    // Source hash check (covers all kinds).
    let current = config::compute_source_hash(entry);
    let source_ok = if entry.source_hash.is_empty() {
        // Legacy lock without recorded hash — best effort: just confirm
        // we could resolve a source at all.
        !current.is_empty()
    } else {
        current == entry.source_hash
    };

    // Per-kind install check.
    let (install_ok, note) = match entry.kind {
        ItemKind::PiExtension => verify_pi_install(entry, global, sources),
        ItemKind::Skill => verify_skill_install(entry, global),
        ItemKind::Agent => verify_agent_install(&entry.name, &entry.harnesses, global),
        ItemKind::Hook => verify_hook_install(&entry.name, &entry.harnesses, global),
        ItemKind::Extra => (None, None),
    };

    VerifyRow {
        kind,
        name,
        source_ok,
        install_ok,
        note,
    }
}

fn verify_pi_install(
    entry: &LockEntry,
    global: bool,
    sources: Option<&[RefreshSource]>,
) -> (Option<bool>, Option<String>) {
    let name = &entry.name;
    let install_dir = config::pi_packages_dir(global).join(name);
    if !install_dir.is_dir() {
        return (Some(false), Some("install path missing".into()));
    }
    let source_dir = match locate_pi_source_for_entry(entry, global, sources) {
        Some(p) => p,
        None => return (None, Some("source path unresolvable".into())),
    };
    let src_hash = hash_dir_walk(&source_dir);
    let install_hash = hash_dir_walk(&install_dir);
    let ok = src_hash == install_hash;
    let note = if ok {
        None
    } else {
        Some(format!(
            "install drift: src {} vs install {}",
            short_hash(src_hash),
            short_hash(install_hash)
        ))
    };
    (Some(ok), note)
}

fn locate_pi_source_for_entry(
    entry: &LockEntry,
    global: bool,
    sources: Option<&[RefreshSource]>,
) -> Option<PathBuf> {
    if let Some(sources) = sources
        && let Some(path) = locate_pi_source_from_sources(entry, sources)
    {
        return Some(path);
    }
    locate_pi_source(&entry.name, global)
}

fn locate_pi_source_from_sources(entry: &LockEntry, sources: &[RefreshSource]) -> Option<PathBuf> {
    let source = crate::refresh_sources::refresh_source_for_entry(sources, entry)?;
    crate::refresh_sources::source_pi_extension_for_lock_name(&source.pi_extensions, &entry.name)
        .map(|ext| ext.source_dir.clone())
}

fn verify_skill_install(entry: &LockEntry, global: bool) -> (Option<bool>, Option<String>) {
    if entry.harnesses.is_empty() {
        return (Some(false), Some("no harnesses recorded".into()));
    }
    let mut unknown = Vec::new();
    let mut path_harnesses: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for h in &entry.harnesses {
        let Some(harness) = crate::harness::Harness::from_id(h) else {
            unknown.push(h.clone());
            continue;
        };
        let path = harness.skills_dir(global).join(&entry.name);
        path_harnesses.entry(path).or_default().push(h.clone());
    }

    let mut missing = Vec::new();
    let mut retargeted = Vec::new();
    for (path, harnesses) in path_harnesses {
        if !path.join("SKILL.md").is_file() {
            missing.extend(harnesses);
        } else if !skill_link_resolves_to_canonical_install(&path, &entry.name) {
            retargeted.extend(harnesses);
        }
    }

    if missing.is_empty() && unknown.is_empty() && retargeted.is_empty() {
        (Some(true), None)
    } else {
        let mut notes = Vec::new();
        if !missing.is_empty() {
            notes.push(format!("install path missing for {}", missing.join(", ")));
        }
        if !retargeted.is_empty() {
            notes.push(format!(
                "install path is a symlink outside the canonical skill tree for {}",
                retargeted.join(", ")
            ));
        }
        if !unknown.is_empty() {
            notes.push(format!("unknown harness id(s): {}", unknown.join(", ")));
        }
        (Some(false), Some(notes.join("; ")))
    }
}

/// A skill install that is a symlink must land on a canonical skill tree entry:
/// every checkout spells that `<root>/.agents/skills/<name>`, and the installer
/// only ever points these links at one. Following the link and finding *a*
/// `SKILL.md` is not enough — a redirected link would otherwise pass
/// verification and let `propagate --stage` commit a pointer to unrelated
/// instructions. A non-symlink install (copy method) is not constrained here.
fn skill_link_resolves_to_canonical_install(path: &Path, name: &str) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return true;
    }
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return false;
    };
    resolved.ends_with(Path::new(".agents").join("skills").join(name))
}

fn verify_agent_install(
    name: &str,
    harnesses: &[String],
    global: bool,
) -> (Option<bool>, Option<String>) {
    if harnesses.is_empty() {
        return (Some(false), Some("no harnesses recorded".into()));
    }
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for h in harnesses {
        let Some(harness) = crate::harness::Harness::from_id(h) else {
            unknown.push(h.clone());
            continue;
        };
        let path = harness
            .agents_dir(global)
            .join(harness.agent_filename(name));
        if !path.exists() {
            missing.push(h.clone());
        }
    }
    if missing.is_empty() && unknown.is_empty() {
        (Some(true), None)
    } else {
        let mut notes = Vec::new();
        if !missing.is_empty() {
            notes.push(format!("missing in: {}", missing.join(", ")));
        }
        if !unknown.is_empty() {
            notes.push(format!("unknown harness id(s): {}", unknown.join(", ")));
        }
        (Some(false), Some(notes.join("; ")))
    }
}

fn verify_hook_install(
    name: &str,
    harnesses: &[String],
    global: bool,
) -> (Option<bool>, Option<String>) {
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for h in harnesses {
        let Some(harness) = crate::harness::Harness::from_id(h) else {
            unknown.push(h.clone());
            continue;
        };
        match harness {
            crate::harness::Harness::ClaudeCode => {
                let path = harness
                    .hooks_dir(global)
                    .map(|d| d.join(format!("{name}.sh")));
                if path.is_none_or(|p| !p.exists()) {
                    missing.push(format!("{h}: script missing"));
                }
            }
            crate::harness::Harness::Cursor => {
                let path = crate::installer::cursor_hook_rule_path(global, name);
                if !path.exists() {
                    missing.push(format!("{h}: rule missing"));
                }
            }
            crate::harness::Harness::OpenCode => {
                let path = crate::installer::opencode_hook_instruction_path(global, name);
                if !path.exists() {
                    missing.push(format!("{h}: instruction missing"));
                }
            }
            crate::harness::Harness::Codex => {
                // Native install: script under <root>/.codex/hooks/.
                // Prose-fallback: `## Safety: <name>` block in some agent toml.
                let root = crate::installer::codex_root(global);
                let script = root.join("hooks").join(format!("{name}.sh"));
                let has_script = script.exists();
                let has_prose = !has_script && codex_agent_has_prose(&root, name);
                if !has_script && !has_prose {
                    missing.push(format!("{h}: no script and no prose"));
                }
            }
            crate::harness::Harness::Pi => {
                // Pi has no script-based per-hook install path — the safety
                // hooks ship as the @vanillagreen/pi-hooks extension instead,
                // which is verified separately as a Pi package. Nothing to
                // check here.
            }
        }
    }
    if !unknown.is_empty() {
        missing.push(format!("unknown harness id(s): {}", unknown.join(", ")));
    }
    if missing.is_empty() {
        (Some(true), None)
    } else {
        (Some(false), Some(missing.join("; ")))
    }
}

fn codex_agent_has_prose(codex_root: &Path, hook_name: &str) -> bool {
    let agents_dir = codex_root.join("agents");
    let Ok(entries) = std::fs::read_dir(&agents_dir) else {
        return false;
    };
    let marker = format!("## Safety: {hook_name}");
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.extension().is_some_and(|ex| ex == "toml")
            && std::fs::read_to_string(&path)
                .map(|c| c.contains(&marker))
                .unwrap_or(false)
    })
}

/// Walk a directory and compute an order-stable hash of (relative path, content).
/// Mirrors `config::hash_dir_bytes` so the two are directly comparable.
fn hash_dir_walk(dir: &Path) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut state = FNV_OFFSET;
    let mut walker = walkdir::WalkDir::new(dir)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir()
            && should_skip_hash_dir(entry.file_name().to_string_lossy().as_ref())
        {
            walker.skip_current_dir();
            continue;
        }
        // Mirrors config::hash_dir_bytes_excluding: a symlink contributes its
        // path and its target, so a retargeted link reads as drift.
        if entry.file_type().is_symlink() {
            let Ok(target) = std::fs::read_link(entry.path()) else {
                continue;
            };
            let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
            for bytes in [
                rel.to_string_lossy().as_bytes(),
                config::SYMLINK_HASH_TAG,
                target.to_string_lossy().as_bytes(),
            ] {
                for &b in bytes {
                    state ^= b as u64;
                    state = state.wrapping_mul(FNV_PRIME);
                }
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
        for &b in rel.to_string_lossy().as_bytes() {
            state ^= b as u64;
            state = state.wrapping_mul(FNV_PRIME);
        }
        if let Ok(content) = std::fs::read(entry.path()) {
            for &b in &content {
                state ^= b as u64;
                state = state.wrapping_mul(FNV_PRIME);
            }
        }
    }
    state
}

fn should_skip_hash_dir(name: &str) -> bool {
    // Keep in sync with config::should_skip_hash_dir. `.test-output` is a
    // pi-claude-bridge integration-test artifact dir; running its tests
    // creates symlinks/logs that are gitignored and never part of the
    // distributed package, so they must not influence install drift.
    matches!(
        name,
        "node_modules"
            | ".git"
            | ".turbo"
            | ".next"
            | ".cache"
            | "build"
            | "out"
            | "coverage"
            | ".pi"
            | ".test-output"
    )
}

fn short_hash(h: u64) -> String {
    format!("{h:016x}").chars().take(8).collect()
}

/// Walk the per-scope `.vstack-source.json` to find the source path for a
/// Pi package. Falls back to None if not recorded.
fn locate_pi_source(name: &str, global: bool) -> Option<PathBuf> {
    let index_path = if global {
        crate::config::pi_global_dir().join(".vstack-source.json")
    } else {
        crate::config::pi_project_dir().join(".vstack-source.json")
    };
    let raw = std::fs::read_to_string(&index_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let entry = json.get(name)?;
    let source_path = entry.get("sourcePath").and_then(|v| v.as_str())?;
    let p = PathBuf::from(source_path);
    if p.is_dir() { Some(p) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallMethod, LockEntry, LockFile};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmpdir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vstack-verify-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_file(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn verify_skill_allows_harness_only_copy_install_without_canonical() {
        let project = tmpdir("harness-only-copy-skill");
        let source = tmpdir("harness-only-copy-source");
        std::fs::create_dir_all(&project).unwrap();
        write_file(
            &source.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
        );

        crate::test_util::with_project_root(&project, || {
            let mut entry = LockEntry {
                name: "demo".to_string(),
                kind: ItemKind::Skill,
                source: source.display().to_string(),
                source_repo: None,
                harnesses: vec!["claude-code".to_string()],
                method: InstallMethod::Copy,
                installed_at: "2026-08-11T00:00:00Z".to_string(),
                source_hash: String::new(),
            };
            entry.source_hash = config::compute_source_hash(&entry);
            let mut lock = LockFile::default();
            lock.add(entry);
            lock.save(&config::lock_file_path(false)).unwrap();
            write_file(
                &project.join(".claude/skills/demo/SKILL.md"),
                "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
            );

            run(ScopeFilter::Project, &[]).unwrap();
        });
    }

    #[test]
    fn verify_skill_fails_on_unknown_harness_id() {
        let project = tmpdir("unknown-harness-skill");
        let source = tmpdir("unknown-harness-source");
        std::fs::create_dir_all(&project).unwrap();
        write_file(
            &source.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\nlicense: MIT\n---\n\n# Demo\n",
        );

        crate::test_util::with_project_root(&project, || {
            let mut entry = LockEntry {
                name: "demo".to_string(),
                kind: ItemKind::Skill,
                source: source.display().to_string(),
                source_repo: None,
                harnesses: vec!["ghost".to_string()],
                method: InstallMethod::Copy,
                installed_at: "2026-08-11T00:00:00Z".to_string(),
                source_hash: String::new(),
            };
            entry.source_hash = config::compute_source_hash(&entry);
            let mut lock = LockFile::default();
            lock.add(entry);
            lock.save(&config::lock_file_path(false)).unwrap();

            let (ok, note) = verify_skill_install(lock.entries.get("demo").unwrap(), false);
            assert_eq!(ok, Some(false));
            let note = note.unwrap();
            assert!(note.contains("unknown harness"), "{note}");
            assert!(note.contains("ghost"), "{note}");

            let err = run(ScopeFilter::Project, &[]).unwrap_err().to_string();
            assert!(err.contains("verification failed"), "{err}");
        });
    }

    #[test]
    #[cfg(unix)]
    fn verify_skill_rejects_a_symlink_retargeted_outside_the_canonical_tree() {
        let project = tmpdir("retargeted-skill-symlink");
        std::fs::create_dir_all(&project).unwrap();
        let canonical = project.join(".agents").join("skills").join("demo");
        write_file(&canonical.join("SKILL.md"), "---\nname: demo\n---\n");
        // Unrelated instructions that also happen to carry a SKILL.md.
        let elsewhere = project.join("elsewhere").join("demo");
        write_file(&elsewhere.join("SKILL.md"), "---\nname: other\n---\n");

        crate::test_util::with_project_root(&project, || {
            let entry = LockEntry {
                name: "demo".to_string(),
                kind: ItemKind::Skill,
                source: "/unused/source".to_string(),
                source_repo: None,
                harnesses: vec!["claude-code".to_string()],
                method: InstallMethod::Symlink,
                installed_at: "2026-08-11T00:00:00Z".to_string(),
                source_hash: "stored-hash".to_string(),
            };

            let link = project.join(".claude").join("skills").join("demo");
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&canonical, &link).unwrap();
            let (ok, note) = verify_skill_install(&entry, false);
            assert_eq!(ok, Some(true), "canonical link must pass: {note:?}");

            std::fs::remove_file(&link).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
            let (ok, note) = verify_skill_install(&entry, false);
            assert_eq!(ok, Some(false));
            let note = note.unwrap();
            assert!(note.contains("outside the canonical skill tree"), "{note}");
            assert!(note.contains("claude-code"), "{note}");
        });
    }

    #[test]
    #[cfg(unix)]
    fn verify_hash_walk_tracks_symlink_targets_like_the_source_hash() {
        let root = tmpdir("verify-hash-symlink-target");
        let tree = root.join("pkg");
        std::fs::create_dir_all(tree.join("real")).unwrap();
        std::fs::write(tree.join("real").join("a.js"), b"a").unwrap();
        std::fs::write(tree.join("real").join("b.js"), b"b").unwrap();
        std::os::unix::fs::symlink("real/a.js", tree.join("entry.js")).unwrap();

        let before = hash_dir_walk(&tree);
        std::fs::remove_file(tree.join("entry.js")).unwrap();
        std::os::unix::fs::symlink("real/b.js", tree.join("entry.js")).unwrap();

        assert_ne!(
            before,
            hash_dir_walk(&tree),
            "install drift must see a retargeted symlink"
        );
    }

    #[test]
    fn verify_fails_when_a_lock_entry_records_no_harnesses() {
        let project = tmpdir("empty-harness-entry");
        std::fs::create_dir_all(&project).unwrap();

        crate::test_util::with_project_root(&project, || {
            let entry = LockEntry {
                name: "demo".to_string(),
                kind: ItemKind::Skill,
                source: "/unused/source".to_string(),
                source_repo: None,
                harnesses: Vec::new(),
                method: InstallMethod::Copy,
                installed_at: "2026-08-11T00:00:00Z".to_string(),
                source_hash: "stored-hash".to_string(),
            };
            let (ok, note) = verify_skill_install(&entry, false);
            assert_eq!(ok, Some(false));
            assert!(note.unwrap().contains("no harnesses"), "skill");

            let (ok, note) = verify_agent_install("demo", &[], false);
            assert_eq!(ok, Some(false));
            assert!(note.unwrap().contains("no harnesses"), "agent");
        });
    }

    #[test]
    fn verify_agent_and_hook_fail_on_unknown_harness_id() {
        let project = tmpdir("unknown-harness-agent-hook");
        std::fs::create_dir_all(&project).unwrap();

        crate::test_util::with_project_root(&project, || {
            let (ok, note) = verify_agent_install("demo", &["ghost".to_string()], false);
            assert_eq!(ok, Some(false));
            let note = note.unwrap();
            assert!(note.contains("unknown harness"), "{note}");
            assert!(note.contains("ghost"), "{note}");

            let (ok, note) = verify_hook_install("demo", &["ghost".to_string()], false);
            assert_eq!(ok, Some(false));
            let note = note.unwrap();
            assert!(note.contains("unknown harness"), "{note}");
            assert!(note.contains("ghost"), "{note}");
        });
    }

    #[test]
    fn verify_skill_reports_all_harnesses_sharing_missing_path() {
        let project = tmpdir("shared-missing-skill-path");
        std::fs::create_dir_all(&project).unwrap();
        crate::test_util::with_project_root(&project, || {
            let entry = LockEntry {
                name: "demo".to_string(),
                kind: ItemKind::Skill,
                source: "/unused/source".to_string(),
                source_repo: None,
                harnesses: vec!["codex".to_string(), "pi".to_string()],
                method: InstallMethod::Copy,
                installed_at: "2026-08-11T00:00:00Z".to_string(),
                source_hash: "stored-hash".to_string(),
            };

            let (ok, note) = verify_skill_install(&entry, false);
            assert_eq!(ok, Some(false));
            let note = note.unwrap();
            assert!(note.contains("codex"), "{note}");
            assert!(note.contains("pi"), "{note}");
        });
    }
}
