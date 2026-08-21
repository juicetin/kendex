use clap::Args;

use kendex_core::engine::{PlanOptions, edited_here, plan_scope};
use kendex_core::env::Env;
use kendex_core::lock::{load as load_lock, lock_path};
use kendex_core::manifest::{load_for_mutation, manifest_path};

use super::pin::parse_kind;
use super::{CliResult, resolve_scopes, say};
use crate::scope::ScopeFilter;

/// Put one package's declared content back over the edits made to its
/// installed files — the other exit beside `fork`, and the narrow one.
///
/// `refresh --discard-edits` is the whole scope: run it to resolve one
/// package and every other hand-edited package in that scope loses its
/// edits in the same pass. That is why the drift report names this per
/// package, and why the app has only ever offered the targeted apply.
#[derive(Args)]
pub struct DiscardArgs {
    /// agent | skill
    kind: String,
    name: String,
    #[arg(short = 'g', long)]
    global: bool,
    /// project | global (default project)
    #[arg(long)]
    scope: Option<String>,
}

pub fn run(env: &Env, args: DiscardArgs) -> CliResult {
    let kind = parse_kind(&args.kind)?;
    let filter = ScopeFilter::resolve(args.scope.as_deref(), args.global, ScopeFilter::Project)?;
    let scope = resolve_scopes(env, filter)?.remove(0);
    let manifest = load_for_mutation(&manifest_path(env, &scope))?.ok_or("no manifest")?;
    // A name this scope never declared is a mistake worth saying out loud,
    // not a quiet success that ran a scope's pending work under it.
    if !manifest.declared(kind).contains_key(&args.name) {
        return Err(format!(
            "no {} named '{}' is declared in this scope",
            kind.name(),
            args.name
        )
        .into());
    }
    // The plan below is the scope's, and the permission it carries is this
    // package's alone: with no edit to overwrite, the permission does
    // nothing and executing would apply whatever else the scope had
    // pending under a line saying this package was restored.
    if !edited_here(env, &scope, kind, &args.name)? {
        say(&format!(
            "{} '{}' has no edits to discard — nothing was applied",
            kind.name(),
            args.name
        ));
        return Ok(());
    }
    let lock = load_lock(&lock_path(env, &scope))?;
    let report = plan_scope(
        env,
        &scope,
        &manifest,
        &lock,
        &PlanOptions {
            overwrite_edited_names: Some(vec![(kind, args.name.clone())]),
            ..PlanOptions::default()
        },
    )?;
    for op in &report.plan.ops {
        say(&format!("  - {}", op.description));
    }
    let outcome = kendex_core::apply::execute(env, &report.plan, None)?;
    say(&format!(
        "{} '{}': {} change(s) — its declared content is back",
        kind.name(),
        args.name,
        outcome.applied
    ));
    Ok(())
}
