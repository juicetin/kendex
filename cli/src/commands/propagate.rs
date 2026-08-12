use crate::config::{self, ItemKind, LockFile};
use crate::harness::Harness;
use crate::scope::ScopeFilter;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
    source_records: Vec<crate::refresh_sources::ResolvedSource>,
}

impl ScopeDrift {
    fn has_unavailable_sources(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.status == DriftStatus::SourceUnavailable)
    }
}

pub fn run(
    scope: ScopeFilter,
    check: bool,
    verbose: bool,
    stage: bool,
    explicit_scope: bool,
) -> Result<()> {
    if check && stage {
        bail!("--stage cannot be combined with --check");
    }
    if stage && scope != ScopeFilter::Project {
        bail!("--stage is only supported with --scope project");
    }

    let pre_refresh_stage_paths = if stage {
        pre_refresh_project_stage_paths()?
    } else {
        Vec::new()
    };
    let mut checked_any = false;
    let mut drift_any = false;
    let mut unavailable_sources = false;
    let mut source_records_by_scope: Vec<(bool, Vec<crate::refresh_sources::ResolvedSource>)> =
        Vec::new();

    for &global in scope.globals() {
        let drift = detect_drift_for_scope(global)?;
        if drift.checked == 0 {
            continue;
        }
        checked_any = true;
        drift_any |= !drift.rows.is_empty();
        unavailable_sources |= drift.has_unavailable_sources();
        print_scope_report(&drift);
        source_records_by_scope.push((global, drift.source_records.clone()));
    }

    if !checked_any {
        if explicit_scope {
            bail!(
                "no installed items found in explicitly selected {} scope",
                scope.label()
            );
        }
        eprintln!("Nothing installed in selected scope(s).");
        return Ok(());
    }

    if unavailable_sources {
        bail!("cannot propagate while one or more locked sources are unavailable");
    }

    if !drift_any {
        if stage {
            eprintln!("No source drift; verifying and staging current managed changes...");
            crate::commands::verify::run(scope, &[])?;
            stage_project_paths(&pre_refresh_stage_paths)?;
        } else {
            eprintln!("No propagation needed.");
        }
        return Ok(());
    }

    if check {
        bail!(
            "propagation needed; run `vstack propagate --scope {}` to refresh",
            scope.label()
        );
    }

    eprintln!("\nRunning refresh for {} scope...", scope.label());
    crate::commands::refresh::run_with_source_records(scope, verbose, &source_records_by_scope)?;

    eprintln!("\nVerifying refreshed install...");
    crate::commands::verify::run(scope, &[])?;

    if stage {
        stage_project_paths(&pre_refresh_stage_paths)?;
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

    // Strict source resolution clones/updates remote caches once and fails
    // closed when a cached remote cannot be fetched or reset to origin/HEAD.
    let source_records = crate::refresh_sources::resolve_source_records_strict_remote(&lock)?;

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
        source_records,
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

fn pre_refresh_project_stage_paths() -> Result<Vec<PathBuf>> {
    let lock = LockFile::load(&config::lock_file_path(false))?;
    project_stage_paths(&lock, true)
}

fn stage_project_paths(pre_refresh_paths: &[PathBuf]) -> Result<()> {
    let lock = LockFile::load(&config::lock_file_path(false))?;
    let mut paths = BTreeSet::new();
    paths.extend(pre_refresh_paths.iter().cloned());
    paths.extend(project_stage_paths(&lock, false)?);
    paths.extend(managed_paths_from_git_status()?);
    let paths: Vec<PathBuf> = paths.into_iter().collect();
    stage_paths(&paths)
}

fn stage_paths(paths: &[PathBuf]) -> Result<()> {
    let project_root = config::project_root();
    let (stageable, ignored) = filter_stageable_paths(paths)?;
    if !ignored.is_empty() {
        let display: Vec<String> = ignored
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        eprintln!(
            "Skipped ignored vstack-managed paths: {}",
            display.join(", ")
        );
    }
    if stageable.is_empty() {
        eprintln!("No vstack-managed project paths exist to stage.");
        return Ok(());
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(&project_root)
        .args(["add", "-A", "--"])
        .args(&stageable)
        .status()
        .context("running git add for vstack-managed paths")?;
    if !status.success() {
        bail!("git add failed while staging vstack-managed paths");
    }

    let display: Vec<String> = stageable
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    eprintln!("Staged vstack-managed paths: {}", display.join(", "));
    Ok(())
}

fn project_stage_paths(lock: &LockFile, include_missing: bool) -> Result<Vec<PathBuf>> {
    let project_root = config::project_root();
    let mut paths = BTreeSet::new();
    push_if_stageable(
        &mut paths,
        &project_root,
        Path::new(".vstack-lock.json"),
        include_missing,
    );
    push_if_stageable(
        &mut paths,
        &project_root,
        Path::new("vstack.toml"),
        include_missing,
    );
    push_if_stageable(
        &mut paths,
        &project_root,
        Path::new("vstack.settings.toml"),
        include_missing,
    );

    let mut has_agent = false;
    let mut has_opencode_hook = false;
    let mut has_claude_hook = false;
    let mut has_codex_hook = false;
    let mut has_pi_package = false;

    for entry in lock.entries.values() {
        if entry.kind == ItemKind::PiExtension {
            crate::pi_extension::checked_pi_package_path(&entry.name, false)
                .with_context(|| format!("unsafe locked Pi package name {}", entry.name))?;
        } else {
            crate::path_safety::validate_item_name(&entry.name)
                .with_context(|| format!("unsafe locked item name {}", entry.name))?;
        }
        match entry.kind {
            ItemKind::Agent => {
                has_agent = true;
                for harness in entry.harnesses.iter().filter_map(|id| Harness::from_id(id)) {
                    push_abs_if_exists(
                        &mut paths,
                        &project_root,
                        harness
                            .agents_dir(false)
                            .join(harness.agent_filename(&entry.name)),
                        include_missing,
                    );
                }
            }
            ItemKind::Skill => {
                push_if_stageable(
                    &mut paths,
                    &project_root,
                    &Path::new(".agents").join("skills").join(&entry.name),
                    include_missing,
                );
                for harness in entry.harnesses.iter().filter_map(|id| Harness::from_id(id)) {
                    push_abs_if_exists(
                        &mut paths,
                        &project_root,
                        harness.skills_dir(false).join(&entry.name),
                        include_missing,
                    );
                }
            }
            ItemKind::Hook => {
                for harness in entry.harnesses.iter().filter_map(|id| Harness::from_id(id)) {
                    match harness {
                        Harness::ClaudeCode => {
                            has_claude_hook = true;
                            push_if_stageable(
                                &mut paths,
                                &project_root,
                                &Path::new(".claude")
                                    .join("hooks")
                                    .join(format!("{}.sh", entry.name)),
                                include_missing,
                            );
                        }
                        Harness::Cursor => {
                            push_abs_if_exists(
                                &mut paths,
                                &project_root,
                                crate::installer::cursor_hook_rule_path(false, &entry.name),
                                include_missing,
                            );
                        }
                        Harness::OpenCode => {
                            has_opencode_hook = true;
                            push_abs_if_exists(
                                &mut paths,
                                &project_root,
                                crate::installer::opencode_hook_instruction_path(
                                    false,
                                    &entry.name,
                                ),
                                include_missing,
                            );
                        }
                        Harness::Codex => {
                            has_codex_hook = true;
                            push_if_stageable(
                                &mut paths,
                                &project_root,
                                &Path::new(".codex")
                                    .join("hooks")
                                    .join(format!("{}.sh", entry.name)),
                                include_missing,
                            );
                        }
                        Harness::Pi => {}
                    }
                }
            }
            ItemKind::PiExtension => {
                has_pi_package = true;
                let package_dir = crate::pi_extension::checked_pi_package_path(&entry.name, false)?;
                push_abs_if_exists(
                    &mut paths,
                    &project_root,
                    package_dir.clone(),
                    include_missing,
                );
                if let Ok(ext) = crate::pi_extension::PiExtension::from_dir(&package_dir) {
                    for bin_name in ext.bin.keys() {
                        crate::path_safety::validate_item_name(bin_name)
                            .with_context(|| format!("unsafe Pi bin name {bin_name}"))?;
                        push_abs_if_exists(
                            &mut paths,
                            &project_root,
                            config::pi_bin_dir(false).join(bin_name),
                            include_missing,
                        );
                    }
                }
            }
            ItemKind::Extra => {}
        }
    }

    push_project_owned_skill_paths(&mut paths, &project_root, lock, include_missing)?;

    if has_agent {
        push_abs_if_exists(
            &mut paths,
            &project_root,
            crate::agent::failure_reporting_reference_path(false),
            include_missing,
        );
    }
    if has_claude_hook {
        push_if_stageable(
            &mut paths,
            &project_root,
            Path::new(".claude").join("settings.json").as_path(),
            include_missing,
        );
    }
    if has_codex_hook {
        push_if_stageable(
            &mut paths,
            &project_root,
            Path::new(".codex").join("hooks.json").as_path(),
            include_missing,
        );
        push_if_stageable(
            &mut paths,
            &project_root,
            Path::new(".codex").join("config.toml").as_path(),
            include_missing,
        );
    }
    if has_opencode_hook {
        push_abs_if_exists(
            &mut paths,
            &project_root,
            config::opencode_project_config_path(),
            include_missing,
        );
    }
    if has_pi_package {
        push_abs_if_exists(
            &mut paths,
            &project_root,
            config::pi_settings_path(false),
            include_missing,
        );
        push_abs_if_exists(
            &mut paths,
            &project_root,
            config::pi_source_index_path(false),
            include_missing,
        );
        push_abs_if_exists(
            &mut paths,
            &project_root,
            crate::pi_extension::append_system_path(false),
            include_missing,
        );
    }

    Ok(paths.into_iter().collect())
}

fn push_if_stageable(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    relative: &Path,
    include_missing: bool,
) {
    let path = project_root.join(relative);
    push_abs_if_exists(paths, project_root, path, include_missing);
}

fn push_abs_if_exists(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    path: PathBuf,
    include_missing: bool,
) {
    if !include_missing && std::fs::symlink_metadata(&path).is_err() {
        return;
    }
    if let Ok(relative) = path.strip_prefix(project_root) {
        paths.insert(relative.to_path_buf());
    }
}

fn push_project_owned_skill_paths(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    lock: &LockFile,
    include_missing: bool,
) -> Result<()> {
    push_project_skill_dirs_from(
        paths,
        project_root,
        &project_root.join(".agents").join("skills"),
        lock,
        include_missing,
    )?;

    let project_config = crate::project_config::ProjectConfig::load(project_root);
    let Some(configured) = project_config.project_skills_dir.as_deref() else {
        return Ok(());
    };
    let relative = configured.trim().trim_end_matches('/');
    if relative.is_empty() {
        return Ok(());
    }
    let configured_path = safe_project_relative_path(relative)
        .with_context(|| format!("invalid project-skills-dir `{relative}`"))?;
    push_project_skill_dirs_from(
        paths,
        project_root,
        &project_root.join(configured_path),
        lock,
        include_missing,
    )
}

fn push_project_skill_dirs_from(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    skills_root: &Path,
    lock: &LockFile,
    include_missing: bool,
) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(skills_root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", skills_root.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if lock.entries.contains_key(name)
            || crate::path_safety::validate_new_item_name(name).is_err()
        {
            continue;
        }
        if path.join("SKILL.md").is_file() {
            push_abs_if_exists(paths, project_root, path, include_missing);
        }
    }
    Ok(())
}

fn safe_project_relative_path(relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute() {
        bail!("path must be relative");
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => bail!("path must stay inside the project"),
        }
    }
    Ok(path.to_path_buf())
}

fn managed_paths_from_git_status() -> Result<Vec<PathBuf>> {
    let project_root = config::project_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(&project_root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .context("running git status for vstack-managed paths")?;
    if !output.status.success() {
        bail!("git status failed while inspecting vstack-managed paths");
    }

    let project_skill_prefixes = project_skill_status_prefixes(&project_root)?;
    let mut paths = BTreeSet::new();
    let mut skip_next = false;
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        if skip_next {
            skip_next = false;
            continue;
        }
        if record.len() < 4 {
            continue;
        }
        let status = &record[..2];
        let path = path_from_git_status_bytes(&record[3..])?;
        if status[0] == b'R' || status[0] == b'C' {
            skip_next = true;
        }
        if is_safe_relative_path(&path) && is_managed_status_path(&path, &project_skill_prefixes) {
            paths.insert(path);
        }
    }
    Ok(paths.into_iter().collect())
}

fn project_skill_status_prefixes(project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut prefixes = vec![PathBuf::from(".agents").join("skills")];
    let project_config = crate::project_config::ProjectConfig::load(project_root);
    if let Some(configured) = project_config.project_skills_dir.as_deref() {
        let relative = configured.trim().trim_end_matches('/');
        if !relative.is_empty() {
            prefixes.push(
                safe_project_relative_path(relative)
                    .with_context(|| format!("invalid project-skills-dir `{relative}`"))?,
            );
        }
    }
    Ok(prefixes)
}

fn path_from_git_status_bytes(bytes: &[u8]) -> Result<PathBuf> {
    let value = String::from_utf8(bytes.to_vec()).context("git status path was not UTF-8")?;
    Ok(PathBuf::from(value))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn is_managed_status_path(path: &Path, project_skill_prefixes: &[PathBuf]) -> bool {
    let path = path.components().collect::<PathBuf>();
    if matches!(
        path.to_str(),
        Some(".vstack-lock.json")
            | Some("vstack.toml")
            | Some("vstack.settings.toml")
            | Some(".agents/skill-failure-reporting.md")
            | Some(".claude/settings.json")
            | Some(".codex/hooks.json")
            | Some(".codex/config.toml")
            | Some("opencode.json")
            | Some("opencode.jsonc")
            | Some(".pi/settings.json")
            | Some(".pi/.vstack-source.json")
            | Some(".pi/APPEND_SYSTEM.md")
    ) {
        return true;
    }
    let Some(path_str) = path.to_str() else {
        return false;
    };
    project_skill_prefixes
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(prefix))
        || path_str.starts_with(".cursor/rules/safety-")
        || path_str.starts_with(".opencode/instructions/vstack-hook-")
}

fn filter_stageable_paths(paths: &[PathBuf]) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let project_root = config::project_root();
    let mut stageable = Vec::new();
    let mut ignored = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        if !is_safe_relative_path(path) {
            bail!("refusing to stage unsafe path {}", path.display());
        }
        if git_has_tracked_entries(&project_root, path)? {
            stageable.push(path.clone());
            continue;
        }
        if std::fs::symlink_metadata(project_root.join(path)).is_err() {
            continue;
        }
        if git_check_ignore(&project_root, path)? {
            ignored.push(path.clone());
        } else {
            stageable.push(path.clone());
        }
    }
    Ok((stageable, ignored))
}

