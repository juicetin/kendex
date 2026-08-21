//! Computing the desired world: what declaration says should exist on
//! disk, resolved against every source it names.
//!
//! Computed against the manifest that will be on disk once this plan
//! applies. An upstream skill merge rewrites the manifest, and hashes and
//! renderings must reflect that rewrite — otherwise the very next audit
//! reads the merged manifest and calls a clean install stale. The merge is
//! idempotent, so recomputing against it converges in one repeat.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::env::Env;
use crate::error::Result;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::model::{HarnessId, ItemKind, Scope};
use crate::source::find_item;

use crate::engine::desired::{DesiredState, ItemCtx};
use crate::engine::desired_item::no_harness_note;
use crate::engine::desired_source::{published_review, read_catalog, resolve_source};
use crate::engine::{PlanOptions, desired_agent, desired_kinds, desired_skill::desired_skill};

/// The desired world, computed against the manifest that will be on disk
/// once this plan applies. An upstream skill merge rewrites the manifest,
/// and hashes and renderings must reflect that rewrite — otherwise the very
/// next audit reads the merged manifest and calls a clean install stale. The
/// merge is idempotent, so recomputing against it converges in one repeat.
pub fn desired_state(
    env: &Env,
    scope: &Scope,
    manifest: &Manifest,
    lock: &Lock,
    options: &PlanOptions,
) -> Result<DesiredState> {
    let first = compute(env, scope, manifest, lock, options)?;
    let Some(merged) = first.manifest_update else {
        return Ok(first);
    };
    let mut second = compute(env, scope, &merged, lock, options)?;
    second.manifest_update = Some(merged);
    Ok(second)
}

fn compute(
    env: &Env,
    scope: &Scope,
    manifest: &Manifest,
    lock: &Lock,
    options: &PlanOptions,
) -> Result<DesiredState> {
    let mut state = DesiredState::default();
    let mut updated_manifest = manifest.clone();
    let mut manifest_changed = false;
    // Everything is planned from the closure — what was declared, what the
    // installed bundles carry, and what those skills require — while the
    // manifest keeps holding only what was chosen.
    let expansion = crate::engine::expansion::expand(env, scope, manifest, &mut state);
    // One parse of each catalog root's reviews file per pass: it is one
    // file, and every item that root carries would otherwise re-read it.
    // Keyed by root, since one declared source resolves to several when
    // its items pin different revisions.
    let mut reviews: BTreeMap<PathBuf, BTreeMap<String, crate::quality::reviews::SafetyReview>> =
        BTreeMap::new();
    let collisions = crate::engine::catalog::Collisions::find(&expansion, &mut state);

    for kind in crate::engine::expansion::PLANNED_KINDS {
        for (name, planned) in expansion.of(kind) {
            let decl = &planned.decl;
            let Some((root, provenance, source_commit)) =
                resolve_source(env, scope, name, decl, manifest, &mut state)?
            else {
                continue;
            };
            let Some((sealed, config)) =
                read_catalog(&root, &provenance, name, &decl.source, &mut state)?
            else {
                continue;
            };
            crate::engine::catalog::notes(&config, &decl.source, &mut state);
            let Some(item_path) = find_item(&sealed, &config, kind, name) else {
                state
                    .notes
                    .push(format!("{name}: not found in source '{}'", decl.source));
                continue;
            };
            state.processed.insert((kind, name.clone()));
            let author_review = published_review(
                &sealed,
                &decl.source,
                &provenance,
                &config,
                kind,
                name,
                &item_path,
                &mut reviews,
                &mut state,
            )?;
            let mut harnesses = planned.harnesses.clone();
            // Every tool this is declared for is one that holds no such kind
            // here. Nothing installs, and silence would read as success.
            if harnesses.is_empty() {
                no_harness_note(kind, name, decl, manifest, &mut state);
            }
            harnesses.retain(|harness| collisions.allows(kind, name, *harness));
            let reasons = reasons_for(kind, name, &harnesses, &expansion);
            let ctx = ItemCtx {
                env,
                scope,
                manifest,
                lock,
                config: &config,
                sealed: &sealed,
                name,
                decl,
                item_path: &item_path,
                provenance: &provenance,
                source_commit: source_commit.as_deref(),
                harnesses,
                reasons: &reasons,
                author_review,
                planned: options.acts_on(kind, name),
            };
            let outcome = match kind {
                ItemKind::Skill => desired_skill(&ctx, &mut state),
                ItemKind::Agent => desired_agent::desired_agent(
                    &ctx,
                    &mut state,
                    &mut updated_manifest,
                    &mut manifest_changed,
                ),
                ItemKind::Hook => desired_kinds::desired_hook(&ctx, &mut state),
                ItemKind::Command => {
                    crate::engine::desired_command::desired_command(&ctx, &mut state)
                }
                ItemKind::McpServer => crate::engine::desired_mcp::desired_mcp(&ctx, &mut state),
                _ => Ok(()),
            };
            match outcome {
                Ok(()) => {}
                // One hostile item must not take the whole scope down: the
                // refused read becomes an unreadable note, and what it
                // already installed stays out of the orphan sweep.
                // "unreadable" is the phrase verify keys on: a refused item
                // must fail verification, never print a green tick.
                Err(crate::error::CoreError::SourceEscape { path, reason }) => {
                    state.unreadable(
                        kind,
                        name,
                        format!(
                            "{name}: unreadable — refused catalog read: {reason} ({})",
                            path.display()
                        ),
                    );
                }
                Err(other) => return Err(other),
            }
        }
    }
    desired_kinds::desired_plugins(env, scope, manifest, &mut state);
    crate::engine::desired_custom_hooks::desired_custom_hooks(env, scope, manifest, &mut state);

    if manifest_changed {
        state.manifest_update = Some(updated_manifest);
    }
    Ok(state)
}

/// Why each of an item's installations is wanted, as the closure derived it.
fn reasons_for(
    kind: ItemKind,
    name: &str,
    harnesses: &[HarnessId],
    expansion: &crate::engine::expansion::Expansion,
) -> BTreeMap<HarnessId, BTreeSet<crate::lock::Reason>> {
    harnesses
        .iter()
        .map(|harness| (*harness, expansion.reasons(kind, name, *harness)))
        .collect()
}
