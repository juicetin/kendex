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
/// so an edit lives in the installation rather than in the catalog.
#[allow(clippy::unwrap_used)]
fn project_with_two_skills(home: &Path) -> PathBuf {
    let project = home.join("dev/app");
    fs::create_dir_all(project.join(".claude")).unwrap();
    let catalog = home.join("catalog");
    write(&catalog, "kendex.toml", "[catalog]\n");
    skill(&catalog, "gh", "Upstream gh.");
    skill(&catalog, "lint", "Upstream lint.");
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
