//! The discard exit the drift report prints. It is named for one package
//! because it takes one package: `refresh --discard-edits` is the whole
//! scope, so printing that as the fix for one line would spend every other
//! hand-edited package's work on resolving this one.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[allow(clippy::expect_used)]
fn kendex(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kendex"))
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .expect("kendex binary runs")
}

#[allow(clippy::unwrap_used)]
fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[allow(clippy::unwrap_used)]
fn skill(catalog: &Path, name: &str, body: &str) {
    write(
        catalog,
        &format!("skills/{name}/SKILL.md"),
        &format!("---\nname: {name}\ndescription: about {name}\n---\n{body}\n"),
    );
}

/// A project holding two skills from a local catalog, installed as copies
/// so an edit lives in the installation rather than in the catalog. The
/// catalog also offers `notes`, which nothing declares yet.
#[allow(clippy::unwrap_used)]
fn project_with_two_skills(home: &Path) -> PathBuf {
    let project = home.join("dev/app");
    fs::create_dir_all(project.join(".claude")).unwrap();
    let catalog = home.join("catalog");
    write(&catalog, "kendex.toml", "[catalog]\n");
    skill(&catalog, "gh", "Upstream gh.");
    skill(&catalog, "lint", "Upstream lint.");
    skill(&catalog, "notes", "Upstream notes.");
    write(
        &project,
        "kendex.toml",
        &format!(
            "schema = 5\n\n[sources.cat]\npath = \"{}\"\n\n[install]\nharnesses = [\"claude\"]\nmethod = \"copy\"\n\n[skills.gh]\nsource = \"cat\"\n\n[skills.lint]\nsource = \"cat\"\n",
            catalog.display()
        ),
    );
    project
}

/// Work the scope has waiting that this command was not asked about: a
/// third skill declared after the install, so the scope's plan is not empty
/// however clean the named package is.
#[allow(clippy::unwrap_used)]
fn declare_pending_work(project: &Path) {
    let manifest = project.join("kendex.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[skills.notes]\nsource = \"cat\"\n");
    fs::write(&manifest, text).unwrap();
}

#[test]
#[allow(clippy::unwrap_used)]
fn discarding_one_packages_edits_leaves_the_other_packages_edits_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);

    let output = kendex(home, &project, &["apply", "-y"]);
    assert!(
        output.status.success(),
        "apply: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let gh = project.join(".claude/skills/gh/SKILL.md");
    let lint = project.join(".claude/skills/lint/SKILL.md");
    assert!(gh.is_file() && lint.is_file(), "both skills installed");

    fs::write(&gh, "my gh edit").unwrap();
    fs::write(&lint, "my lint edit").unwrap();

    // The command the drift report prints for one edited package.
    let output = kendex(home, &project, &["discard-edits", "skill", "gh"]);
    assert!(
        output.status.success(),
        "discard-edits: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        fs::read_to_string(&gh).unwrap().contains("Upstream gh."),
        "the named package came back"
    );
    assert_eq!(
        fs::read_to_string(&lint).unwrap(),
        "my lint edit",
        "following the printed fix took another package's edits"
    );
}

/// The control on the scope-wide spelling: it is still there, it still
/// takes everything, and that is why it is not what the report prints.
#[test]
#[allow(clippy::unwrap_used)]
fn refresh_with_discard_edits_is_the_whole_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);

    assert!(kendex(home, &project, &["apply", "-y"]).status.success());
    let gh = project.join(".claude/skills/gh/SKILL.md");
    let lint = project.join(".claude/skills/lint/SKILL.md");
    fs::write(&gh, "my gh edit").unwrap();
    fs::write(&lint, "my lint edit").unwrap();

    let output = kendex(home, &project, &["refresh", "-y", "--discard-edits"]);
    assert!(
        output.status.success(),
        "refresh: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(fs::read_to_string(&gh).unwrap().contains("Upstream gh."));
    assert!(
        fs::read_to_string(&lint)
            .unwrap()
            .contains("Upstream lint."),
        "the scope-wide flag takes every edit — the reason it is not a fix line"
    );
}

