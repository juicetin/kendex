//! Whether a fork's own copy can still produce what a discard would put
//! back. Asked of the copy rather than of the path: the discard renders
//! from it, so what counts is that the render succeeds, and every way a
//! path check answers yes while the render refuses is a Discard button
//! that fails when pressed.

use crate::env::Env;
use crate::model::{ItemKind, Scope};

/// A fork's row: no versions, no update — the Library still needs to
/// know it is a fork, and whether its files have been edited since. A fork
/// is the one local source with a row, so a hardcoded "not edited" here
/// would be the only place the measured edit is thrown away.
/// Whether a fork's own copy can still be re-rendered from — asked of the
/// sealed source, which is what the discard reads through. A path check
/// answers a different question than the render does: a skill directory
/// emptied of its `SKILL.md`, an agent file replaced by a directory, a
/// symlink anywhere in the tree, a tree nested past the catalog depth or
/// over its file and byte budgets all read as present and all refuse when
/// the discard runs. Collecting the tree is the refusal, so it is the
/// question — the fork's own tree, which this row already hashes to know
/// it was edited.
pub(super) fn local_copy_resolves(
    env: &Env,
    scope: &Scope,
    kind: ItemKind,
    name: &str,
    decl: &crate::manifest::ItemDecl,
    manifest: &crate::manifest::Manifest,
) -> bool {
    // Every tool the declaration targets, not only the ones whose files
    // happen to be edited now: the discard renders for all of them, and one
    // it cannot render for refuses the whole apply. And with this scope's
    // own manifest, since what it says — a skill's instructions, and the
    // protected block they occupy — is part of what the render has to fit.
    let harnesses = crate::engine::harnesses_for(decl.harnesses.as_deref(), manifest, kind, scope);
    let root = crate::source::local_source_root(env, scope);
    let Ok(sealed) = crate::source_read::SealedSource::open(&root) else {
        return false;
    };
    match kind {
        // Collecting is not enough for a skill either: the planner renders
        // what it collected and puts the result past the loader's own
        // rules, and a tree that fails there is refused as unmeasured. The
        // check is made for each tool that edited it, since the rules
        // differ between them and the discard writes for all of them.
        ItemKind::Skill => {
            let dir = root.join("skills").join(name);
            if !sealed.is_file(&dir.join("SKILL.md")) {
                return false;
            }
            let Ok(rendered) = crate::render::skill::render_skill(&sealed, &dir, manifest, name)
            else {
                return false;
            };
            harnesses.iter().all(|harness| {
                !crate::render::validate::validate_skill_tree(
                    *harness,
                    name,
                    name,
                    rendered.files(),
                )
                .iter()
                .any(|finding| finding.is_breakage())
            })
        }
        // Read is not enough for an agent: the planner parses the file it
        // reads, and a copy that is readable but has no usable frontmatter
        // is refused there as unmeasured. Offering a discard on the
        // strength of the read alone names a way out that cannot run.
        _ => sealed
            .read_to_string(&root.join("agents").join(format!("{name}.md")))
            .is_ok_and(|text| crate::render::agent::parse_source_agent(&text).is_ok()),
    }
}
