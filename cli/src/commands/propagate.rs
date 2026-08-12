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

    let (pre_refresh_stage_paths, pre_refresh_dirty_shared) = if stage {
        (
            pre_refresh_project_stage_paths()?,
            dirty_shared_config_paths()?,
        )
    } else {
        (Vec::new(), Vec::new())
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
        crate::commands::verify::run_with_source_records(scope, &[], &source_records_by_scope)?;
        if stage {
            // Install breakage first: it is the more actionable diagnostic, and
            // the shared-config guard below still runs before anything stages.
            verify_project_auxiliary_installs_before_stage()?;
            refuse_pre_existing_shared_config_edits(&pre_refresh_dirty_shared)?;
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
    crate::commands::verify::run_with_source_records(scope, &[], &source_records_by_scope)?;

    if stage {
        verify_project_auxiliary_installs_before_stage()?;
        refuse_pre_existing_shared_config_edits(&pre_refresh_dirty_shared)?;
        stage_project_paths_after_refresh(&pre_refresh_stage_paths)?;
    }

    Ok(())
}

/// A shared config file that was already modified or untracked before
/// propagation ran carries consumer state propagation did not make. Git stages
/// whole files, so the only way not to absorb it is to refuse and let the
/// consumer separate their work first. Applied on both staging paths: the
/// no-drift branch stages the same shared files and is the one a scheduled job
/// hits most often.
fn refuse_pre_existing_shared_config_edits(dirty: &[PathBuf]) -> Result<()> {
    if dirty.is_empty() {
        return Ok(());
    }
    let display: Vec<String> = dirty
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    bail!(
        "refusing to stage: shared vstack-managed config file(s) already had uncommitted changes before propagation ran: {}. Commit them (or stash, including untracked) first so the automated commit carries only propagated changes.",
        display.join(", ")
    )
}

fn verify_project_auxiliary_installs_before_stage() -> Result<()> {
    let lock = LockFile::load(&config::lock_file_path(false))?;
    let mut failures = Vec::new();
    for entry in lock.entries.values() {
        match entry.kind {
            ItemKind::Hook => {
                if entry.harnesses.is_empty() {
                    failures.push(format!(
                        "locked hook {} records no harnesses, so no safety mechanism was checked",
                        entry.name
                    ));
                }
                let registration = locked_hook_registration(entry);
                for harness in entry.harnesses.iter().filter_map(|id| Harness::from_id(id)) {
                    verify_hook_auxiliary_install(
                        &entry.name,
                        harness,
                        registration.as_ref(),
                        &mut failures,
                    );
                }
            }
            ItemKind::PiExtension => {
                verify_pi_auxiliary_install(&entry.name, &mut failures)?;
            }
            // Skills, agents, and extras have no auxiliary registration to
            // check: they are files, verified by `verify::run`. Listed rather
            // than wildcarded so a new kind has to be considered here.
            ItemKind::Skill | ItemKind::Agent | ItemKind::Extra => {}
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "auxiliary verification failed before staging: {}",
            failures.join("; ")
        )
    }
}

fn verify_hook_auxiliary_install(
    name: &str,
    harness: Harness,
    registration: Option<&LockedHookRegistration>,
    failures: &mut Vec<String>,
) {
    match harness {
        Harness::ClaudeCode => {
            let script = Path::new(".claude")
                .join("hooks")
                .join(format!("{name}.sh"));
            if config::project_root().join(&script).exists() {
                require_installed_hook_script_matches_source(
                    &script,
                    registration,
                    &format!("Claude hook {name}"),
                    failures,
                );
                require_hook_command_registration(
                    Path::new(".claude").join("settings.json").as_path(),
                    &crate::installer::claude_project_hook_command(name),
                    registration.map(|r| (r.event(), r.matcher())),
                    &format!("Claude hook {name}"),
                    failures,
                );
            }
        }
        Harness::Codex => {
            let script = Path::new(".codex").join("hooks").join(format!("{name}.sh"));
            if config::project_root().join(&script).exists() {
                require_installed_hook_script_matches_source(
                    &script,
                    registration,
                    &format!("Codex hook {name}"),
                    failures,
                );
                require_hook_command_registration(
                    Path::new(".codex").join("hooks.json").as_path(),
                    &crate::installer::codex_project_hook_command(name),
                    registration.and_then(|r| {
                        crate::installer::codex_event_for(r.event())
                            .map(|event| (event, r.matcher()))
                    }),
                    &format!("Codex hook {name}"),
                    failures,
                );
                // Codex ignores hooks.json entirely unless the runtime feature
                // is on, so a registered hook with the flag off or the config
                // deleted is a broken install, not a stageable state.
                let config_toml = Path::new(".codex").join("config.toml");
                if !crate::installer::codex_hooks_feature_enabled(
                    &config::project_root().join(&config_toml),
                ) {
                    failures.push(format!(
                        "{} missing [features] hooks = true for Codex hook {name}",
                        config_toml.display()
                    ));
                }
            }
        }
        Harness::OpenCode => {
            let instruction = crate::installer::opencode_hook_instruction_path(false, name);
            if instruction.exists() {
                require_translated_hook_artifact_matches_source(
                    &instruction,
                    registration
                        .map(|r| crate::installer::opencode_hook_instruction_contents(&r.hook)),
                    &format!("OpenCode hook {name}"),
                    failures,
                );
                require_opencode_instruction_registration(name, failures);
            }
        }
        Harness::Cursor => {
            let rule = crate::installer::cursor_hook_rule_path(false, name);
            if rule.exists() {
                require_translated_hook_artifact_matches_source(
                    &rule,
                    registration.map(|r| crate::installer::cursor_hook_rule_contents(&r.hook)),
                    &format!("Cursor hook {name}"),
                    failures,
                );
            }
        }
        _ => {}
    }
}

/// OpenCode only loads an instruction file listed in the *active* config's
/// `instructions` array — the file existing on disk proves nothing, and a
/// registration sitting in the inactive spelling is not loaded either. The
/// active file is whichever `config::opencode_project_config_path` selects,
/// the same one the installer writes to.
fn require_opencode_instruction_registration(name: &str, failures: &mut Vec<String>) {
    let expected = crate::installer::opencode_hook_instruction_ref(false, name);
    let config_path = config::opencode_project_config_path();
    let project_root = config::project_root();
    let relative = config_path
        .strip_prefix(&project_root)
        .unwrap_or(config_path.as_path());
    let label = relative.display();
    if !config_path.exists() {
        failures.push(format!(
            "{label} missing registration for OpenCode hook {name}"
        ));
        return;
    }
    match read_project_json(relative) {
        Ok(json) if opencode_config_lists_instruction(&json, &expected) => {}
        Ok(_) => failures.push(format!(
            "{label} missing instructions entry {expected} for OpenCode hook {name}"
        )),
        Err(err) => failures.push(format!("{label} {err}")),
    }
}

fn opencode_config_lists_instruction(json: &serde_json::Value, expected: &str) -> bool {
    json.get("instructions")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entries| entries.iter().any(|entry| entry.as_str() == Some(expected)))
}