fn git_has_tracked_entries(project_root: &Path, path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["ls-files", "--"])
        .arg(path)
        .output()
        .with_context(|| format!("checking tracked paths under {}", path.display()))?;
    if !output.status.success() {
        bail!("git ls-files failed while checking {}", path.display());
    }
    Ok(!output.stdout.is_empty())
}

fn git_check_ignore(project_root: &Path, path: &Path) -> Result<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["check-ignore", "-q", "--"])
        .arg(path)
        .status()
        .with_context(|| format!("checking ignore status for {}", path.display()))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("git check-ignore failed while checking {}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallMethod, ItemKind, LockEntry};
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
            assert!(paths.contains(&PathBuf::from(".pi/packages/@scope/pkg")));
            assert!(paths.contains(&PathBuf::from(".pi/bin/pi-tool")));
            assert!(paths.contains(&PathBuf::from(".pi/settings.json")));
            assert!(paths.contains(&PathBuf::from(".pi/.vstack-source.json")));
            assert!(paths.contains(&PathBuf::from(".pi/APPEND_SYSTEM.md")));
            assert!(!paths.contains(&PathBuf::from(".opencode/secret.txt")));
            assert!(!paths.contains(&PathBuf::from(".opencode/agents/unrelated.md")));

            stage_paths(&paths).unwrap();
            let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
            assert!(staged.contains(".vstack-lock.json\n"));
            assert!(staged.contains(".opencode/agents/worker.md\n"));
            assert!(staged.contains(".agents/skill-failure-reporting.md\n"));
            assert!(staged.contains("opencode.json\n"));
            assert!(staged.contains(".opencode/instructions/vstack-hook-protect.md\n"));
            assert!(staged.contains(".pi/packages/@scope/pkg/package.json\n"));
            assert!(staged.contains(".pi/bin/pi-tool\n"));
            assert!(staged.contains(".pi/APPEND_SYSTEM.md\n"));
            assert!(!staged.contains(".opencode/secret.txt"));
            assert!(!staged.contains(".opencode/agents/unrelated.md"));
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
            assert!(paths.contains(&PathBuf::from(".pi/packages/ignored-pkg")));
            stage_paths(&paths).unwrap();

            let staged = git_output(&project, &["diff", "--cached", "--name-only"]);
            assert!(staged.contains(".vstack-lock.json\n"), "{staged}");
            assert!(!staged.contains(".pi/packages/ignored-pkg"), "{staged}");
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
}
