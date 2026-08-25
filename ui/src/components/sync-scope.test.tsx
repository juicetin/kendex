import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AuditView, ItemSafety } from "@/bindings";
import { findingHeadline } from "@/lib/finding-headlines";
import { SyncScopeCard } from "./sync-scope";

/** An installed hook whose command tripped a finding nobody has ruled on
 *  and whose script could not be opened. It is an open row, not a clean
 *  one, and that is the row the footer used to lose. */
function hookRow(): ItemSafety {
  return {
    kind: "hook",
    name: "PreToolUse:*:guard",
    harness: "claude",
    scope: { scope: "global" },
    safety: { score: 70, deductions: [] },
    quality: null,
    findings: [
      {
        rule: "dangerous-commands",
        severity: "high",
        location: "/home/me/.claude/settings.json:17",
        message: "`mkfs` formats a filesystem",
        remediation: "narrow the command to the exact path it needs",
      },
    ],
    skipped: [
      {
        rule: "hook-script",
        reason:
          "the script this hook's command invokes could not be read from disk (/home/me/.claude/hooks/guard.sh)",
      },
    ],
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

function view(row: ItemSafety): AuditView {
  return {
    scope: { scope: "global" },
    drift: [],
    plan: [],
    notes: [],
    warnings: [],
    safety: [row],
    adoptable: [],
    exits: [],
    heldBack: [],
    queued: [],
  };
}

// The card is the one place the partition and the footer meet: a hook
// with an open finding and an unread script has to come out of it with
// the finding in the decision zone and the gap in the footer. Summarizing
// the footer's gaps from clean rows alone drops exactly this row.
describe("a scope card with a hook that has a finding and an unread script", () => {
  it("shows the finding and the not-checked line together", () => {
    const row = hookRow();
    const html = renderToStaticMarkup(
      <SyncScopeCard
        view={view(row)}
        busy={false}
        onApply={() => {}}
        onDismiss={() => {}}
        onKeepFiles={() => Promise.resolve()}
        onReplaceFiles={() => Promise.resolve()}
        onSeeUnmanaged={() => {}}
      />,
    );
    expect(html).toContain(
      findingHeadline(row.findings[0].rule, row.findings[0].message),
    );
    expect(html).toContain(
      "Not checked: 1 hook — its script could not be read",
    );
  });
});