/// The event and matcher a locked hook must be registered with, read from its
/// source definition. `None` means the definition could not be read — a
/// dependency failure, not a pass: callers refuse to stage rather than accept a
/// registration they cannot check.
struct LockedHookRegistration {
    hook: crate::hook::Hook,
}

impl LockedHookRegistration {
    fn event(&self) -> &str {
        &self.hook.event
    }

    fn matcher(&self) -> Option<&str> {
        self.hook.matcher.as_deref()
    }

    /// The bytes the installer writes verbatim to `.claude/hooks/<name>.sh`
    /// and `.codex/hooks/<name>.sh`.
    fn script(&self) -> &str {
        &self.hook.script
    }
}

fn locked_hook_registration(entry: &config::LockEntry) -> Option<LockedHookRegistration> {
    let source_root = config::resolve_source_path(&entry.source)?;
    crate::catalog::discover_hooks(&source_root)
        .ok()?
        .into_iter()
        .find(|hook| hook.name == entry.name)
        .map(|hook| LockedHookRegistration { hook })
}

fn verify_pi_auxiliary_install(name: &str, failures: &mut Vec<String>) -> Result<()> {
    let package_dir = crate::pi_extension::checked_pi_package_path(name, false)?;
    if std::fs::symlink_metadata(&package_dir).is_err() {
        return Ok(());
    }
    let settings_path = config::pi_settings_path(false);
    let relative_settings = settings_path
        .strip_prefix(config::project_root())
        .unwrap_or(settings_path.as_path());
    match read_project_json(relative_settings) {
        Ok(settings) => {
            if !pi_settings_references_package(&settings, name, &package_dir) {
                failures.push(format!(
                    "{} missing registration for Pi package {name}",
                    relative_settings.display()
                ));
            }
        }
        Err(err) => failures.push(format!("{} {err}", relative_settings.display())),
    }

    verify_pi_append_system_block(name, &package_dir, failures)?;
    verify_pi_source_index_entry(name, failures);

    #[cfg(unix)]
    if let Ok(ext) = crate::pi_extension::PiExtension::from_dir(&package_dir) {
        for (bin_name, rel_target) in &ext.bin {
            crate::pi_extension::validate_pi_bin_name(bin_name)
                .with_context(|| format!("unsafe Pi bin name {bin_name}"))?;
            let declared = crate::pi_extension::checked_package_child_path(
                &package_dir,
                rel_target,
                "Pi bin target",
            )
            .with_context(|| format!("unsafe Pi bin target {rel_target}"))?;
            let bin = config::pi_bin_dir(false).join(bin_name);
            if let Some(failure) = pi_bin_link_failure(&bin, bin_name, &declared, name) {
                failures.push(failure);
            }
        }
    }

    Ok(())
}

/// Pi loads `.pi/APPEND_SYSTEM.md` as the project's whole append-system
/// payload, and nothing else checks it: package hashing covers the package
/// directory, not this file. A package that declares `pi.appendSystem` must
/// therefore have its marker-delimited block present and byte-equal to the
/// content it declares, or the prompt silently lost instructions the lock says
/// are installed.
fn verify_pi_append_system_block(
    name: &str,
    package_dir: &Path,
    failures: &mut Vec<String>,
) -> Result<()> {
    let declared = crate::pi_extension::declared_append_system_content(package_dir)?;
    let append_path = crate::pi_extension::append_system_path(false);
    let installed = crate::pi_extension::append_system_block_content(&append_path, name)?;
    match (declared, installed) {
        (None, None) => {}
        (None, Some(_)) => failures.push(format!(
            ".pi/APPEND_SYSTEM.md still carries a block for Pi package {name}, which no longer declares one"
        )),
        (Some(_), None) => failures.push(format!(
            ".pi/APPEND_SYSTEM.md missing the block for Pi package {name}"
        )),
        (Some(declared), Some(installed)) if declared != installed => failures.push(format!(
            ".pi/APPEND_SYSTEM.md block for Pi package {name} does not match the package content"
        )),
        (Some(_), Some(_)) => {}
    }
    Ok(())
}

/// `.pi/.vstack-source.json` is the sidecar `vstack update-pi` and
/// pi-extension-manager read for update detection. Source records resolved
/// elsewhere let verification succeed without it, so a missing, malformed, or
/// incomplete sidecar would otherwise survive a clean no-drift run.
fn verify_pi_source_index_entry(name: &str, failures: &mut Vec<String>) {
    match crate::pi_extension::read_source_index(false) {
        Ok(index) => match index.get(name) {
            // A key alone is not metadata: `vstack update-pi` and
            // pi-extension-manager classify a record with no source to compare
            // against as Unknown. Either locator is enough — a recorded path
            // may be stale without being useless, which verification already
            // tolerates by resolving sources separately.
            Some(entry) if source_index_entry_has_locator(entry) => {}
            Some(_) => failures.push(format!(
                ".pi/.vstack-source.json entry for Pi package {name} records no source repo or path"
            )),
            None => failures.push(format!(
                ".pi/.vstack-source.json missing the entry for Pi package {name}"
            )),
        },
        Err(err) => failures.push(format!(".pi/.vstack-source.json {err}")),
    }
}

fn source_index_entry_has_locator(entry: &crate::pi_extension::SourceIndexEntry) -> bool {
    [&entry.source_repo, &entry.source_path]
        .into_iter()
        .any(|value| {
            value
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        })
}

