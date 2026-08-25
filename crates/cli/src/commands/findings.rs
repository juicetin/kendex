//! What the safety rules found in the content of this machine's scopes —
//! what a tool would load right now, and what an apply would put there.
//!
//! Every open finding is printed with the token that names exactly it on
//! exactly this content, and every held-back item with the flag that
//! accepts exactly the bytes shown above it. Both are the same discipline:
//! a decision names the content it was made against, so the reader can
//! never rule on bytes nobody put in front of them.

use clap::Args;
use kendex_core::engine::decisions::{DecisionState, DecisionToken, short_token};
use kendex_core::engine::{
    ItemSafety, PlanOptions, allow_unsafe_flag, observed_safety, plan_apply,
};
use kendex_core::env::Env;
use kendex_core::error::CoreError;

use super::{CliResult, resolve_scopes, say};
use crate::scope::ScopeFilter;

#[derive(Args)]
pub struct FindingsArgs {
    #[arg(short = 'g', long)]
    global: bool,
    /// project | global | all (default all)
    #[arg(long)]
    scope: Option<String>,
}

/// What the safety rules found in what is installed right now, each finding
/// with the token a dismissal takes and what has already been decided about
/// it.
pub fn findings(env: &Env, args: FindingsArgs) -> CliResult {
    let filter = ScopeFilter::resolve(args.scope.as_deref(), args.global, ScopeFilter::All)?;
    for scope in resolve_scopes(env, filter)? {
        let rows = rows(env, &scope)?;
        if rows.is_empty() {
            say(&format!("{}: nothing found", scope.label()));
            continue;
        }
        say(&format!("{}:", scope.label()));
        for row in &rows {
            print_row(row);
        }
    }
    Ok(())
}

/// One reading of one item, and which bytes it read.
struct Reading {
    row: ItemSafety,
    /// Read from the plan — the bytes an apply would write, which are the
    /// bytes the gate judges and `--allow-unsafe` accepts.
    planned: bool,
    /// This item's other reading is listed too, so each has to say which
    /// side it is. False where one reading says everything.
    paired: bool,
}

impl Reading {
    /// Which bytes, and what is happening to them. An item with one
    /// reading says only whether it is held back, as it always has.
    fn about(&self) -> &'static str {
        match (self.paired, self.planned, self.row.blocked()) {
            (true, true, _) => " — the update, held back",
            (true, false, _) => " — installed now",
            (false, _, true) => " — held back",
            (false, _, false) => "",
        }
    }
}

/// Every safety reading this scope has to show, worst first.
///
/// A declared item is held back over its *desired* render, and that is the
/// content `--allow-unsafe` accepts. Reporting the copy on disk instead
/// would show findings from bytes the gate never reads and hand out a token
/// the gate rejects — a printed instruction that does nothing when
/// followed. So a held-back item is read from the plan.
///
/// The installed copy is read beside it whenever the two are different
/// bytes: something unsafe that a tool is loading this second is not made
/// less true by an update stuck behind the gate, and the installed bytes
/// are where this item's dismissal tokens bind. Where the plan would write
/// exactly what is already there, one reading says everything.
fn rows(env: &Env, scope: &kendex_core::model::Scope) -> Result<Vec<Reading>, CoreError> {
    let held: Vec<ItemSafety> = plan_apply(env, scope, &PlanOptions::default())?
        .safety
        .into_iter()
        .filter(ItemSafety::blocked)
        .collect();
    // Skip-only rows stay in: a hook whose script nobody could read has no
    // finding, and dropping the row would drop the one place this surface
    // says so.
    let installed: Vec<ItemSafety> = observed_safety(env, scope)?
        .into_iter()
        .filter(|row| !row.findings.is_empty() || !row.skipped.is_empty())
        .collect();

    let mut rows: Vec<Reading> = held
        .iter()
        .map(|row| Reading {
            row: row.clone(),
            planned: true,
            paired: !same(row, &installed)
                && installed.iter().any(|other| other.key() == row.key()),
        })
        .collect();
    rows.extend(
        installed
            .iter()
            .filter(|row| !same(row, &held))
            .map(|row| Reading {
                row: row.clone(),
                planned: false,
                paired: held.iter().any(|other| other.key() == row.key()),
            }),
    );
    rows.sort_by_key(|reading| (!reading.row.blocked(), reading.row.safety.score));
    Ok(rows)
}