// The command names one package, so it acts on one package or on nothing.
// A scope always has other work waiting sooner or later, and a plan built
// to carry this package's permission carries that work too — executing it
// under a line saying this package was restored spends the one on the
// other.
#[test]
#[allow(clippy::unwrap_used)]
fn a_clean_target_applies_nothing_even_with_work_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());

    let lint = project.join(".claude/skills/lint/SKILL.md");
    fs::write(&lint, "my lint edit").unwrap();
    declare_pending_work(&project);
    let notes = project.join(".claude/skills/notes/SKILL.md");
    assert!(!notes.exists(), "the waiting work has not run yet");

    // gh is clean: there is nothing here to discard.
    let output = kendex(home, &project, &["discard-edits", "skill", "gh"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{said}");
    assert!(said.contains("no edits to discard"), "{said}");
    assert!(
        !notes.exists(),
        "a clean target ran the scope's waiting work: {said}"
    );
    assert_eq!(
        fs::read_to_string(&lint).unwrap(),
        "my lint edit",
        "and took another package's edits with it"
    );
}

/// Declared but never installed reads the same way: there is no edit here,
/// so there is nothing to put back.
#[test]
#[allow(clippy::unwrap_used)]
fn a_target_with_no_installation_applies_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());
    fs::write(project.join(".claude/skills/lint/SKILL.md"), "my lint edit").unwrap();
    declare_pending_work(&project);

    let notes = project.join(".claude/skills/notes/SKILL.md");
    let output = kendex(home, &project, &["discard-edits", "skill", "notes"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{said}");
    assert!(said.contains("no edits to discard"), "{said}");
    assert!(!notes.exists(), "nothing was installed under it: {said}");
}

/// A name this scope never declared is a mistake, and saying so is better
/// than a success line over work the caller never asked for.
#[test]
#[allow(clippy::unwrap_used)]
fn an_undeclared_target_refuses_and_applies_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());
    fs::write(project.join(".claude/skills/lint/SKILL.md"), "my lint edit").unwrap();
    declare_pending_work(&project);

    let notes = project.join(".claude/skills/notes/SKILL.md");
    let output = kendex(home, &project, &["discard-edits", "skill", "nope"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{said}");
    assert!(said.contains("is installed"), "{said}");
    assert!(!notes.exists(), "nothing ran under the wrong name: {said}");
    assert_eq!(
        fs::read_to_string(project.join(".claude/skills/lint/SKILL.md")).unwrap(),
        "my lint edit"
    );
}

// The target genuinely has edits, and the scope has work waiting that
// nobody asked this command about. The permission to overwrite one
// package's bytes does not narrow the plan those bytes are written by, so
// the plan has to be narrowed too.
#[test]
#[allow(clippy::unwrap_used)]
fn an_edited_target_leaves_the_scope_pending_work_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());

    let gh = project.join(".claude/skills/gh/SKILL.md");
    let lint = project.join(".claude/skills/lint/SKILL.md");
    fs::write(&gh, "my gh edit").unwrap();
    fs::write(&lint, "my lint edit").unwrap();
    declare_pending_work(&project);
    let notes = project.join(".claude/skills/notes/SKILL.md");

    let output = kendex(home, &project, &["discard-edits", "skill", "gh"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{said}");

    assert!(
        fs::read_to_string(&gh).unwrap().contains("Upstream gh."),
        "the package asked for came back: {said}"
    );
    assert!(
        !notes.exists(),
        "and installed a package nobody asked about: {said}"
    );
    assert_eq!(
        fs::read_to_string(&lint).unwrap(),
        "my lint edit",
        "and took another package's edits"
    );
    assert!(
        !said.contains("notes"),
        "the line names one package: {said}"
    );

    // The record still knows both installs — a plan that forgot them would
    // reinstall or sweep them on the next pass.
    let listed = kendex(home, &project, &["list"]);
    let table = String::from_utf8_lossy(&listed.stderr).into_owned()
        + &String::from_utf8_lossy(&listed.stdout);
    assert!(table.contains("gh") && table.contains("lint"), "{table}");
}

/// A package installed here because something else needed it is a package
/// installed here. The app has always offered its discard; a guard reading
/// declarations alone refused the command for exactly the packages a person
/// is most likely to have edited without declaring.
#[test]
#[allow(clippy::unwrap_used)]
fn a_dependency_nobody_declared_can_still_be_discarded() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let project = project_with_two_skills(home);
    // gh requires helper, which nothing declares.
    let catalog = home.join("catalog");
    write(
        &catalog,
        "skills/gh/SKILL.md",
        "---\nname: gh\ndescription: about gh\ndependencies:\n  required: [helper]\n---\nUpstream gh.\n",
    );
    skill(&catalog, "helper", "Upstream helper.");
    assert!(kendex(home, &project, &["apply", "-y"]).status.success());

    let helper = project.join(".claude/skills/helper/SKILL.md");
    assert!(helper.is_file(), "the dependency is installed");
    fs::write(&helper, "my helper edit").unwrap();

    let output = kendex(home, &project, &["discard-edits", "skill", "helper"]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{said}");
    assert!(
        fs::read_to_string(&helper)
            .unwrap()
            .contains("Upstream helper."),
        "the discard the app offers is the one the CLI refused: {said}"
    );
}