/// A locked `.pi/bin/<cmd>` is only a valid install when it is a symlink that
/// resolves to the exact file the package's `bin` entry declares. An inode
/// alone is not evidence, and neither is package containment: a consumer-owned
/// regular file, a link redirected elsewhere, and a link redirected to another
/// file inside the same package all run something other than the locked
/// entrypoint, so staging must refuse them rather than absorb them into a PR.
#[cfg(unix)]
fn pi_bin_link_failure(
    bin: &Path,
    bin_name: &str,
    declared_target: &Path,
    package_name: &str,
) -> Option<String> {
    let Ok(meta) = std::fs::symlink_metadata(bin) else {
        return Some(format!(
            ".pi/bin/{bin_name} missing for Pi package {package_name}"
        ));
    };
    if !meta.file_type().is_symlink() {
        return Some(format!(
            ".pi/bin/{bin_name} is not a symlink for Pi package {package_name}"
        ));
    }
    let Ok(target) = std::fs::read_link(bin) else {
        return Some(format!(
            ".pi/bin/{bin_name} is an unreadable symlink for Pi package {package_name}"
        ));
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        bin.parent().unwrap_or(Path::new(".")).join(target)
    };
    let resolved = crate::installer::normalize_absolute_path(&resolved);
    let declared = crate::installer::normalize_absolute_path(declared_target);
    if resolved != declared {
        return Some(format!(
            ".pi/bin/{bin_name} does not point at the target declared by Pi package {package_name}"
        ));
    }
    if !resolved.exists() {
        return Some(format!(
            ".pi/bin/{bin_name} is a dangling symlink for Pi package {package_name}"
        ));
    }
    None
}

/// The installer writes a native hook script verbatim from its source
/// definition, so any difference on disk is a local edit — a body replaced or
/// commented out still passes an existence check and still registers, and
/// staging would commit the disabled script into an automated PR. Compared
/// byte for byte against the source; an unreadable source fails closed, the
/// same way the event check does.
fn require_installed_hook_script_matches_source(
    relative: &Path,
    registration: Option<&LockedHookRegistration>,
    label: &str,
    failures: &mut Vec<String>,
) {
    let Some(registration) = registration else {
        failures.push(format!(
            "cannot read the locked script for {label} from its source; refusing to verify {}",
            relative.display()
        ));
        return;
    };
    let path = config::project_root().join(relative);
    match std::fs::read_to_string(&path) {
        Ok(installed) if installed == registration.script() => {}
        Ok(_) => failures.push(format!(
            "{} does not match the locked script for {label}",
            relative.display()
        )),
        Err(err) => failures.push(format!("{} is unreadable: {err}", relative.display())),
    }
}

/// A translated safety artifact — an OpenCode instruction file, a Cursor rule —
/// is rendered from the hook, not copied, so its body is still fully
/// installer-owned. Existence and registration say nothing about the prose
/// actually in it, and a locally replaced body would otherwise be staged as
/// though propagation produced it. `expected` is `None` only when the source
/// definition could not be read, which fails closed like the other checks.
fn require_translated_hook_artifact_matches_source(
    path: &Path,
    expected: Option<String>,
    label: &str,
    failures: &mut Vec<String>,
) {
    let project_root = config::project_root();
    let display = path.strip_prefix(&project_root).unwrap_or(path).display();
    let Some(expected) = expected else {
        failures.push(format!(
            "cannot read the locked definition for {label} from its source; refusing to verify {display}"
        ));
        return;
    };
    match std::fs::read_to_string(path) {
        Ok(installed) if installed == expected => {}
        Ok(_) => failures.push(format!(
            "{display} does not match the locked {label} content"
        )),
        Err(err) => failures.push(format!("{display} is unreadable: {err}")),
    }
}

/// Require the exact command the installer writes to be live in the harness
/// config, under the locked event and with the locked matcher. The harness only
/// runs `hooks.<event>[].hooks[]` entries whose `type` is `command`, so a path
/// in an unrelated metadata string is not a registration — and neither is a
/// command that merely mentions the script (`echo <script>`,
/// `bash <script>.disabled`), which is why this compares the whole command
/// rather than searching within it. The matcher decides which tool calls reach
/// the hook, so a rewritten matcher disables it just as effectively.
///
/// `expected` is `None` only when the source definition could not be read.
/// That is a dependency failure, not a pass: it fails closed rather than
/// accepting a registration under an unverified event.
fn require_hook_command_registration(
    relative: &Path,
    expected_command: &str,
    expected: Option<(&str, Option<&str>)>,
    label: &str,
    failures: &mut Vec<String>,
) {
    let Some((event, matcher)) = expected else {
        failures.push(format!(
            "cannot read the locked event for {label} from its source; refusing to verify {}",
            relative.display()
        ));
        return;
    };
    match read_project_json(relative) {
        Ok(json) if hook_command_registered(&json, expected_command, event, matcher) => {}
        Ok(_) => failures.push(format!(
            "{} missing command registration under {event} for {label}",
            relative.display()
        )),
        Err(err) => failures.push(format!("{} {err}", relative.display())),
    }
}

fn hook_command_registered(
    json: &serde_json::Value,
    expected_command: &str,
    event: &str,
    matcher: Option<&str>,
) -> bool {
    let Some(entries) = json
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .and_then(|hooks| hooks.get(event))
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    entries
        .iter()
        .filter(|entry| entry.get("matcher").and_then(serde_json::Value::as_str) == matcher)
        .filter_map(|entry| entry.get("hooks").and_then(serde_json::Value::as_array))
        .flatten()
        .any(|handler| {
            handler.get("type").and_then(serde_json::Value::as_str) == Some("command")
                && handler.get("command").and_then(serde_json::Value::as_str)
                    == Some(expected_command)
        })
}

fn read_project_json(relative: &Path) -> Result<serde_json::Value> {
    let path = config::project_root().join(relative);
    let content =
        std::fs::read_to_string(&path).with_context(|| "missing or unreadable".to_string())?;
    serde_json::from_str(&content).with_context(|| "contains invalid JSON".to_string())
}

fn pi_settings_references_package(
    settings: &serde_json::Value,
    name: &str,
    package_dir: &Path,
) -> bool {
    let canonical = format!("./packages/{name}");
    let absolute = package_dir.to_string_lossy();
    let matches = |value: &str| value == canonical || value == absolute;
    settings
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|packages| {
            packages.iter().any(|entry| match entry {
                serde_json::Value::String(value) => matches(value),
                serde_json::Value::Object(map) => map
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(matches),
                _ => false,
            })
        })
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

/// How a locked source that will not resolve should be treated while
/// collecting stage paths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceStrictness {
    /// Fail: the paths this pass returns are the authoritative staged set.
    Required,
    /// Skip the item: this pass runs before sources have been resolved, so a
    /// remote cache may not exist yet on a clean runner. `stage_project_paths`
    /// recollects with `Required` after the refresh, so nothing is lost.
    BestEffort,
}