/// Same installation, same bytes: one reading, not two. "Same bytes" needs
/// bytes on both sides — a skip-only row carries no review hash by
/// construction, and treating two absent hashes as agreement would merge a
/// held reading away on the strength of nothing having been read.
fn same(row: &ItemSafety, others: &[ItemSafety]) -> bool {
    others.iter().any(|other| {
        other.key() == row.key()
            && row.review_hash.is_some()
            && other.review_hash == row.review_hash
    })
}

fn print_row(reading: &Reading) {
    let (row, gated) = (&reading.row, reading.planned);
    say(&format!(
        "  {} {} for {} scores {}/100{}",
        row.kind.name(),
        row.name,
        row.harness.display_name(),
        row.safety.score,
        reading.about()
    ));
    for (finding, decision) in row.findings.iter().zip(&row.decisions) {
        say(&format!(
            "    [{}] {}: {}",
            finding.severity.name(),
            finding.location,
            finding.message
        ));
        say(&format!("      fix: {}", finding.remediation));
        match &decision.state {
            DecisionState::Open { earlier } => {
                if let Some(token) = &decision.token {
                    let printed = match DecisionToken::parse(token) {
                        Ok(parsed) => short_token(&parsed),
                        Err(_) => token.clone(),
                    };
                    say(&format!("      token: {printed}"));
                }
                if let Some(earlier) = earlier {
                    say(&format!("      dismissed before, but {earlier}"));
                }
            }
            DecisionState::Dismissed {
                reason,
                dismissed_at,
            } => say(&format!(
                "      dismissed {dismissed_at} — {}",
                reason.name()
            )),
            DecisionState::AuthorDismissed {
                reason,
                dismissed_at,
                publisher,
            } => say(&format!(
                "      {} reviewed this {} and recorded it as {}",
                kendex_core::names::shown(publisher),
                kendex_core::names::shown(dismissed_at),
                reason.name()
            )),
            DecisionState::Accepted { granted_at } => {
                say(&format!("      accepted {granted_at}"));
            }
        }
    }
    super::engine_common::print_skipped(row);
    // Only what the gate is holding back can be accepted this way. Content
    // already on disk that nothing declares is not waiting on a grant, and
    // offering one would name bytes no plan is about.
    if let Some(review_hash) = &row.review_hash
        && gated
    {
        say(&format!(
            "    to install it anyway, review the findings above and apply with --allow-unsafe {}",
            allow_unsafe_flag(&row.name, review_hash)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook_row(review_hash: Option<&str>) -> ItemSafety {
        ItemSafety {
            kind: kendex_core::model::ItemKind::Hook,
            name: "PreToolUse:*:guard".to_owned(),
            harness: kendex_core::model::HarnessId::Claude,
            scope: kendex_core::model::Scope::Global,
            location: "settings.json".to_owned(),
            safety: kendex_core::quality::SafetyScore {
                score: 100,
                deductions: Vec::new(),
            },
            quality: None,
            findings: Vec::new(),
            skipped: Vec::new(),
            verdict: kendex_core::quality::Verdict::Clean,
            reasons: Vec::new(),
            content_hash: "content".to_owned(),
            review_hash: review_hash.map(str::to_owned),
            provenance: None,
            override_state: kendex_core::quality::overrides::OverrideState::Absent,
            decisions: Vec::new(),
        }
    }

    /// A hook whose script nobody could read has review hash `None` on both
    /// readings by construction. Absent hashes are not the same bytes —
    /// merging on them would print the held reading and silently drop the
    /// installed one, gap and findings alike.
    #[test]
    fn two_absent_review_hashes_are_not_one_reading() {
        let row = hook_row(None);
        assert!(!same(&row, &[hook_row(None)]));
    }

    /// The merge still happens where it is proved: both hashes present and
    /// equal. A differing or missing counterpart keeps both readings.
    #[test]
    fn only_matching_present_hashes_merge() {
        let row = hook_row(Some("abc"));
        assert!(same(&row, &[hook_row(Some("abc"))]));
        assert!(!same(&row, &[hook_row(Some("def"))]));
        assert!(!same(&row, &[hook_row(None)]));
    }
}
