//! Writing a whole file from a copy someone has been holding.
//!
//! The copy carries the base of the file it came from, and every write of
//! that file in the plan binds to it — including one the plan brought
//! itself, which binds to what the file was when the plan ran. That later
//! question accepts a writer the copy never saw, which is the writer the
//! base exists to keep out.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use kendex_core::apply::{Op, Plan, PlannedOp, Pre};
use kendex_core::env::{Env, FakeOs};
use kendex_core::manifest;
use kendex_core::model::Scope;

#[allow(clippy::unwrap_used)]
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// What a plan looks like when the engine brought its own manifest write —
/// a schema upgrade, a repository move — bound to the file as the plan
/// found it rather than as the editor's copy left it.
fn planned_by_the_engine(scope: &Scope, path: &Path, pre: Pre) -> Plan {
    Plan {
        scope: scope.clone(),
        ops: vec![PlannedOp {
            description: "Save kendex.toml".into(),
            op: Op::WriteFile {
                path: path.to_path_buf(),
                bytes: b"schema = 5\n# the plan's own write\n".to_vec(),
                pre,
            },
        }],
    }
}

#[test]
#[allow(clippy::unwrap_used)]
fn a_write_the_plan_brought_binds_to_the_copy_being_written() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = manifest::manifest_path(&env, &scope);
    write(&path, "schema = 5\n");

    // The editor read the file here.
    let (_, held) = manifest::read_for_mutation(&path).unwrap();
    let base = Pre::from(&held);

    // Something else wrote it before the save reached the disk.
    write(&path, "schema = 5\n\n[forks.skill.gh]\nsource = \"cat\"\n");

    // As the engine planned it, the write binds to the file it found — the
    // one that replaced the copy — and would go through.
    let observed = Pre::observed(&path).unwrap();
    let mut plan = planned_by_the_engine(&scope, &path, observed);
    plan.bind_writes(&path, &base);

    let refused = kendex_core::apply::execute(&env, &plan, None);
    assert!(refused.is_err(), "the copy was written over the fork");
    assert!(
        fs::read_to_string(&path).unwrap().contains("forks"),
        "and the fork is still there"
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn the_same_write_lands_while_the_file_is_still_the_one_it_came_from() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = manifest::manifest_path(&env, &scope);
    write(&path, "schema = 5\n");

    let (_, held) = manifest::read_for_mutation(&path).unwrap();
    let mut plan = planned_by_the_engine(&scope, &path, Pre::Any);
    plan.bind_writes(&path, &Pre::from(&held));

    kendex_core::apply::execute(&env, &plan, None).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("the plan's own write")
    );
}

/// Nothing was there when the copy was read, so nothing may be there when
/// it is written: a place that got its first manifest in between keeps it.
#[test]
#[allow(clippy::unwrap_used)]
fn a_first_write_refuses_a_file_that_appeared_in_between() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = manifest::manifest_path(&env, &scope);

    let (_, held) = manifest::read_for_mutation(&path).unwrap();
    assert_eq!(held, manifest::Base::absent(), "nothing to read yet");
    write(&path, "schema = 5\n\n[skills.gh]\nsource = \"cat\"\n");

    let mut plan = planned_by_the_engine(&scope, &path, Pre::Any);
    plan.bind_writes(&path, &Pre::Absent);

    assert!(kendex_core::apply::execute(&env, &plan, None).is_err());
    assert!(fs::read_to_string(&path).unwrap().contains("skills.gh"));
}

/// What an apply hands back about the file it wrote, and when it is true.
///
/// A caller that writes a whole file and then reads the file back is
/// pairing its own copy with whatever landed in between: the apply lets the
/// scope go before that read, so the base it gets can already be somebody
/// else's, and the next write carrying that pair is accepted over them. The
/// apply reads it while it still owns the scope, which is the last moment
/// the answer is provably its own.
#[test]
#[allow(clippy::unwrap_used)]
fn the_apply_answers_for_the_file_it_left() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = manifest::manifest_path(&env, &scope);
    write(&path, "schema = 5\n");

    let (_, held) = manifest::read_for_mutation(&path).unwrap();
    let mut plan = planned_by_the_engine(&scope, &path, Pre::Any);
    plan.bind_writes(&path, &Pre::from(&held));
    let outcome = kendex_core::apply::execute(&env, &plan, None).unwrap();

    // The bytes this apply left, not a later reading of the path.
    let (_, after) = manifest::read_for_mutation(&path).unwrap();
    assert_eq!(outcome.manifest_base, after);

    // Someone else writes, as they may the moment the scope is free. The
    // base the apply handed back still describes what the apply wrote, so
    // the next write carrying it is refused rather than landing on top.
    write(&path, "schema = 5\n\n[forks.skill.gh]\nsource = \"cat\"\n");
    assert!(manifest::check_base(&path, &outcome.manifest_base).is_err());

    let mut next = planned_by_the_engine(&scope, &path, Pre::Any);
    next.bind_writes(&path, &Pre::from(&outcome.manifest_base));
    assert!(kendex_core::apply::execute(&env, &next, None).is_err());
    assert!(fs::read_to_string(&path).unwrap().contains("forks"));
}
