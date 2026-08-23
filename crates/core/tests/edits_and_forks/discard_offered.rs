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
