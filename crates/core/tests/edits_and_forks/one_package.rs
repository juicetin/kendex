//! What a command naming one package may touch. Each of these plans the
//! whole scope carrying that package's permission, so what keeps them
//! honest is measured before the plan and restricted inside it.

use std::fs;

use kendex_core::apply;
use kendex_core::engine::{PlanOptions, plan_scope};
use kendex_core::lock::{load as load_lock, lock_path};
use kendex_core::manifest;
use kendex_core::model::ItemKind;

use super::{commit, declare, skill_file, sync_and_apply, world, write_skill};

// The one fact both discard exits rest on — the CLI's `discard-edits` and
// the app's targeted apply. Each plans the whole scope carrying a
// permission for one package, so this is what stands between "put this
// package back" and "run whatever the scope had waiting".
#[test]
#[allow(clippy::unwrap_used)]
fn edited_here_answers_for_the_package_asked_about() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream gh.");
    write_skill(&w.upstream, "lint", "Upstream lint.");
    commit(&w.upstream, "one");
    declare(
        &w,
        "[skills.gh]\nsource = \"cat\"\n\n[skills.lint]\nsource = \"cat\"\n",
    );
    sync_and_apply(&w);
    let edited = |name: &str| {
        kendex_core::engine::edited_here(&w.env, &w.scope, ItemKind::Skill, name).unwrap()
    };

    assert!(!edited("gh"), "nothing edited yet");
    fs::write(skill_file(&w), "my gh edit").unwrap();
    assert!(edited("gh"), "the edit this exit is for");
    assert!(!edited("lint"), "a sibling's clean copy is not this edit");
    assert!(!edited("nope"), "nothing is declared by that name");
}

// A plan is always the scope's. `only_names` is what keeps a command that
// names one package from installing, updating and re-rendering the rest of
// the scope under it — while the records of those others carry forward, so
// the lock this plan writes still knows what is installed.
#[test]
#[allow(clippy::unwrap_used)]
fn a_plan_for_one_package_leaves_every_other_declaration_alone() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream gh.");
    write_skill(&w.upstream, "lint", "Upstream lint.");
    write_skill(&w.upstream, "notes", "Upstream notes.");
    commit(&w.upstream, "one");
    declare(
        &w,
        "[skills.gh]\nsource = \"cat\"\n\n[skills.lint]\nsource = \"cat\"\n",
    );
    sync_and_apply(&w);
    fs::write(skill_file(&w), "my gh edit").unwrap();
    // Work the scope has waiting that this command was not asked about.
    declare(
        &w,
        "[skills.gh]\nsource = \"cat\"\n\n[skills.lint]\nsource = \"cat\"\n\n[skills.notes]\nsource = \"cat\"\n",
    );

    let manifest = manifest::load_for_mutation(&manifest::manifest_path(&w.env, &w.scope))
        .unwrap()
        .unwrap();
    let lock = load_lock(&lock_path(&w.env, &w.scope)).unwrap();
    let one = (ItemKind::Skill, "gh".to_owned());
    let report = plan_scope(
        &w.env,
        &w.scope,
        &manifest,
        &lock,
        &PlanOptions {
            overwrite_edited_names: Some(vec![one.clone()]),
            only_names: Some(vec![one]),
            ..Default::default()
        },
    )
    .unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();

    assert!(
        fs::read_to_string(skill_file(&w))
            .unwrap()
            .contains("Upstream gh."),
        "the package named came back"
    );
    assert!(
        !w.home.join("app/.agents/skills/notes/SKILL.md").exists(),
        "a package nobody asked about was installed"
    );
    let after = load_lock(&lock_path(&w.env, &w.scope)).unwrap();
    assert!(
        after.entries.values().any(|entry| entry.name == "lint"),
        "the record of an untouched install was dropped: {after:?}"
    );
}
