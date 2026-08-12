use crate::config::{self, ItemKind, LockFile};
use crate::harness::Harness;
use crate::scope::ScopeFilter;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        if check {
            eprintln!("No propagation needed.");
            return Ok(());
        }
        if stage {
            eprintln!("No source drift; verifying and staging current managed changes...");
        } else {
            eprintln!("No source drift; verifying current install...");
        }
        crate::commands::verify::run(scope, &[])?;
        if stage {
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
    let status_paths = managed_paths_from_git_status(&paths)?;
    paths.extend(status_paths);
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

    let status = git_literal_command()
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
                push_pi_package_stage_paths(
                    &mut paths,
                    &project_root,
                    entry,
                    &package_dir,
                    include_missing,
                )?;
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

fn push_pi_package_stage_paths(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    entry: &config::LockEntry,
    package_dir: &Path,
    include_missing: bool,
) -> Result<()> {
    let source_package_dir = config::resolve_source_path(&entry.source).and_then(|source_root| {
        crate::catalog::find_item_path(&source_root, entry.kind, &entry.name)
    });
    let enumerate_root = source_package_dir.as_deref().unwrap_or(package_dir);
    push_pi_package_files_from(
        paths,
        project_root,
        package_dir,
        enumerate_root,
        include_missing,
    )
}

fn push_pi_package_files_from(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    package_dir: &Path,
    enumerate_root: &Path,
    include_missing: bool,
) -> Result<()> {
    if !enumerate_root.is_dir() {
        return Ok(());
    }
    let mut stack = vec![enumerate_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let path = entry.path();
            let file_name = entry.file_name();
            if file_name == OsStr::new("node_modules") {
                continue;
            }
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading file type for {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }
            let Ok(relative_to_package) = path.strip_prefix(enumerate_root) else {
                continue;
            };
            push_abs_if_exists(
                paths,
                project_root,
                package_dir.join(relative_to_package),
                include_missing,
            );
        }
    }
    Ok(())
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

#[derive(Debug)]
struct GitProject {
    root: PathBuf,
    prefix: PathBuf,
}

fn git_project() -> Result<GitProject> {
    let project_root = config::project_root();
    let root = git_stdout_os(
        &project_root,
        &["rev-parse", "--show-toplevel"],
        "resolving git top level for vstack-managed paths",
    )?;
    let prefix = git_stdout_os(
        &project_root,
        &["rev-parse", "--show-prefix"],
        "resolving git project prefix for vstack-managed paths",
    )?;
    Ok(GitProject {
        root: PathBuf::from(root),
        prefix: PathBuf::from(prefix),
    })
}

fn git_stdout_os(project_root: &Path, args: &[&str], context: &str) -> Result<OsString> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()
        .context(context.to_string())?;
    if !output.status.success() {
        bail!("{context}");
    }
    Ok(os_string_without_line_ending(output.stdout))
}

fn os_string_without_line_ending(mut bytes: Vec<u8>) -> OsString {
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    #[cfg(unix)]
    {
        OsString::from_vec(bytes)
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn git_literal_command() -> Command {
    let mut command = Command::new("git");
    command.env("GIT_LITERAL_PATHSPECS", "1");
    command
}

fn managed_paths_from_git_status(seed_paths: &BTreeSet<PathBuf>) -> Result<Vec<PathBuf>> {
    let project_root = config::project_root();
    let git = git_project()?;
    let status_pathspecs = managed_status_pathspecs(seed_paths)?;
    let owned_deleted_native_hooks = owned_deleted_native_hook_paths(seed_paths, &git)?;
    let pi_package_prefixes = pi_package_status_prefixes(seed_paths);
    let top_level_pathspecs: Vec<PathBuf> = status_pathspecs
        .iter()
        .map(|path| project_to_git_path(&git, path))
        .collect();
    let output = git_literal_command()
        .arg("-C")
        .arg(&git.root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
        ])
        .args(&top_level_pathspecs)
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
        let top_level_path = path_from_git_status_bytes(&record[3..]);
        if status[0] == b'R' || status[0] == b'C' {
            skip_next = true;
        }
        let Some(path) = git_to_project_path(&git, &top_level_path) else {
            continue;
        };
        if is_safe_relative_path(&path)
            && is_managed_status_path(
                &path,
                &project_skill_prefixes,
                &pi_package_prefixes,
                &owned_deleted_native_hooks,
                status,
            )
        {
            paths.insert(path);
        }
    }
    Ok(paths.into_iter().collect())
}

fn owned_deleted_native_hook_paths(
    seed_paths: &BTreeSet<PathBuf>,
    git: &GitProject,
) -> Result<BTreeSet<PathBuf>> {
    let mut owned = native_hook_paths_from(seed_paths);
    owned.extend(native_hook_paths_from(&committed_project_stage_paths(git)?));
    Ok(owned)
}

