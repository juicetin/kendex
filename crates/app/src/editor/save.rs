//! The Customize tab's whole-manifest read and write.
//!
//! Every other write in the app is a targeted operation that loads, changes
//! and saves in one breath. This one hands a person the whole file, waits
//! while they type, and writes all of it back — so it is the one write that
//! can put an older file over a newer one, and the only one that carries
//! the base of the file its copy came from to stop that.

use kendex_core::apply::{self, Op, PlannedOp, Pre};
use kendex_core::engine::{self, PlanOptions, ops};
use kendex_core::lock::{load as load_lock, lock_path};
use kendex_core::manifest::{self, Finding, Manifest};
use kendex_core::model::Scope;
use serde::Serialize;
use specta::Type;

use super::env;
use crate::audit::{AuditView, view};

/// A place's manifest and what the file it came from was at that moment.
/// One value, because a copy without its base cannot be written back
/// safely, and the two read apart could describe different files.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManifestRead {
    /// Absent where the place has no manifest yet — the editor still opens,
    /// on an empty one.
    pub manifest: Option<Manifest>,
    /// `None` where nothing was there, which is itself a base: a write
    /// carrying it says "there was no file", and is refused if there is
    /// one now.
    pub base: Option<String>,
}

/// Why a whole-manifest write did not happen. Refusing is a normal answer
/// here, not a failure, so it is a shape the editor can act on rather than
/// a message it would have to recognise by its words.
#[derive(Serialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WriteRefused {
    /// The file is no longer the one this copy was read from. Something
    /// else wrote it — a fork, a hold, a dismissal, an install — and
    /// writing this copy would put that back.
    Stale,
    Failed {
        message: String,
    },
}

impl From<String> for WriteRefused {
    fn from(message: String) -> WriteRefused {
        WriteRefused::Failed { message }
    }
}

/// A whole-manifest write that landed, and what the file is now: the base
/// for the next write from the same copy, so saving twice in a row does not
/// have to wait for a re-read in between.
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManifestWritten {
    pub view: AuditView,
    pub base: Option<String>,
}

#[tauri::command(async)]
#[specta::specta]
pub fn get_manifest(scope: Scope) -> Result<ManifestRead, String> {
    let env = env()?;
    let path = manifest::manifest_path(&env, &scope);
    // One read for both halves: read apart, the manifest could be the old
    // file's and the base the new one's, and the write that follows would
    // be accepted over the writer in between.
    let (manifest, base) = manifest::read_for_mutation(&path).map_err(|e| e.to_string())?;
    Ok(ManifestRead { manifest, base })
}

