//! What the app hands over when `discard-edits` has no exit of its own.
//! `kendex remove` matches on name alone, so recommending it is safe only
//! where nothing else answers to that name — and the record of what does
//! is in two halves.

use std::fs;

use super::{skill, write};
use crate::common::{kendex, project_with_two_skills};

/// `kendex remove` matches on name alone, so recommending it where two
/// kinds share a name hands over a command that takes the other one with
/// it. The advice says so instead of reading as a safe exit.
#[test]
#[allow(clippy::unwrap_used)]
fn removal_advice_names_what_else_shares_the_name() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    let catalog = home.join("catalog");
    skill(&catalog, "helper", "Upstream helper.");
    write(
        &catalog,
        "agents/helper.md",
        "---\nname: helper\ndescription: about helper\n---\nAgent helper.\n",
    );
    // The skill arrives as gh's dependency; the agent is declared outright.
    write(
        &catalog,
        "skills/gh/SKILL.md",
        "---\nname: gh\ndescription: about gh\ndependencies:\n  required: [helper]\n---\nUpstream gh.\n",
    );
    let manifest = project.join("kendex.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!("{text}\n[agents.helper]\nsource = \"cat\"\n"),
    )
    .unwrap();
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());

    // gh stops requiring the skill, so nothing needs it any more.
    write(
        &catalog,
        "skills/gh/SKILL.md",
        "---\nname: gh\ndescription: about gh\n---\nUpstream gh.\n",
    );
    let helper = project.join(".claude/skills/helper/SKILL.md");
    fs::write(&helper, "my helper edit").unwrap();

    let output = kendex(home, &project, &["discard-edits", "skill", "helper"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{said}");
    assert!(
        said.contains("would take the agent of that name with it"),
        "it says what else the command would take: {said}"
    );
    assert!(
        !said.contains("remove it with 'kendex remove helper'"),
        "and never hands over the command as a safe exit: {said}"
    );
}

/// A package can outlive the line that asked for it: drop the declaration
/// and the lock still holds it, its files still installed. `remove` takes
/// those too, so advice read off the declarations alone calls the command
/// safe over exactly the files it is about to delete.
#[test]
#[allow(clippy::unwrap_used)]
fn removal_advice_names_a_sharer_that_lives_only_in_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    let catalog = home.join("catalog");
    skill(&catalog, "helper", "Upstream helper.");
    write(
        &catalog,
        "agents/helper.md",
        "---\nname: helper\ndescription: about helper\n---\nAgent helper.\n",
    );
    write(
        &catalog,
        "skills/gh/SKILL.md",
        "---\nname: gh\ndescription: about gh\ndependencies:\n  required: [helper]\n---\nUpstream gh.\n",
    );
    let manifest = project.join("kendex.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!("{text}\n[agents.helper]\nsource = \"cat\"\n"),
    )
    .unwrap();
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());

    // The agent keeps its lock entry and its file; only the line asking
    // for it is gone, which is what `remove` would sweep up by name.
    let declared = fs::read_to_string(&manifest).unwrap();
    let dropped = declared
        .replace("[agents.helper]\nsource = \"cat\"\n", "")
        .replace("[agents.helper]\nsource = 'cat'\n", "");
    assert_ne!(dropped, declared, "the agent declaration was removed");
    fs::write(&manifest, dropped).unwrap();
    assert!(
        project.join(".claude/agents/helper.md").exists(),
        "the agent is still installed with nothing declaring it"
    );

    write(
        &catalog,
        "skills/gh/SKILL.md",
        "---\nname: gh\ndescription: about gh\n---\nUpstream gh.\n",
    );
    let helper = project.join(".claude/skills/helper/SKILL.md");
    fs::write(&helper, "my helper edit").unwrap();

    let output = kendex(home, &project, &["discard-edits", "skill", "helper"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{said}");
    assert!(
        said.contains("would take the agent of that name with it"),
        "the undeclared agent is named too: {said}"
    );
    assert!(
        !said.contains("remove it with 'kendex remove helper'"),
        "and the command is never handed over as a safe exit: {said}"
    );
}