fn committed_project_stage_paths(git: &GitProject) -> Result<BTreeSet<PathBuf>> {
    let project_lock_path = project_to_git_path(git, Path::new(".vstack-lock.json"));
    let Some(project_lock_path) = project_lock_path.to_str() else {
        return Ok(BTreeSet::new());
    };
    let spec = format!("HEAD:{project_lock_path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(&git.root)
        .args(["show", &spec])
        .output()
        .context("reading committed vstack lock for managed hook ownership")?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }
    let Ok(lock) = serde_json::from_slice::<LockFile>(&output.stdout) else {
        return Ok(BTreeSet::new());
    };
    Ok(project_stage_paths(&lock, true)?.into_iter().collect())
}

fn native_hook_paths_from(paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter(|path| is_native_hook_script_path(path))
        .cloned()
        .collect()
}

fn is_native_hook_script_path(path: &Path) -> bool {
    let Some(path_str) = path.to_str() else {
        return false;
    };
    path.extension().is_some_and(|extension| extension == "sh")
        && (path_str.starts_with(".claude/hooks/") || path_str.starts_with(".codex/hooks/"))
}

fn pi_package_status_prefixes(seed_paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    seed_paths
        .iter()
        .filter_map(|path| pi_package_prefix_from_path(path))
        .collect()
}

fn pi_package_prefix_from_path(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    if components.next()?.as_os_str() != ".pi" {
        return None;
    }
    if components.next()?.as_os_str() != "packages" {
        return None;
    }
    let first = components.next()?.as_os_str();
    if first.to_string_lossy().starts_with('@') {
        let second = components.next()?.as_os_str();
        Some(Path::new(".pi").join("packages").join(first).join(second))
    } else {
        Some(Path::new(".pi").join("packages").join(first))
    }
}

fn managed_status_pathspecs(seed_paths: &BTreeSet<PathBuf>) -> Result<Vec<PathBuf>> {
    let project_root = config::project_root();
    let mut paths = seed_paths.clone();
    for prefix in pi_package_status_prefixes(seed_paths) {
        paths.insert(prefix);
    }
    for path in [
        ".vstack-lock.json",
        "vstack.toml",
        "vstack.settings.toml",
        ".agents/skill-failure-reporting.md",
        ".claude/settings.json",
        ".claude/hooks",
        ".codex/hooks.json",
        ".codex/config.toml",
        ".codex/hooks",
        ".cursor/rules",
        ".opencode/instructions",
        "opencode.json",
        "opencode.jsonc",
        ".pi/settings.json",
        ".pi/.vstack-source.json",
        ".pi/APPEND_SYSTEM.md",
    ] {
        paths.insert(PathBuf::from(path));
    }
    for prefix in project_skill_status_prefixes(&project_root)? {
        paths.insert(prefix);
    }
    Ok(paths
        .into_iter()
        .filter(|path| is_safe_relative_path(path))
        .collect())
}

fn project_to_git_path(git: &GitProject, path: &Path) -> PathBuf {
    if git.prefix.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        git.prefix.join(path)
    }
}

fn git_to_project_path(git: &GitProject, path: &Path) -> Option<PathBuf> {
    if git.prefix.as_os_str().is_empty() {
        return Some(path.to_path_buf());
    }
    path.strip_prefix(&git.prefix).ok().map(Path::to_path_buf)
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

fn path_from_git_status_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
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

fn is_managed_status_path(
    path: &Path,
    project_skill_prefixes: &[PathBuf],
    pi_package_prefixes: &BTreeSet<PathBuf>,
    owned_deleted_native_hooks: &BTreeSet<PathBuf>,
    status: &[u8],
) -> bool {
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
        || (pi_package_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
            && !path
                .components()
                .any(|component| component.as_os_str() == OsStr::new("node_modules")))
        || path_str.starts_with(".cursor/rules/safety-")
        || path_str.starts_with(".opencode/instructions/vstack-hook-")
        || (status.contains(&b'D')
            && is_native_hook_script_path(&path)
            && owned_deleted_native_hooks.contains(&path))
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
    let output = git_literal_command()
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
    let mut child = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["check-ignore", "-q", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("checking ignore status for {}", path.display()))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .context("opening git check-ignore stdin")?;
        stdin
            .write_all(&path_bytes(path))
            .with_context(|| format!("writing ignore path {}", path.display()))?;
        stdin
            .write_all(&[0])
            .with_context(|| format!("terminating ignore path {}", path.display()))?;
    }
    let status = child
        .wait()
        .with_context(|| format!("checking ignore status for {}", path.display()))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("git check-ignore failed while checking {}", path.display()),
    }
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests;
