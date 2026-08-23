//! Editing a fork's own bytes: what is measured, what is reported, and the
//! one exit left once keeping it as your own is already done.

use super::*;

#[test]
#[allow(clippy::unwrap_used)]
fn a_fork_edited_by_hand_says_so_instead_of_reading_as_untouched() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream.");
    commit(&w.upstream, "one");
    declare(&w, "[skills.gh]\nsource = \"cat\"\n");
    sync_and_apply(&w);
    fs::write(
        skill_file(&w),
        "---\nname: gh\ndescription: mine\n---\nMy fork.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Skill, "gh", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();

    // A fork is the one local source that gets a row, so its row is the
    // only place this fact can be told from.
    let clean = kendex_core::package::updates::updates(&w.env, &w.scope)
        .unwrap()
        .rows;
    let gh = clean.iter().find(|row| row.name == "gh").unwrap();
    assert!(!gh.blocked_by_local_edit, "{gh:?}");

    fs::write(skill_file(&w), "edited after forking").unwrap();
    let rows = kendex_core::package::updates::updates(&w.env, &w.scope)
        .unwrap()
        .rows;
    let gh = rows.iter().find(|row| row.name == "gh").unwrap();
    assert!(gh.forked, "{gh:?}");
    assert!(
        gh.blocked_by_local_edit,
        "an edited fork must not read as untouched: {gh:?}"
    );
    assert_eq!(gh.edited_harnesses, vec![HarnessId::Claude], "{gh:?}");
}

/// The measurement's own fail-open. With the fork's copy gone from the
/// local source, the plan can render nothing to compare the installed files
/// against and skips the item with a note — and a reader taking that
/// silence for cleanliness reports an edited place as untouched, which is
/// the fault this whole area exists to prevent.
#[test]
#[allow(clippy::unwrap_used)]
fn a_fork_whose_copy_is_gone_still_says_its_files_were_edited() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream.");
    commit(&w.upstream, "one");
    declare(&w, "[skills.gh]\nsource = \"cat\"\n");
    sync_and_apply(&w);
    fs::write(
        skill_file(&w),
        "---\nname: gh\ndescription: mine\n---\nMy fork.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Skill, "gh", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();

    fs::write(skill_file(&w), "edited after forking").unwrap();
    // And now the copy the fork was made into is gone — the source the
    // plan would have rendered from cannot answer for it.
    let own = w
        .home
        .join("app")
        .join(kendex_core::rename::LOCAL_SOURCE_DIR)
        .join("skills")
        .join("gh");
    assert!(
        own.is_dir(),
        "the fork's own copy is where the discard reads"
    );
    fs::remove_dir_all(&own).unwrap();

    let rows = kendex_core::package::updates::updates(&w.env, &w.scope)
        .unwrap()
        .rows;
    let gh = rows.iter().find(|row| row.name == "gh").unwrap();
    assert!(
        gh.blocked_by_local_edit,
        "an unmeasurable place read as untouched: {gh:?}"
    );
    assert_eq!(gh.edited_harnesses, vec![HarnessId::Claude], "{gh:?}");
    // And the exit it offers matches: there is nothing left to put back.
    assert!(!gh.can_discard, "{gh:?}");
}

