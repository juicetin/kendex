//! Whether the file the editor's save produces is the manifest it handed
//! in. A copy typed while the write is away descends from what was sent, so
//! taking the written file's base for it is only right when the two are the
//! same — and the write puts down things nobody typed: the seed a first
//! manifest gets, a name derived for a custom hook, and whatever the
//! planner records for itself.
#![cfg(unix)]

use std::fs;

use kendex_app::editor::write_manifest;
use kendex_core::env::{Env, FakeOs};
use kendex_core::manifest::Manifest;
use kendex_core::model::Scope;

/// The base the next save must carry: what the file on disk hashes to,
/// derived the one way anything derives one.
#[allow(clippy::unwrap_used)]
fn base_now(f: &Fixture) -> Option<String> {
    let text = fs::read_to_string(f.project.join("kendex.toml")).unwrap();
    Some(kendex_core::hash::hash_bytes(text.as_bytes()))
}

struct Fixture {
    _tmp: tempfile::TempDir,
    env: Env,
    scope: Scope,
    project: std::path::PathBuf,
}

#[allow(clippy::unwrap_used)]
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    let env = Env::fake(&home, FakeOs::Linux);
    let project = home.join("dev/app");
    fs::create_dir_all(project.join(".claude")).unwrap();
    let source = home.join("catalog");
    fs::create_dir_all(source.join("skills/gh")).unwrap();
    fs::write(
        source.join("skills/gh/SKILL.md"),
        "---\nname: gh\ndescription: about gh\n---\nUpstream gh.\n",
    )
    .unwrap();
    Fixture {
        _tmp: tmp,
        env,
        scope: Scope::Project {
            root: project.clone(),
        },
        project,
    }
}

#[allow(clippy::unwrap_used)]
fn sent(f: &Fixture, body: &str) -> Manifest {
    let source = f.env.home.join("catalog");
    let text = format!(
        "schema = 5\n\n[sources.cat]\npath = \"{}\"\n\n[install]\nharnesses = [\"claude\"]\nmethod = \"copy\"\n{body}",
        source.display()
    );
    toml::from_str(&text).unwrap()
}

/// The ordinary save: what the caller sent is what the file holds, so the
/// copy that made it may go on using this file's base. Told otherwise,
/// every save would refuse the copy that produced it.
#[test]
#[allow(clippy::unwrap_used)]
fn a_save_that_changes_nothing_says_the_file_is_what_was_sent() {
    let f = fixture();
    let manifest = sent(&f, "\n[skills.gh]\nsource = \"cat\"\n");
    let first = write_manifest(&f.env, f.scope.clone(), manifest.clone(), None).unwrap();
    assert!(!first.wrote_more, "nothing here is added by the write");

    // And again over the file it just made.
    let again = write_manifest(&f.env, f.scope.clone(), manifest, base_now(&f)).unwrap();
    assert!(!again.wrote_more);
}

/// Creating the file seeds the default source and this machine's harnesses,
/// which no copy in hand carries.
#[test]
#[allow(clippy::unwrap_used)]
fn creating_the_file_says_it_holds_more_than_was_sent() {
    let f = fixture();
    let bare = Manifest {
        schema: 5,
        ..Manifest::default()
    };
    let written = write_manifest(&f.env, f.scope.clone(), bare, None).unwrap();
    assert!(
        written.wrote_more,
        "the seed is in the file and nowhere else"
    );
}

/// A hook that arrives without a name is named by the save, on a file that
/// already exists.
#[test]
#[allow(clippy::unwrap_used)]
fn naming_a_hook_says_the_file_holds_more_than_was_sent() {
    let f = fixture();
    let plain = sent(&f, "");
    write_manifest(&f.env, f.scope.clone(), plain, None).unwrap();

    let hooked = sent(
        &f,
        "\n[[custom-hooks]]\nevent = \"PreToolUse\"\ncommand = \"./guard.sh\"\nagents = \"all\"\n",
    );
    let written = write_manifest(&f.env, f.scope.clone(), hooked, base_now(&f)).unwrap();
    assert!(
        written.wrote_more,
        "the derived name is not in what was sent"
    );
}

/// And what the planner records for itself: an agent's mapping merges back
/// whatever upstream gained, which reaches a manifest that already exists
/// and is not one of the changes made before planning.
#[test]
#[allow(clippy::unwrap_used)]
fn a_change_the_planner_records_says_the_file_holds_more() {
    let f = fixture();
    let source = f.env.home.join("catalog");
    fs::create_dir_all(source.join("agents")).unwrap();
    fs::write(
        source.join("agents/rust.md"),
        "---\nname: rust\ndescription: rusty\nmodel: opus\nrole: engineer\n---\nAgent.\n",
    )
    .unwrap();

    let declared = sent(
        &f,
        "\n[agents.rust]\nsource = \"cat\"\n\n[agent-skills]\nrust = [\"gh\"]\n",
    );
    write_manifest(&f.env, f.scope.clone(), declared, None).unwrap();
    assert!(f.project.join("kendex.toml").is_file());

    // Upstream gains a skill this agent picks up, after the file was last
    // read. The copy the editor holds cannot know about it.
    fs::create_dir_all(source.join("skills/rust-perf")).unwrap();
    fs::write(
        source.join("skills/rust-perf/SKILL.md"),
        "---\nname: rust-perf\ndescription: faster\n---\nPerf.\n",
    )
    .unwrap();

    // Saving the file exactly as it stands: the pass merges the new skill
    // into the mapping, so what lands is not what was sent.
    let held = match kendex_core::manifest::load(&f.project.join("kendex.toml")).unwrap() {
        kendex_core::manifest::ManifestFile::Current(m) => *m,
        _ => panic!("the file this test just wrote reads as current"),
    };
    let again = write_manifest(&f.env, f.scope.clone(), held, base_now(&f)).unwrap();
    assert!(
        again.wrote_more,
        "the mapping the pass recorded is not in what was sent"
    );
}