/// Whether a refresh rewrote the managed tree in this invocation. A Pi package
/// directory is cleared and re-copied wholesale by install, so after a refresh
/// every tracked path under it is vstack-driven; without one, a vanished
/// tracked file is the consumer's own and must not be staged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshState {
    Refreshed,
    NotRefreshed,
}

fn pre_refresh_project_stage_paths() -> Result<Vec<PathBuf>> {
    let lock = LockFile::load(&config::lock_file_path(false))?;
    // Runs before `detect_drift_for_scope` clones remote sources.
    project_stage_paths_with(&lock, true, SourceStrictness::BestEffort)
}

fn stage_project_paths(pre_refresh_paths: &[PathBuf]) -> Result<()> {
    stage_project_paths_with(pre_refresh_paths, RefreshState::NotRefreshed)
}

fn stage_project_paths_after_refresh(pre_refresh_paths: &[PathBuf]) -> Result<()> {
    stage_project_paths_with(pre_refresh_paths, RefreshState::Refreshed)
}

fn stage_project_paths_with(pre_refresh_paths: &[PathBuf], refreshed: RefreshState) -> Result<()> {
    let lock = LockFile::load(&config::lock_file_path(false))?;
    let mut paths = BTreeSet::new();
    paths.extend(pre_refresh_paths.iter().cloned());
    paths.extend(project_stage_paths(&lock, false)?);
    let mut ownership_paths = paths.clone();
    ownership_paths.extend(project_stage_paths(&lock, true)?);
    let status_paths = managed_paths_from_git_status(&ownership_paths, refreshed)?;
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
    project_stage_paths_with(lock, include_missing, SourceStrictness::Required)
}

fn project_stage_paths_with(
    lock: &LockFile,
    include_missing: bool,
    strictness: SourceStrictness,
) -> Result<Vec<PathBuf>> {
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
    let mut has_pi_append_system = false;

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
                push_locked_skill_stage_paths(&mut paths, &project_root, entry, include_missing)?;
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
                    strictness,
                )?;
                if let Ok(ext) = crate::pi_extension::PiExtension::from_dir(&package_dir) {
                    if ext.append_system.is_some() {
                        has_pi_append_system = true;
                    }
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
    }
    // Owning a Pi package is not ownership of the whole prompt file. Stage
    // `.pi/APPEND_SYSTEM.md` only when a locked package actually contributes a
    // block, or when the file still carries one that a package drop must
    // remove; a purely consumer-authored prompt stays out of the PR.
    let append_system = crate::pi_extension::append_system_path(false);
    if has_pi_package
        && (has_pi_append_system
            || crate::pi_extension::append_system_has_managed_block(&append_system)?)
    {
        push_abs_if_exists(&mut paths, &project_root, append_system, include_missing);
    }

    Ok(paths.into_iter().collect())
}

fn push_locked_skill_stage_paths(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    entry: &config::LockEntry,
    include_missing: bool,
) -> Result<()> {
    let Some(source_skill_dir) =
        config::resolve_source_path(&entry.source).and_then(|source_root| {
            crate::catalog::find_item_path(&source_root, entry.kind, &entry.name)
        })
    else {
        return Ok(());
    };
    let canonical_dir = project_root
        .join(".agents")
        .join("skills")
        .join(&entry.name);
    for dest_dir in locked_skill_stage_dirs(project_root, entry) {
        let dest_meta = std::fs::symlink_metadata(&dest_dir);
        let dest_is_symlink = dest_meta
            .as_ref()
            .is_ok_and(|meta| meta.file_type().is_symlink());
        let missing_symlink_dest = cfg!(unix)
            && include_missing
            && entry.method == config::InstallMethod::Symlink
            && dest_dir != canonical_dir
            && dest_meta.is_err();
        if dest_is_symlink || missing_symlink_dest {
            push_abs_if_exists(paths, project_root, dest_dir, include_missing);
            continue;
        }
        push_installed_files_from_source(
            paths,
            project_root,
            &dest_dir,
            &source_skill_dir,
            include_missing,
        )?;
    }
    Ok(())
}

fn locked_skill_stage_dirs(project_root: &Path, entry: &config::LockEntry) -> BTreeSet<PathBuf> {
    let mut dirs = BTreeSet::new();
    dirs.insert(
        project_root
            .join(".agents")
            .join("skills")
            .join(&entry.name),
    );
    for harness in entry.harnesses.iter().filter_map(|id| Harness::from_id(id)) {
        dirs.insert(harness.skills_dir(false).join(&entry.name));
    }
    dirs
}

fn push_installed_files_from_source(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    install_dir: &Path,
    source_dir: &Path,
    include_missing: bool,
) -> Result<()> {
    if !source_dir.is_dir() {
        return Ok(());
    }
    let mut stack = vec![source_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let path = entry.path();
            let file_name = entry.file_name();
            if file_name == OsStr::new(".vstack-refreshed") {
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
            let Ok(relative_to_skill) = path.strip_prefix(source_dir) else {
                continue;
            };
            push_abs_if_exists(
                paths,
                project_root,
                install_dir.join(relative_to_skill),
                include_missing,
            );
        }
    }
    Ok(())
}

fn push_pi_package_stage_paths(
    paths: &mut BTreeSet<PathBuf>,
    project_root: &Path,
    entry: &config::LockEntry,
    package_dir: &Path,
    include_missing: bool,
    strictness: SourceStrictness,
) -> Result<()> {
    let source_root = match config::resolve_source_path(&entry.source) {
        Some(root) => root,
        None if strictness == SourceStrictness::BestEffort => return Ok(()),
        None => bail!(
            "unable to resolve source for locked Pi package {}",
            entry.name
        ),
    };
    let source_package_dir =
        match crate::catalog::find_item_path(&source_root, entry.kind, &entry.name) {
            Some(dir) => dir,
            None if strictness == SourceStrictness::BestEffort => return Ok(()),
            None => bail!(
                "unable to find source package {} in {}",
                entry.name,
                source_root.display()
            ),
        };
    push_pi_package_files_from(
        paths,
        project_root,
        package_dir,
        &source_package_dir,
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
    let files =
        pi_package_files_from_source(project_root, package_dir, enumerate_root, include_missing)?;
    paths.extend(files);
    Ok(())
}

fn pi_package_files_from_source(
    project_root: &Path,
    package_dir: &Path,
    enumerate_root: &Path,
    include_missing: bool,
) -> Result<BTreeSet<PathBuf>> {
    pi_package_files_from_source_with_options(
        project_root,
        package_dir,
        enumerate_root,
        include_missing,
        true,
    )
}

fn pi_package_files_from_source_with_options(
    project_root: &Path,
    package_dir: &Path,
    enumerate_root: &Path,
    include_missing: bool,
    skip_node_modules: bool,
) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    if !enumerate_root.is_dir() {
        return Ok(paths);
    }
    let mut stack = vec![enumerate_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let path = entry.path();
            let file_name = entry.file_name();
            if skip_node_modules && file_name == OsStr::new("node_modules") {
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
                &mut paths,
                project_root,
                package_dir.join(relative_to_package),
                include_missing,
            );
        }
    }
    Ok(paths)
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
            push_abs_if_exists(paths, project_root, path.join("SKILL.md"), include_missing);
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

