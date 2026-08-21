//! Blocking at apply: what the plan says about content it would write, and
//! what never reaches the disk.

use kendex_core::apply;
use kendex_core::engine::{DriftState, edited_here};
use kendex_core::model::ItemKind;
use kendex_core::quality::Verdict;

use super::fixture::{fixture, installed, plan};

/// The plan carries both scores for every item it would write, and the
/// blocked one never reaches the op list.
#[test]
#[allow(clippy::unwrap_used)]
fn a_critical_finding_holds_an_item_back_and_installs_the_rest() {
    let f = fixture();
    let report = plan(&f, &[]);

    let hostile = report
        .safety
        .iter()
        .find(|row| row.name == "hostile")
        .unwrap();
    assert_eq!(hostile.verdict, Verdict::Block);
    assert_eq!(hostile.safety.score, 75);
    assert!(hostile.blocked());
    assert!(hostile.quality.is_some(), "a skill has authored prose");

    let clean = report
        .safety
        .iter()
        .find(|row| row.name == "clean")
        .unwrap();
    assert_eq!(clean.verdict, Verdict::Clean);
    assert_eq!(clean.safety.score, 100);

    // The conflict row says why, in the same machinery a refused rendering
    // already uses.
    let row = report
        .drift
        .iter()
        .find(|row| row.name == "hostile")
        .unwrap();
    assert_eq!(row.state, DriftState::Conflict);
    assert!(row.detail.contains("held back by the safety check"));

    apply::execute(&f.env, &report.plan, None).unwrap();
    assert!(installed(&f, "clean"));
    assert!(!installed(&f, "hostile"));
}

/// A held-back package is a conflict, and it is not an edited one. The
/// discard exits — the CLI's `discard-edits`, the app's targeted apply —
/// plan the whole scope carrying one package's permission, so a predicate
/// that answered on the conflict alone would let them run that scope's
/// pending work under a package nobody edited.
#[test]
#[allow(clippy::unwrap_used)]
fn a_package_the_gate_holds_back_is_not_an_edited_one() {
    let f = fixture();
    let report = plan(&f, &[]);
    let row = report
        .drift
        .iter()
        .find(|row| row.name == "hostile")
        .unwrap();
    assert_eq!(row.state, DriftState::Conflict, "the control: a conflict");
    assert!(row.cause.is_none(), "and not an edit: {row:?}");

    assert!(!edited_here(&f.env, &f.scope, ItemKind::Skill, "hostile").unwrap());
}