#[test]
#[allow(clippy::unwrap_used)]
fn editing_your_own_fork_is_not_drift_and_offers_no_second_fork() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream.");
    commit(&w.upstream, "one");
    declare(&w, "[skills.gh]\nsource = \"cat\"\n");
    sync_and_apply(&w);
    fs::write(
        skill_file(&w),
        "---\nname: gh\ndescription: mine\n---\nMine.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Skill, "gh", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();

    // Edit the fork's own bytes. Keeping it as a fork is already done, so
    // the one exit left is putting the kept copy back — and every reader of
    // this state has to name that same one.
    fs::write(skill_file(&w), "edited after forking").unwrap();
    let held = audit(&w.env, &w.scope).unwrap();
    let row = held
        .drift
        .iter()
        .find(|row| row.name == "gh")
        .unwrap_or_else(|| panic!("{held:?}"));
    assert!(row.detail.contains("your own copy"), "{row:?}");
    assert!(!row.detail.contains("keep it as a fork"), "{row:?}");

    kendex_core::drift::snapshot::record(&w.env, &w.scope).unwrap();
    let checked = kendex_core::drift::report::check(&w.env, std::slice::from_ref(&w.scope));
    let text = kendex_core::drift::report::render_plain(&checked);
    // The report and the engine agree: one state, said once, with the exit
    // that exists. A silent `check` beside a permanent badge in the app is
    // the shape this closes.
    assert_eq!(
        checked.status,
        kendex_core::drift::report::CheckStatus::Drift,
        "{text}"
    );
    assert!(text.contains("re-render from your own copy"), "{text}");
    assert!(!text.contains("kendex fork"), "{text}");
    // The printed fix has to be the command that performs the exit the line
    // names, and only for the package the line is about. A bare `kendex
    // refresh` holds the edit and prints "up to date" over it; the
    // scope-wide `refresh --discard-edits` resolves this line by discarding
    // every other hand-edited package in the scope. render_plain is what an
    // agent pastes, so the narrow spelling is the only safe one to print.
    assert!(
        text.contains("fix: kendex discard-edits skill gh"),
        "{text}"
    );
    assert!(!text.contains("refresh --discard-edits"), "{text}");
    let row = updates::updates(&w.env, &w.scope).unwrap().rows;
    let row = row.iter().find(|row| row.name == "gh").unwrap();
    assert!(row.can_discard, "the exit the notice offers: {row:?}");
    assert!(row.forkable_harness.is_none(), "{row:?}");

    // And running that exit resolves the state, by the option the app's own
    // button sends: apply_discard_edits names the item rather than setting
    // the scope-wide flag, and removal.rs reads only the flag.
    let manifest = manifest::load_for_mutation(&manifest::manifest_path(&w.env, &w.scope))
        .unwrap()
        .unwrap();
    let lock = load_lock(&lock_path(&w.env, &w.scope)).unwrap();
    let discarded = plan_scope(
        &w.env,
        &w.scope,
        &manifest,
        &lock,
        &PlanOptions {
            overwrite_edited_names: Some(vec![(ItemKind::Skill, "gh".to_owned())]),
            ..Default::default()
        },
    )
    .unwrap();
    apply::execute(&w.env, &discarded.plan, None).unwrap();
    kendex_core::drift::snapshot::record(&w.env, &w.scope).unwrap();
    assert_eq!(
        kendex_core::drift::report::check(&w.env, std::slice::from_ref(&w.scope)).status,
        kendex_core::drift::report::CheckStatus::Clean,
        "the printed fix has to leave `check` clean"
    );

    // And forking again would write a fresh provenance over the recorded
    // one, whose source and commit a local declaration cannot supply.
    let again = fork::fork(&w.env, &w.scope, ItemKind::Skill, "gh", HarnessId::Claude);
    assert!(
        matches!(
            again,
            Err(kendex_core::error::CoreError::AlreadyForked { .. })
        ),
        "{again:?}"
    );
    let manifest = manifest::load_for_mutation(&manifest::manifest_path(&w.env, &w.scope))
        .unwrap()
        .unwrap();
    let recorded = &manifest.forks[&ItemKind::Skill]["gh"];
    assert_eq!(recorded.source, "cat", "{recorded:?}");
    assert!(recorded.repo.is_some(), "{recorded:?}");
}

#[test]
#[allow(clippy::unwrap_used)]
fn discarding_an_edit_to_a_fork_restores_the_forks_own_bytes() {
    let w = world();
    write_skill(&w.upstream, "gh", "Upstream.");
    commit(&w.upstream, "one");
    declare(&w, "[skills.gh]\nsource = \"cat\"\n");
    sync_and_apply(&w);
    fs::write(
        skill_file(&w),
        "---\nname: gh\ndescription: mine\n---\nMine.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Skill, "gh", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();
    let kept = fs::read_to_string(skill_file(&w)).unwrap();

    fs::write(skill_file(&w), "edited after forking").unwrap();
    let manifest = manifest::load_for_mutation(&manifest::manifest_path(&w.env, &w.scope))
        .unwrap()
        .unwrap();
    let lock = load_lock(&lock_path(&w.env, &w.scope)).unwrap();
    let discarded = plan_scope(
        &w.env,
        &w.scope,
        &manifest,
        &lock,
        &PlanOptions {
            overwrite_edited: true,
            ..Default::default()
        },
    )
    .unwrap();
    apply::execute(&w.env, &discarded.plan, None).unwrap();

    // The fork's own copy is in the local source, so there is something to
    // put back — the exit the row must offer.
    assert_eq!(fs::read_to_string(skill_file(&w)).unwrap(), kept);
}
