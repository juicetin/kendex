//! The manifest write a plan carries, and whether it already carries one.
//! A plan writes the manifest at most once: the write binds to the bytes
//! it read, so a second write's precondition is bytes the first one has
//! already replaced, and it could never run.

use crate::apply::{Op, PlannedOp};
use crate::env::Env;
use crate::error::Result;
use crate::manifest::{self, Manifest};
use crate::model::Scope;

use super::PlanOptions;
use super::desired;
use super::scope_writes::{plan_repo_move_write, plan_schema_upgrade};

/// The plan's one manifest write, when anything needs it: skills an agent
/// gained upstream or a review of findings this run was asked to record
/// take the full serialized write — or, with neither, the repository move
/// or the schema upgrade lands as a surgical text edit that keeps the
/// user's comments and formatting. One write whatever put it there: a
/// second manifest write could never run, its precondition binds to the
/// bytes the first one replaces. The description names the biggest cause;
/// the rest ride along in the same bytes.
pub(super) fn plan_manifest_write(
    env: &Env,
    scope: &Scope,
    repo_moved: bool,
    manifest: &Manifest,
    state: &desired::DesiredState,
    options: &PlanOptions,
    ops: &mut Vec<PlannedOp>,
) -> Result<()> {
    let Some(update) = &state.manifest_update else {
        if repo_moved {
            return plan_repo_move_write(env, scope, manifest, options, ops);
        }
        if manifest.schema < manifest::MANIFEST_SCHEMA {
            plan_schema_upgrade(env, scope, manifest, options, ops)?;
        }
        return Ok(());
    };
    let path = manifest::manifest_path(env, scope);
    let pre = options.manifest_pre(&path)?;
    let mut updated = update.clone();
    updated.schema = manifest::MANIFEST_SCHEMA;
    let granted = updated.safety_overrides != manifest.safety_overrides;
    ops.push(PlannedOp {
        description: match (repo_moved, granted) {
            (true, _) => crate::repo_move::MOVE_DESCRIPTION.into(),
            (false, true) => "Update kendex.toml with the safety findings you accepted".into(),
            (false, false) => "Add new catalog skills to kendex.toml".into(),
        },
        op: Op::WriteManifest {
            pre,
            path,
            manifest: Box::new(updated),
        },
    });
    Ok(())
}

/// Whether a plan already persists the manifest — the full serialized
/// write, or the repository move's surgical text edit. A caller about to
/// insert its own save must count both: a second write to the same file
/// binds to bytes the first one replaces and could never run.
/// The manifest a plan puts on disk, where it plans one.
///
/// A caller that handed a manifest in cannot assume that is what lands:
/// creating the file seeds it, saving names a custom hook that arrived
/// without one, and the planner itself can record what an agent's mapping
/// gained upstream. Asking the plan what it will write is one question
/// that covers all of those, and any later one — the alternative is a
/// list of normalizations that has to be kept complete by hand.
pub fn written_manifest(ops: &[PlannedOp]) -> Option<&Manifest> {
    ops.iter().find_map(|op| match &op.op {
        Op::WriteManifest { manifest, .. } => Some(manifest.as_ref()),
        _ => None,
    })
}

/// Whether this plan already writes the manifest, in any of the shapes
/// that counts as writing it.
///
/// Two of them are surgical file writes rather than a whole-manifest op —
/// the rename generation's, and the schema upgrade's — and both bind their
/// precondition to the bytes on disk. Missed here, the caller adds a whole
/// write of its own bound to those same bytes, the first moves them, and
/// the second is refused as stale after the file has already changed.
pub fn persists_manifest(ops: &[PlannedOp]) -> bool {
    let surgical = [
        crate::repo_move::MOVE_DESCRIPTION.to_owned(),
        super::scope_writes::schema_upgrade_description(),
    ];
    ops.iter().any(|op| {
        matches!(op.op, Op::WriteManifest { .. })
            || (surgical.contains(&op.description) && matches!(op.op, Op::WriteFile { .. }))
    })
}