fn managed_paths_from_git_status(
    seed_paths: &BTreeSet<PathBuf>,
    refreshed: RefreshState,
) -> Result<Vec<PathBuf>> {
    let git = git_project()?;
    let mut owned_deleted_locked_skill_paths = committed_locked_skill_stage_paths(&git)?;
    owned_deleted_locked_skill_paths.extend(
        vendored_locked_skill_stage_paths_without_committed_lock(&git)?,
    );
    let committed_pi_package_paths = committed_pi_package_stage_paths(&git, refreshed)?;
    let mut committed_paths = committed_project_stage_paths(&git)?;
    committed_paths.extend(owned_deleted_locked_skill_paths.iter().cloned());
    committed_paths.extend(committed_pi_package_paths.iter().cloned());
    let owned_exact_paths = owned_exact_status_paths(seed_paths);
    let committed_pi_bin_paths = committed_pi_bin_paths(&git)?;
    let owned_shared_paths = owned_shared_status_paths(seed_paths, &committed_paths);
    let owned_deleted_native_hooks = owned_deleted_native_hook_paths(seed_paths, &committed_paths);
    let owned_cursor_safety_rules = owned_cursor_safety_rule_paths(seed_paths, &committed_paths);
    let owned_opencode_hook_instructions =
        owned_opencode_hook_instruction_paths(seed_paths, &committed_paths);
    let owned_pi_bin_paths =
        owned_pi_bin_paths(seed_paths, &committed_paths, &committed_pi_bin_paths);
    let status_pathspecs = managed_status_pathspecs(
        seed_paths,
        &owned_exact_paths,
        &owned_deleted_locked_skill_paths,
        &committed_pi_package_paths,
        &owned_shared_paths,
        &owned_cursor_safety_rules,
        &owned_opencode_hook_instructions,
        &owned_pi_bin_paths,
    )?;
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
                &owned_exact_paths,
                &owned_deleted_locked_skill_paths,
                &committed_pi_package_paths,
                &owned_shared_paths,
                &owned_deleted_native_hooks,
                &owned_cursor_safety_rules,
                &owned_opencode_hook_instructions,
                &owned_pi_bin_paths,
                status,
            )
        {
            paths.insert(path);
        }
    }
    Ok(paths.into_iter().collect())
}

fn owned_exact_status_paths(seed_paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    seed_paths
        .iter()
        .filter(|path| is_safe_relative_path(path))
        .cloned()
        .collect()
}

fn owned_deleted_native_hook_paths(
    seed_paths: &BTreeSet<PathBuf>,
    committed_paths: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut owned = native_hook_paths_from(seed_paths);
    owned.extend(native_hook_paths_from(committed_paths));
    owned
}

fn owned_cursor_safety_rule_paths(
    seed_paths: &BTreeSet<PathBuf>,
    committed_paths: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut owned = cursor_safety_rule_paths_from(seed_paths);
    owned.extend(cursor_safety_rule_paths_from(committed_paths));
    owned
}

fn owned_opencode_hook_instruction_paths(
    seed_paths: &BTreeSet<PathBuf>,
    committed_paths: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut owned = opencode_hook_instruction_paths_from(seed_paths);
    owned.extend(opencode_hook_instruction_paths_from(committed_paths));
    owned
}

fn owned_pi_bin_paths(
    seed_paths: &BTreeSet<PathBuf>,
    committed_paths: &BTreeSet<PathBuf>,
    committed_pi_bin_paths: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut owned = pi_bin_paths_from(seed_paths);
    owned.extend(pi_bin_paths_from(committed_paths));
    owned.extend(committed_pi_bin_paths.iter().cloned());
    owned
}

