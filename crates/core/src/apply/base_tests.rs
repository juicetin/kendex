//! What a caller may carry away from an apply, and when the honest
//! answer is nothing.

use super::base::base_this_apply_left;
use super::{Op, Plan, PlannedOp, Pre};
use crate::env::Env;
use crate::env::FakeOs;
use crate::model::Scope;

/// The plan a save brings: one whole-manifest write for this scope.
#[allow(clippy::unwrap_used)]
fn saving(env: &Env, scope: &Scope, manifest: crate::manifest::Manifest) -> Plan {
    Plan {
        scope: scope.clone(),
        ops: vec![PlannedOp {
            description: "Save kendex.toml".into(),
            op: Op::WriteManifest {
                pre: Pre::Absent,
                path: crate::manifest::manifest_path(env, scope),
                manifest: Box::new(manifest),
            },
        }],
    }
}

/// The window the lock does not cover. kendex's own writers are
/// serialised, so no other apply sits between the journal committing
/// and the base being read — an editor saving `kendex.toml` in that
/// instant is not. A base taken from its bytes describes a moment this
/// apply never made, and the caller's next save would carry it, pass
/// the check, and land on top of that write.
#[test]
#[allow(clippy::unwrap_used)]
fn a_file_someone_else_left_answers_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = crate::manifest::manifest_path(&env, &scope);
    let mine = crate::manifest::Manifest {
        schema: crate::manifest::MANIFEST_SCHEMA,
        ..Default::default()
    };
    let plan = saving(&env, &scope, mine.clone());

    // The file is what this plan writes: the base describes it, and the
    // copy that made it may go on using it.
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, crate::manifest::as_text(&path, &mine).unwrap()).unwrap();
    assert!(base_this_apply_left(&env, &plan).is_some());

    // And now somebody else's save is what is there.
    std::fs::write(&path, "schema = 5\n\n[forks.skill.gh]\nsource = \"cat\"\n").unwrap();
    assert_eq!(
        base_this_apply_left(&env, &plan),
        None,
        "a base describing another writer's bytes is worse than no base"
    );
}

/// A plan that writes no manifest has nothing of its own to check
/// against, and its callers still want to know what the file says.
#[test]
#[allow(clippy::unwrap_used)]
fn a_plan_that_wrote_no_manifest_still_answers() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let env = Env::fake(&home, FakeOs::Linux);
    let scope = Scope::Global;
    let path = crate::manifest::manifest_path(&env, &scope);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "schema = 5\n").unwrap();

    let plan = Plan {
        scope: scope.clone(),
        ops: Vec::new(),
    };
    assert_eq!(
        base_this_apply_left(&env, &plan),
        Some(crate::manifest::Base::of("schema = 5\n"))
    );
}
