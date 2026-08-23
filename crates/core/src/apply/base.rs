//! What a caller may carry away from an apply: the base its next write
//! binds to, and when the honest answer is nothing.

use crate::env::Env;
use crate::model::Scope;

use super::{Op, Plan};

/// The base a caller may carry after this apply: what is on disk, once it
/// has been shown to be what this apply put there.
///
/// A plan that writes the manifest knows the bytes it wrote, so the file
/// is compared against them. Anything else — an editor that saved in the
/// window between the journal committing and this read — answers `None`
/// rather than a base describing a moment this apply never made. Handed
/// one of those, the caller's next save would carry it, pass the check,
/// and overwrite the other write, which is the loss the base exists to
/// prevent.
///
/// `None` is recoverable and a wrong base is not: the caller marks the
/// place and re-reads, which settles it whenever nobody has typed since.
pub(super) fn base_this_apply_left(env: &Env, plan: &Plan) -> Option<crate::manifest::Base> {
    let path = crate::manifest::manifest_path(env, &plan.scope);
    let wrote = plan.ops.iter().find_map(|planned| match &planned.op {
        Op::WriteManifest {
            path: at, manifest, ..
        } if *at == path => crate::manifest::as_text(at, manifest).ok(),
        Op::WriteFile {
            path: at, bytes, ..
        } if *at == path => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    });
    let Some(wrote) = wrote else {
        // This plan did not write the manifest, so there is nothing of its
        // own to check against: the file as it stands is the answer, which
        // is what a caller that wrote nothing here is asking for.
        return manifest_base(env, &plan.scope);
    };
    match crate::fs::read_if_exists(&path) {
        Ok(Some(text)) if text == wrote => Some(crate::manifest::Base::of(&text)),
        _ => None,
    }
}

/// The scope's manifest as it stands, for an apply that still holds the
/// lock. Derived from the bytes read here, like every other base, and
/// `None` where they could not be read — every caller of this is past the
/// point of undoing anything, so a read that fails costs the answer and
/// never the apply.
pub(super) fn manifest_base(env: &Env, scope: &Scope) -> Option<crate::manifest::Base> {
    let path = crate::manifest::manifest_path(env, scope);
    match crate::fs::read_if_exists(&path) {
        Ok(Some(text)) => Some(crate::manifest::Base::of(&text)),
        Ok(None) => Some(crate::manifest::Base::absent()),
        Err(_) => None,
    }
}