fn committed_project_lock(git: &GitProject) -> Result<Option<LockFile>> {
    let project_lock_path = project_to_git_path(git, Path::new(".vstack-lock.json"));
    let Some(project_lock_path) = project_lock_path.to_str() else {
        return Ok(None);
    };
    let spec = format!("HEAD:{project_lock_path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(&git.root)
        .args(["show", &spec])
        .output()
        .context("reading committed vstack lock for managed ownership")?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(serde_json::from_slice::<LockFile>(&output.stdout).ok())
}

fn committed_project_stage_paths(git: &GitProject) -> Result<BTreeSet<PathBuf>> {
    let Some(lock) = committed_project_lock(git)? else {
        return Ok(BTreeSet::new());
    };
    Ok(project_stage_paths(&lock, true)?.into_iter().collect())
}

fn committed_locked_skill_stage_paths(git: &GitProject) -> Result<BTreeSet<PathBuf>> {
    let Some(lock) = committed_project_lock(git)? else {
        return Ok(BTreeSet::new());
    };
    committed_locked_skill_stage_paths_from_lock(git, &lock)
}

fn vendored_locked_skill_stage_paths_without_committed_lock(
    git: &GitProject,
) -> Result<BTreeSet<PathBuf>> {
    if committed_project_lock(git)?.is_some() {
        return Ok(BTreeSet::new());
    }
    if !git_head_exists(git)? {
        return Ok(BTreeSet::new());
    }
    let lock_path = config::lock_file_path(false);
    if !lock_path.exists() {
        return Ok(BTreeSet::new());
    }
    let lock = LockFile::load(&lock_path)?;
    let project_root = config::project_root();
    let mut dirs = BTreeSet::new();
    for entry in lock
        .entries
        .values()
        .filter(|entry| entry.kind == ItemKind::Skill)
    {
        crate::path_safety::validate_item_name(&entry.name)
            .with_context(|| format!("unsafe locked skill name {}", entry.name))?;
        for dir in locked_skill_stage_dirs(&project_root, entry) {
            if !locked_skill_dir_has_managed_marker(git, &project_root, &dir)? {
                continue;
            }
            if let Ok(relative) = dir.strip_prefix(&project_root) {
                dirs.insert(relative.to_path_buf());
            }
        }
    }
    git_tracked_project_paths_under(git, &dirs)
}

fn git_head_exists(git: &GitProject) -> Result<bool> {
    let output = git_literal_command()
        .arg("-C")
        .arg(&git.root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .context("checking git HEAD for managed ownership")?;
    Ok(output.status.success())
}

fn locked_skill_dir_has_managed_marker(
    git: &GitProject,
    project_root: &Path,
    dir: &Path,
) -> Result<bool> {
    let marker = dir.join(".vstack-refreshed");
    if std::fs::symlink_metadata(&marker).is_ok() {
        return Ok(true);
    }
    let Ok(relative) = marker.strip_prefix(project_root) else {
        return Ok(false);
    };
    git_head_has_project_path(git, relative)
}

fn committed_locked_skill_stage_paths_from_lock(
    git: &GitProject,
    lock: &LockFile,
) -> Result<BTreeSet<PathBuf>> {
    let project_root = config::project_root();
    let mut dirs = BTreeSet::new();
    for entry in lock
        .entries
        .values()
        .filter(|entry| entry.kind == ItemKind::Skill)
    {
        crate::path_safety::validate_item_name(&entry.name)
            .with_context(|| format!("unsafe locked skill name {}", entry.name))?;
        for dir in locked_skill_stage_dirs(&project_root, entry) {
            if let Ok(relative) = dir.strip_prefix(&project_root) {
                dirs.insert(relative.to_path_buf());
            }
        }
    }
    git_tracked_project_paths_under(git, &dirs)
}

fn committed_pi_package_stage_paths(
    git: &GitProject,
    refreshed: RefreshState,
) -> Result<BTreeSet<PathBuf>> {
    let Some(lock) = committed_project_lock(git)? else {
        return Ok(BTreeSet::new());
    };
    let project_root = config::project_root();
    // Install clears and re-copies the package directory, so after a refresh a
    // tracked file that is gone was removed by vstack — including supporting
    // files no manifest entrypoint names. The ownership set is only needed
    // without a refresh, so it is not computed (and cannot fail) otherwise.
    let refreshed = refreshed == RefreshState::Refreshed;
    let mut dirs = BTreeSet::new();
    let mut owned_paths = BTreeSet::new();
    for entry in lock
        .entries
        .values()
        .filter(|entry| entry.kind == ItemKind::PiExtension)
    {
        let package_dir = crate::pi_extension::checked_pi_package_path(&entry.name, false)?;
        if let Ok(relative) = package_dir.strip_prefix(&project_root) {
            dirs.insert(relative.to_path_buf());
        }
        if refreshed {
            continue;
        }
        if let Some(source_root) = config::resolve_source_path(&entry.source)
            && let Some(source_package_dir) =
                crate::catalog::find_item_path(&source_root, entry.kind, &entry.name)
        {
            owned_paths.extend(pi_package_files_from_source_with_options(
                &project_root,
                &package_dir,
                &source_package_dir,
                true,
                false,
            )?);
        }
        owned_paths.extend(committed_pi_manifest_owned_package_paths(
            git,
            &project_root,
            &package_dir,
        )?);
    }
    let tracked = git_tracked_project_paths_under(git, &dirs)?;
    if refreshed {
        return Ok(tracked);
    }
    Ok(tracked
        .into_iter()
        .filter(|path| owned_paths.contains(path))
        .collect())
}

fn committed_pi_manifest_owned_package_paths(
    git: &GitProject,
    project_root: &Path,
    package_dir: &Path,
) -> Result<BTreeSet<PathBuf>> {
    let package_json = package_dir.join("package.json");
    let Ok(project_package_json) = package_json.strip_prefix(project_root) else {
        return Ok(BTreeSet::new());
    };
    let git_path = project_to_git_path(git, project_package_json);
    let Some(git_path) = git_path.to_str() else {
        return Ok(BTreeSet::new());
    };
    let spec = format!("HEAD:{git_path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(&git.root)
        .args(["show", &spec])
        .output()
        .with_context(|| format!("reading committed Pi package manifest {}", git_path))?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }

    let mut paths = BTreeSet::new();
    paths.insert(project_package_json.to_path_buf());
    for relative in pi_manifest_owned_relative_paths(&output.stdout)? {
        let owned_path = safe_pi_package_manifest_path(&relative)
            .with_context(|| format!("unsafe committed Pi package path {relative}"))?;
        let abs_path = package_dir.join(owned_path);
        if let Ok(project_path) = abs_path.strip_prefix(project_root) {
            paths.insert(project_path.to_path_buf());
        }
    }
    Ok(paths)
}

fn pi_manifest_owned_relative_paths(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).context("parsing committed Pi package manifest")?;
    let mut paths = BTreeSet::new();
    if let Some(pi) = manifest.get("pi") {
        if let Some(extensions) = pi.get("extensions").and_then(serde_json::Value::as_array) {
            for value in extensions {
                if let Some(path) = value.as_str() {
                    paths.insert(path.to_string());
                }
            }
        }
        if let Some(path) = pi.get("appendSystem").and_then(serde_json::Value::as_str) {
            paths.insert(path.to_string());
        }
    }
    match manifest.get("bin") {
        Some(serde_json::Value::String(path)) => {
            paths.insert(path.to_string());
        }
        Some(serde_json::Value::Object(map)) => {
            for value in map.values() {
                if let Some(path) = value.as_str() {
                    paths.insert(path.to_string());
                }
            }
        }
        _ => {}
    }
    Ok(paths)
}

fn safe_pi_package_manifest_path(relative: &str) -> Result<PathBuf> {
    let path = relative.trim_start_matches("./");
    let path = Path::new(path);
    if path.is_absolute() {
        bail!("path must be relative");
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => bail!("path must stay inside the package"),
        }
    }
    Ok(path.to_path_buf())
}