/// Validate an edited manifest the way a hand-written file is validated, so
/// the editor rejects exactly the same things — fix strings included.
fn check(manifest: &Manifest) -> Result<(), String> {
    let text = toml::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let findings = manifest::validate(&table);
    if findings.is_empty() {
        return Ok(());
    }
    Err(findings
        .iter()
        .map(Finding::to_string)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The editor can create the first manifest for a scope, and first creation
/// is where the default source is seeded — skipping it here would drop it
/// for good, since later reconciliation never re-adds it.
fn on_first_creation(mut manifest: Manifest, seed: Manifest) -> Manifest {
    if manifest.sources.is_empty() {
        manifest.sources = seed.sources;
        if manifest.install.harnesses.is_empty() {
            manifest.install.harnesses = seed.install.harnesses;
        }
    }
    manifest
}

/// Write an edited manifest and reconcile the scope to it.
///
/// `base` is what the file was when this copy was read. A whole manifest
/// goes back with every save, so a copy read before something else wrote
/// the file would put that back — and the caller cannot be relied on to
/// notice: the app tells the editor about every such write, and a caller
/// that forgets to says nothing at all. Refusing here needs no caller to
/// remember anything.
#[tauri::command(async)]
#[specta::specta]
pub fn update_manifest(
    scope: Scope,
    manifest: Manifest,
    base: Option<String>,
) -> Result<ManifestWritten, WriteRefused> {
    let env = env()?;
    let path = manifest::manifest_path(&env, &scope);
    let held = match &base {
        Some(hash) => Pre::HashIs { hash: hash.clone() },
        None => Pre::Absent,
    };
    if manifest::check_base(&path, base.as_deref()).is_err() {
        return Err(WriteRefused::Stale);
    }
    let mut manifest = match manifest::load_for_mutation(&path).map_err(|e| e.to_string())? {
        Some(_) => manifest,
        None => on_first_creation(
            manifest,
            ops::manifest_for_mutation(&env, &scope).map_err(|e| e.to_string())?,
        ),
    };
    // A custom hook's name is its identity everywhere downstream; saving is
    // when a derived one stops being derived.
    kendex_core::hook::name_custom_hooks(&mut manifest);
    check(&manifest)?;
    let lock = load_lock(&lock_path(&env, &scope)).map_err(|e| e.to_string())?;
    let mut report = engine::plan_scope(&env, &scope, &manifest, &lock, &PlanOptions::default())
        .map_err(|e| e.to_string())?;
    let persisted = engine::persists_manifest(&report.plan.ops);
    if !persisted {
        report.plan.ops.insert(
            0,
            PlannedOp {
                description: "Save kendex.toml".into(),
                op: Op::WriteManifest {
                    pre: held.clone(),
                    path: path.clone(),
                    manifest: Box::new(manifest),
                },
            },
        );
    }
    // Whoever planned the write, it is the copy on screen that is being
    // written, so every write of this file binds to the file that copy came
    // from. A plan that carries its own manifest write — a schema upgrade,
    // a repository move, skills an agent gained upstream — binds to what
    // the file was when the plan ran, which would accept a writer that
    // landed between the check above and here.
    report.plan.bind_writes(&path, &held);
    apply::execute(&env, &report.plan, None).map_err(|e| e.to_string())?;
    Ok(ManifestWritten {
        view: view(&env, &scope),
        base: manifest::base(&path).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kendex_core::manifest::{
        DEFAULT_SOURCE_NAME, DEFAULT_SOURCE_REPO, ItemDecl, MANIFEST_SCHEMA, SourceDecl,
    };
    use kendex_core::model::HarnessId;
    use std::collections::BTreeMap;

    fn manifest() -> Manifest {
        Manifest {
            schema: MANIFEST_SCHEMA,
            sources: BTreeMap::from([(
                DEFAULT_SOURCE_NAME.to_owned(),
                SourceDecl {
                    repo: Some(DEFAULT_SOURCE_REPO.to_owned()),
                    path: None,
                    rev: None,
                    enabled: true,
                },
            )]),
            ..Manifest::default()
        }
    }

    #[test]
    fn customization_tables_pass_the_same_check_a_file_gets() {
        let mut edited = manifest();
        edited
            .agent_skills
            .insert("orch".to_owned(), vec!["github".to_owned()]);
        edited
            .agent_launch_instructions
            .insert("all".to_owned(), "read the plan".to_owned());
        edited.custom_hooks.push(kendex_core::manifest::CustomHook {
            name: None,
            event: "PreToolUse".to_owned(),
            matcher: Some("Bash".to_owned()),
            command: "./guard.sh".to_owned(),
            description: None,
            timeout: None,
            harnesses: None,
            enabled: true,
            agents: kendex_core::manifest::HookAgents::One("all".to_owned()),
        });
        assert_eq!(check(&edited), Ok(()));
    }

    #[test]
    fn creating_a_manifest_here_still_seeds_the_default_source() {
        let seeded = on_first_creation(
            Manifest {
                schema: MANIFEST_SCHEMA,
                ..Manifest::default()
            },
            manifest::seed(&[HarnessId::Claude]),
        );
        assert!(seeded.sources.contains_key("kendex"));
        assert_eq!(seeded.install.harnesses, [HarnessId::Claude]);

        let declared = on_first_creation(manifest(), manifest::seed(&[HarnessId::Pi]));
        assert_eq!(declared.sources.len(), 1);
        assert!(declared.install.harnesses.is_empty());
    }

    #[test]
    fn rejected_edits_come_back_with_their_fix_string() {
        let mut edited = manifest();
        edited
            .skills
            .insert("github".to_owned(), ItemDecl::from_source("gone"));
        let error = check(&edited).expect_err("undeclared source must be rejected");
        assert!(error.contains("skills.github"), "{error}");
        assert!(error.contains("fix: declare [sources.gone]"), "{error}");
    }
}
