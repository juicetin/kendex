//! When the way back is offered at all. A discard re-renders the package
//! from the copy this place kept, so the button belongs on a row only where
//! that copy can actually produce one — a path that exists is not the same
//! answer, and a row offering an exit that refuses is worse than a row
//! offering none.

use super::*;

#[test]
#[allow(clippy::unwrap_used)]
fn a_fork_whose_own_copy_is_gone_offers_no_discard() {
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
    fs::write(skill_file(&w), "edited after forking").unwrap();

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "gh")
            .unwrap()
    };
    assert!(row(&w).can_discard, "the copy is there to put back");

    // Break the local source in the three ways a path check reads as fine.
    // Discarding re-renders from it, so each of these would offer the
    // button and then refuse.
    let Scope::Project { root } = &w.scope else {
        unreachable!("the test world is a project")
    };
    let skill = root.join(".kendex-local/skills/gh");

    // The directory outlives the file the catalog reads.
    fs::remove_file(skill.join("SKILL.md")).unwrap();
    assert!(skill.is_dir(), "the path is still there");
    assert!(!row(&w).can_discard, "{:?}", row(&w));

    // A symlink where content belongs: the source store holds neither.
    std::os::unix::fs::symlink(
        w.upstream.join("skills/gh/SKILL.md"),
        skill.join("SKILL.md"),
    )
    .unwrap();
    assert!(
        skill.join("SKILL.md").exists(),
        "it resolves, and is a link"
    );
    assert!(!row(&w).can_discard, "{:?}", row(&w));

    // And the artifact replaced by a directory of the same name.
    fs::remove_file(skill.join("SKILL.md")).unwrap();
    fs::create_dir(skill.join("SKILL.md")).unwrap();
    assert!(!row(&w).can_discard, "{:?}", row(&w));

    fs::remove_dir_all(&skill).unwrap();
    let gone = row(&w);
    assert!(!gone.can_discard, "nothing left to put back: {gone:?}");
    assert!(gone.forked, "still this place's own copy: {gone:?}");
}

// The path check answered for one file; the discard reads the whole tree
// through the sealed source. A fork whose tree that read refuses is a fork
// with no discard, however sound its own SKILL.md looks.
#[test]
#[allow(clippy::unwrap_used)]
fn a_fork_the_sealed_source_cannot_collect_offers_no_discard() {
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
    fs::write(skill_file(&w), "edited after forking").unwrap();

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "gh")
            .unwrap()
    };
    assert!(row(&w).can_discard, "the copy is there to put back");
    let Scope::Project { root } = &w.scope else {
        unreachable!("the test world is a project")
    };
    let skill = root.join(".kendex-local/skills/gh");

    // A descendant symlink: SKILL.md is a sound file, and the tree beside
    // it is what the render refuses to read through.
    let linked = skill.join("reference.md");
    std::os::unix::fs::symlink(w.upstream.join("skills/gh/SKILL.md"), &linked).unwrap();
    assert!(
        skill.join("SKILL.md").is_file(),
        "the artifact the old check read is still sound"
    );
    assert!(!row(&w).can_discard, "{:?}", row(&w));
    fs::remove_file(&linked).unwrap();
    assert!(
        row(&w).can_discard,
        "and back, with the tree readable again"
    );

    // And a tree nested past what a catalog tree may be: every file is
    // sound, and collecting them is refused.
    let mut deep = skill.join("notes");
    for _ in 0..18 {
        deep = deep.join("d");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("note.md"), "deep").unwrap();
    assert!(!row(&w).can_discard, "{:?}", row(&w));
}

/// The same rule for an agent, where reading and rendering come apart. The
/// planner parses the file it reads, and a copy that is readable but has no
/// usable frontmatter is refused there as unmeasured — so a discard offered
/// on the strength of the read alone names a way out that cannot run.
#[test]
#[allow(clippy::unwrap_used)]
fn a_forked_agent_whose_copy_will_not_parse_offers_no_discard() {
    let w = world();
    let dir = w.upstream.join("agents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("rev.md"),
        "---\nname: rev\ndescription: agent rev\n---\nAgent body.\n",
    )
    .unwrap();
    commit(&w.upstream, "one");
    declare(&w, "[agents.rev]\nsource = \"cat\"\n");
    sync_and_apply(&w);

    // Edited, and still an agent — so the copy the fork keeps is one that
    // renders. A fork of an edit that destroyed the frontmatter has nothing
    // to put back either, which is the same rule a step earlier.
    let installed = w.home.join("app/.claude/agents/rev.md");
    fs::write(
        &installed,
        "---\nname: rev\ndescription: mine now\n---\nMy agent.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Agent, "rev", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();
    fs::write(&installed, "edited after forking").unwrap();

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "rev")
            .unwrap()
    };
    assert!(row(&w).can_discard, "the copy is there to put back");

    // Readable, and no longer an agent: the frontmatter the render needs
    // is gone, while every path check still passes.
    let Scope::Project { root } = &w.scope else {
        unreachable!("the test world is a project")
    };
    let local = root.join(".kendex-local/agents/rev.md");
    assert!(local.is_file(), "the fork kept its own copy here");
    fs::write(&local, "no frontmatter, just words\n").unwrap();
    assert!(fs::read_to_string(&local).is_ok(), "it still reads");

    let held = row(&w);
    assert!(
        !held.can_discard,
        "nothing here can render it back: {held:?}"
    );
    assert!(held.forked, "still this place's own copy: {held:?}");
}