fn git_tracked_project_paths_under(
    git: &GitProject,
    project_paths: &BTreeSet<PathBuf>,
) -> Result<BTreeSet<PathBuf>> {
    if project_paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let top_level_pathspecs: Vec<PathBuf> = project_paths
        .iter()
        .map(|path| project_to_git_path(git, path))
        .collect();
    let output = git_literal_command()
        .arg("-C")
        .arg(&git.root)
        .args(["ls-tree", "-r", "-z", "--name-only", "HEAD", "--"])
        .args(&top_level_pathspecs)
        .output()
        .context("reading committed locked skill paths for managed ownership")?;
    if !output.status.success() {
        bail!("git ls-tree failed while reading committed locked skill paths");
    }
    let mut paths = BTreeSet::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let top_level_path = path_from_git_status_bytes(record);
        let Some(path) = git_to_project_path(git, &top_level_path) else {
            continue;
        };
        if is_safe_relative_path(&path) {
            paths.insert(path);
        }
    }
    Ok(paths)
}

fn git_head_has_project_path(git: &GitProject, project_path: &Path) -> Result<bool> {
    let top_level_path = project_to_git_path(git, project_path);
    let output = git_literal_command()
        .arg("-C")
        .arg(&git.root)
        .args(["ls-tree", "-z", "--name-only", "HEAD", "--"])
        .arg(&top_level_path)
        .output()
        .context("reading committed vstack-managed marker for ownership")?;
    // `git ls-tree` exits 0 with empty output when the path simply is not in
    // HEAD. A non-zero exit is a dependency failure, not an absence answer —
    // reporting it as "no marker" would silently drop managed paths from the
    // staged set. Callers only reach here once a committed lock was read, so
    // HEAD is known to exist.
    if !output.status.success() {
        bail!(
            "git ls-tree failed while reading the committed vstack-managed marker for {}",
            top_level_path.display()
        );
    }
    Ok(!output.stdout.is_empty())
}

fn committed_pi_bin_paths(git: &GitProject) -> Result<BTreeSet<PathBuf>> {
    let Some(lock) = committed_project_lock(git)? else {
        return Ok(BTreeSet::new());
    };
    let project_root = config::project_root();
    let mut paths = BTreeSet::new();
    for entry in lock
        .entries
        .values()
        .filter(|entry| entry.kind == ItemKind::PiExtension)
    {
        let package_json =
            crate::pi_extension::checked_pi_package_path(&entry.name, false)?.join("package.json");
        let Ok(project_path) = package_json.strip_prefix(&project_root) else {
            continue;
        };
        let git_path = project_to_git_path(git, project_path);
        let Some(git_path) = git_path.to_str() else {
            continue;
        };
        let spec = format!("HEAD:{git_path}");
        let output = Command::new("git")
            .arg("-C")
            .arg(&git.root)
            .args(["show", &spec])
            .output()
            .with_context(|| format!("reading committed Pi package manifest {}", git_path))?;
        if !output.status.success() {
            continue;
        }
        for bin_name in pi_bin_names_from_package_manifest(&output.stdout)? {
            crate::pi_extension::validate_pi_bin_name(&bin_name)
                .with_context(|| format!("unsafe committed Pi bin name {bin_name}"))?;
            let bin_path = config::pi_bin_dir(false).join(bin_name);
            if let Ok(relative) = bin_path.strip_prefix(&project_root) {
                paths.insert(relative.to_path_buf());
            }
        }
    }
    Ok(paths)
}

fn pi_bin_names_from_package_manifest(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).context("parsing committed Pi package manifest")?;
    let mut names = BTreeSet::new();
    match manifest.get("bin") {
        Some(serde_json::Value::String(_)) => {
            if let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) {
                names.insert(name.to_string());
            }
        }
        Some(serde_json::Value::Object(map)) => {
            names.extend(map.keys().cloned());
        }
        _ => {}
    }
    Ok(names)
}

fn native_hook_paths_from(paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter(|path| is_native_hook_script_path(path))
        .cloned()
        .collect()
}

fn cursor_safety_rule_paths_from(paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter(|path| is_cursor_safety_rule_path(path))
        .cloned()
        .collect()
}

fn opencode_hook_instruction_paths_from(paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter(|path| is_opencode_hook_instruction_path(path))
        .cloned()
        .collect()
}

fn pi_bin_paths_from(paths: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter(|path| is_pi_bin_path(path))
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

fn is_cursor_safety_rule_path(path: &Path) -> bool {
    let Some(path_str) = path.to_str() else {
        return false;
    };
    path.extension().is_some_and(|extension| extension == "mdc")
        && path_str.starts_with(".cursor/rules/safety-")
}

fn is_opencode_hook_instruction_path(path: &Path) -> bool {
    let Some(path_str) = path.to_str() else {
        return false;
    };
    path.extension().is_some_and(|extension| extension == "md")
        && path_str.starts_with(".opencode/instructions/vstack-hook-")
}

fn is_pi_bin_path(path: &Path) -> bool {
    let mut components = path.components();
    if components
        .next()
        .is_none_or(|component| component.as_os_str() != ".pi")
    {
        return false;
    }
    if components
        .next()
        .is_none_or(|component| component.as_os_str() != "bin")
    {
        return false;
    }
    components.next().is_some() && components.next().is_none()
}

fn owned_shared_status_paths(
    seed_paths: &BTreeSet<PathBuf>,
    committed_paths: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    seed_paths
        .iter()
        .chain(committed_paths.iter())
        .filter(|path| is_shared_status_path(path))
        .cloned()
        .collect()
}

/// Files vstack owns only part of: it maintains its own entries or
/// marker-delimited blocks inside them and consumers own the rest. Git stages
/// whole files, so these are the paths where an unrelated concurrent edit
/// could otherwise ride along into an automated propagation commit.
const SHARED_CONFIG_PATHS: &[&str] = &[
    ".agents/skill-failure-reporting.md",
    "vstack.toml",
    "vstack.settings.toml",
    ".claude/settings.json",
    ".codex/hooks.json",
    ".codex/config.toml",
    "opencode.json",
    "opencode.jsonc",
    ".pi/settings.json",
    ".pi/.vstack-source.json",
    ".pi/APPEND_SYSTEM.md",
];

/// Project-owned skill files, relative to the project root. Same discovery
/// `project_stage_paths` uses, so the guard covers exactly the paths staging
/// would otherwise pass to `git add -A`.
fn project_owned_skill_paths() -> Result<Vec<PathBuf>> {
    let lock_path = config::lock_file_path(false);
    if !lock_path.exists() {
        return Ok(Vec::new());
    }
    let lock = LockFile::load(&lock_path)?;
    let project_root = config::project_root();
    let mut paths = BTreeSet::new();
    push_project_owned_skill_paths(&mut paths, &project_root, &lock, false)?;
    Ok(paths.into_iter().collect())
}

fn is_shared_status_path(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| SHARED_CONFIG_PATHS.contains(&path))
}

