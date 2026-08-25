import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Finding, ItemSafety } from "@/bindings";
import { findingHeadline } from "@/lib/finding-headlines";
import { skipReasonShort } from "@/lib/labels";
import { SafetyWarnings } from "./safety-findings-affected";
import { ScopeFooter } from "./scope-footer";

const FINDING: Finding = {
  rule: "dangerous-commands",
  severity: "high",
  location: "/home/me/.claude/settings.json:17",
  message: "`mkfs` formats a filesystem",
  remediation: "narrow the command to the exact path it needs",
};

const SCRIPT_GAP = {
  rule: "hook-script",
  reason:
    "the script this hook's command invokes could not be read from disk (/home/me/.claude/hooks/guard.sh)",
};

/** An installed hook whose command tripped a finding nobody has ruled on,
 *  and whose script could not be opened — both true of one row. */
function hookRow(): ItemSafety {
  return {
    kind: "hook",
    name: "PreToolUse:*:guard",
    harness: "claude",
    scope: { scope: "global" },
    safety: { score: 70, deductions: [] },
    quality: null,
    findings: [FINDING],
    skipped: [SCRIPT_GAP],
    verdict: "warn",
    reasons: [],
    contentHash: "hash",
    reviewHash: "review-hash",
    location: "/home/me/.claude/settings.json",
    provenance: null,
    decisions: [
      {
        fingerprint: "dangerous-commands:0",
        token: "hook:PreToolUse:*:guard:claude#0@review-hash",
        state: { state: "open", earlier: null },
      },
    ],
    override: { state: "absent" },
  };
}

const footer = (audited: ItemSafety[], row: ItemSafety) =>
  renderToStaticMarkup(
    <ScopeFooter
      clean={[]}
      settled={[]}
      audited={audited}
      alsoScored={[row]}
      notes={[]}
      warnings={[]}
      unmanaged={0}
      onSeeUnmanaged={() => {}}
    />,
  );

// A hook with a finding and an unread script is two facts, and the card
// used to show one: the finding went to the decision zone and the gap was
// summarized only from rows with nothing found, so the row that most
// needed the "not fully checked" line never got it.
describe("a hook with a finding and an unread script", () => {
  it("shows the finding in the decision zone and the gap in the footer", () => {
    const row = hookRow();
    const gap = `Not checked: 1 hook — ${skipReasonShort(SCRIPT_GAP.reason, SCRIPT_GAP.rule)}`;

    const warnings = renderToStaticMarkup(
      <SafetyWarnings
        rows={[row]}
        projectScope={false}
        busy={false}
        onDismiss={() => {}}
      />,
    );
    expect(warnings).toContain(findingHeadline(FINDING.rule, FINDING.message));

    expect(footer([row], row)).toContain(gap);
    // The control: summarizing from clean rows alone, as before, says
    // nothing about this row.
    expect(footer([], row)).not.toContain("Not checked");
  });
});