/// The same rule for a skill. Collecting the tree says the files are there
/// and readable; the planner renders what it collected and puts the result
/// past the loader's own rules, and a tree refused there comes back
/// unmeasured. A discard offered on the collection alone names a way out
/// that cannot run.
#[test]
#[allow(clippy::unwrap_used)]
fn a_forked_skill_whose_copy_will_not_render_offers_no_discard() {
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

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "gh")
            .unwrap()
    };
    assert!(row(&w).can_discard, "the copy is there to put back");

    // The kept copy keeps its files and loses what makes it a skill: the
    // tree still collects, and nothing can render it for the tool that
    // edited it.
    let Scope::Project { root } = &w.scope else {
        unreachable!("the test world is a project")
    };
    let kept = root.join(".kendex-local/skills/gh/SKILL.md");
    assert!(kept.is_file(), "the fork kept its own copy here");
    fs::write(&kept, "no frontmatter, just words\n").unwrap();

    let held = row(&w);
    assert!(
        !held.can_discard,
        "nothing here can render it back: {held:?}"
    );
    assert!(held.forked, "still this place's own copy: {held:?}");
}

/// The check answers for every tool the declaration targets, not only the
/// ones whose files are edited right now. A discard renders for all of
/// them, and one it cannot render for refuses the whole apply — so a row
/// asking only about the edited ones advertises a button that fails.
///
/// Staged the way it happens: the fork is made while one tool carries the
/// package, and another is added to the declaration afterwards. Nothing
/// about the edited files changes; what the discard has to satisfy does.
#[test]
#[allow(clippy::unwrap_used)]
fn a_target_added_after_the_fork_still_decides_the_discard() {
    let w = world();
    write_skill(&w.upstream, "gh_tool", "Upstream.");
    commit(&w.upstream, "one");
    declare(&w, "[skills.gh_tool]\nsource = \"cat\"\n");
    sync_and_apply(&w);

    let installed = w.home.join("app/.agents/skills/gh_tool/SKILL.md");
    fs::write(
        &installed,
        "---\nname: gh_tool\ndescription: mine\n---\nMy fork.\n",
    )
    .unwrap();
    let plan = fork::fork(
        &w.env,
        &w.scope,
        ItemKind::Skill,
        "gh_tool",
        HarnessId::Claude,
    )
    .unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();
    fs::write(&installed, "edited after forking").unwrap();

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "gh_tool")
            .unwrap()
    };
    let offered = row(&w);
    assert!(
        offered.can_discard,
        "the copy is there to put back: {offered:?}"
    );

    // A second tool is added to the declaration. Its loader will not hold a
    // name with an underscore, so the discard cannot render for it — while
    // the edited files, and the tool that edited them, are unchanged.
    // Edited in place: rewriting the whole file would take the fork record
    // with it, and the fork is what this row is about.
    let path = manifest::manifest_path(&w.env, &w.scope);
    let text = fs::read_to_string(&path).unwrap();
    let widened = text.replace(
        "[skills.gh_tool]",
        "[skills.gh_tool]\nharnesses = [\"claude\", \"opencode\"]",
    );
    assert_ne!(widened, text, "the declaration is in the file: {text}");
    fs::write(&path, widened).unwrap();

    let held = row(&w);
    assert_eq!(
        held.edited_harnesses,
        vec![HarnessId::Claude],
        "the edited tool is the same one: {held:?}"
    );
    assert!(
        !held.can_discard,
        "the tool that cannot take it decides too: {held:?}"
    );
}

/// Parsing is not the planner's whole test for an agent either. The file
/// can read, and its frontmatter can be perfectly well formed, and the
/// harness can still refuse what the planner generates from it — a name
/// that disagrees with the agent being installed is refused as breakage.
/// A discard offered on the parse alone names that way out too.
#[test]
#[allow(clippy::unwrap_used)]
fn a_forked_agent_the_harness_will_not_take_offers_no_discard() {
    let w = world();
    let dir = w.upstream.join("agents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("rev.md"),
        "---\nname: rev\ndescription: agent rev\n---\nAgent body.\n",
    )
    .unwrap();
    commit(&w.upstream, "one");
    declare(&w, "[agents.rev]\nsource = \"cat\"\n");
    sync_and_apply(&w);

    let installed = w.home.join("app/.claude/agents/rev.md");
    fs::write(
        &installed,
        "---\nname: rev\ndescription: mine now\n---\nMy agent.\n",
    )
    .unwrap();
    let plan = fork::fork(&w.env, &w.scope, ItemKind::Agent, "rev", HarnessId::Claude).unwrap();
    apply::execute(&w.env, &plan, None).unwrap();
    let report = audit(&w.env, &w.scope).unwrap();
    apply::execute(&w.env, &report.plan, None).unwrap();
    fs::write(&installed, "edited after forking").unwrap();

    let row = |w: &World| {
        updates::updates(&w.env, &w.scope)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.name == "rev")
            .unwrap()
    };
    assert!(row(&w).can_discard, "the copy is there to put back");

    // Well formed, and about a different agent: Claude reads the name in
    // the file and this one no longer answers to the one being installed.
    let Scope::Project { root } = &w.scope else {
        unreachable!("the test world is a project")
    };
    let local = root.join(".kendex-local/agents/rev.md");
    fs::write(
        &local,
        "---\nname: someone-else\ndescription: mine now\n---\nMy agent.\n",
    )
    .unwrap();
    assert!(
        kendex_core::render::agent::parse_source_agent(&fs::read_to_string(&local).unwrap())
            .is_ok(),
        "it parses — the parse is not the thing that refuses it"
    );

    let held = row(&w);
    assert!(
        !held.can_discard,
        "the harness would refuse what this renders to: {held:?}"
    );
    assert!(held.forked, "still this place's own copy: {held:?}");
}