/// Shared config files that already carry uncommitted modifications. Read
/// before the refresh runs, so anything listed is a consumer edit propagation
/// did not produce — staging the file would sweep it into the automated PR.
/// Untracked files are not included: a shared file that does not exist in HEAD
/// yet has nothing of the consumer's to lose.
fn dirty_shared_config_paths() -> Result<Vec<PathBuf>> {
    let git = git_project()?;
    // Project-owned skills are shared in the same way: refresh maintains a
    // marker-delimited instruction block inside a file whose other content is
    // the consumer's. They are discovered rather than fixed, so they are
    // collected here instead of listed in SHARED_CONFIG_PATHS.
    let project_owned = project_owned_skill_paths()?;
    let pathspecs: Vec<PathBuf> = SHARED_CONFIG_PATHS
        .iter()
        .map(Path::new)
        .chain(project_owned.iter().map(PathBuf::as_path))
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
        .args(&pathspecs)
        .output()
        .context("running git status for shared vstack-managed config files")?;
    if !output.status.success() {
        bail!("git status failed while inspecting shared vstack-managed config files");
    }
    let mut dirty = Vec::new();
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
        if status[0] == b'R' || status[0] == b'C' {
            skip_next = true;
        }
        let top_level_path = path_from_git_status_bytes(&record[3..]);
        let Some(path) = git_to_project_path(&git, &top_level_path) else {
            continue;
        };
        if (is_shared_status_path(&path) || project_owned.contains(&path)) && !dirty.contains(&path)
        {
            dirty.push(path);
        }
    }
    dirty.sort();
    Ok(dirty)
}

fn managed_status_pathspecs(
    seed_paths: &BTreeSet<PathBuf>,
    owned_exact_paths: &BTreeSet<PathBuf>,
    owned_deleted_locked_skill_paths: &BTreeSet<PathBuf>,
    owned_deleted_pi_package_paths: &BTreeSet<PathBuf>,
    owned_shared_paths: &BTreeSet<PathBuf>,
    owned_cursor_safety_rules: &BTreeSet<PathBuf>,
    owned_opencode_hook_instructions: &BTreeSet<PathBuf>,
    owned_pi_bin_paths: &BTreeSet<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let mut owned_paths = seed_paths.clone();
    owned_paths.extend(owned_exact_paths.iter().cloned());
    owned_paths.extend(owned_deleted_locked_skill_paths.iter().cloned());
    owned_paths.extend(owned_deleted_pi_package_paths.iter().cloned());
    owned_paths.extend(owned_shared_paths.iter().cloned());
    owned_paths.extend(owned_cursor_safety_rules.iter().cloned());
    owned_paths.extend(owned_opencode_hook_instructions.iter().cloned());
    owned_paths.extend(owned_pi_bin_paths.iter().cloned());
    for path in [
        ".vstack-lock.json",
        "vstack.toml",
        "vstack.settings.toml",
        ".claude/hooks",
        ".codex/hooks",
    ] {
        owned_paths.insert(PathBuf::from(path));
    }
    let pathspecs: BTreeSet<PathBuf> = owned_paths
        .into_iter()
        .filter(|path| is_safe_relative_path(path))
        .map(|path| managed_status_scan_pathspec(&path))
        .collect();
    Ok(pathspecs.into_iter().collect())
}

fn managed_status_scan_pathspec(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(first) = components.next().map(|component| component.as_os_str()) else {
        return path.to_path_buf();
    };
    let Some(second) = components.next().map(|component| component.as_os_str()) else {
        return path.to_path_buf();
    };
    match (first, second) {
        (first, second) if first == OsStr::new(".agents") && second == OsStr::new("skills") => {
            Path::new(".agents").join("skills")
        }
        (first, second)
            if first == OsStr::new(".claude")
                && matches!(second.to_str(), Some("agents" | "hooks" | "skills")) =>
        {
            Path::new(".claude").join(second)
        }
        (first, second) if first == OsStr::new(".cursor") && second == OsStr::new("rules") => {
            Path::new(".cursor").join("rules")
        }
        (first, second)
            if first == OsStr::new(".codex")
                && matches!(second.to_str(), Some("agents" | "hooks")) =>
        {
            Path::new(".codex").join(second)
        }
        (first, second)
            if first == OsStr::new(".opencode")
                && matches!(second.to_str(), Some("agents" | "instructions")) =>
        {
            Path::new(".opencode").join(second)
        }
        (first, second)
            if first == OsStr::new(".pi")
                && matches!(second.to_str(), Some("agents" | "bin" | "packages")) =>
        {
            Path::new(".pi").join(second)
        }
        _ => path.to_path_buf(),
    }
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
    owned_exact_paths: &BTreeSet<PathBuf>,
    owned_deleted_locked_skill_paths: &BTreeSet<PathBuf>,
    owned_deleted_pi_package_paths: &BTreeSet<PathBuf>,
    owned_shared_paths: &BTreeSet<PathBuf>,
    owned_deleted_native_hooks: &BTreeSet<PathBuf>,
    owned_cursor_safety_rules: &BTreeSet<PathBuf>,
    owned_opencode_hook_instructions: &BTreeSet<PathBuf>,
    owned_pi_bin_paths: &BTreeSet<PathBuf>,
    status: &[u8],
) -> bool {
    let path = path.components().collect::<PathBuf>();
    if matches!(
        path.to_str(),
        Some(".vstack-lock.json") | Some("vstack.toml") | Some("vstack.settings.toml")
    ) {
        return true;
    }
    if owned_shared_paths.contains(&path) {
        return true;
    }
    if owned_exact_paths.contains(&path) {
        return true;
    }
    if status.contains(&b'D') && owned_deleted_locked_skill_paths.contains(&path) {
        return true;
    }
    if status.contains(&b'D') && owned_deleted_pi_package_paths.contains(&path) {
        return true;
    }
    if path.to_str().is_none() {
        return false;
    }
    owned_pi_bin_paths.contains(&path)
        || (is_cursor_safety_rule_path(&path) && owned_cursor_safety_rules.contains(&path))
        || (is_opencode_hook_instruction_path(&path)
            && owned_opencode_hook_instructions.contains(&path))
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
