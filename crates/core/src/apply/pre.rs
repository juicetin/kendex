//! What a write binds to.
//!
//! Every mutation revalidates its precondition immediately before running,
//! so a plan computed against one state never lands on another (invariant
//! 7). A whole-file write binds to something stricter still: the file the
//! copy being written came from, which is a `manifest::Base` and outranks
//! whatever the plan observed for itself.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CoreError, Result};
use crate::hash::hash_tree;

/// A precondition every mutation revalidates immediately before running —
/// plans bind to the observed state they were computed from (invariant 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "pre", rename_all = "kebab-case")]
pub enum Pre {
    Absent,
    /// The bytes reachable at the path — through a link, if one sits there —
    /// still hash to this. Whether a link may sit there at all is decided at
    /// plan time (a foreign one is a conflict); a user's own symlinked
    /// settings file is edited in place, link kept, target updated.
    HashIs {
        hash: String,
    },
    SymlinkTo {
        target: PathBuf,
    },
    Any,
}

impl From<&crate::manifest::Base> for Pre {
    /// What a whole-file write binds to: the file is exactly the bytes the
    /// copy came from, or nothing was there and nothing may be there now.
    fn from(base: &crate::manifest::Base) -> Pre {
        match base.hash() {
            Some(hash) => Pre::HashIs {
                hash: hash.to_owned(),
            },
            None => Pre::Absent,
        }
    }
}

impl Pre {
    /// What a plan that rewrites `path` wholesale binds to: the bytes seen
    /// at plan time, or the absence seen at plan time.
    pub fn observed(path: &Path) -> Result<Pre> {
        match path.is_file() {
            true => Ok(Pre::HashIs {
                hash: hash_tree(path)?,
            }),
            false => Ok(Pre::Absent),
        }
    }

    pub(super) fn check(&self, path: &Path) -> Result<()> {
        let ok = match self {
            Pre::Any => true,
            Pre::Absent => !path.exists() && !path.is_symlink(),
            Pre::HashIs { hash } => {
                path.exists() && hash_tree(path).map(|h| h == *hash).unwrap_or(false)
            }
            Pre::SymlinkTo { target } => {
                path.is_symlink() && fs::read_link(path).ok().as_deref() == Some(target)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(CoreError::PlanStale {
                path: path.to_path_buf(),
            })
        }
    }
}
