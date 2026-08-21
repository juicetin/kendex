import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { DriftRow } from "@/bindings";
import { mergeDriftRows, reviewLists } from "@/lib/drift-merge";
import { ScopeChanges, ScopeConflicts } from "./scope-details";

const conflict = (over: Partial<DriftRow> = {}): DriftRow => ({
  kind: "skill",
  name: "gh",
  harness: "claude",
  scope: { scope: "project", root: "/work/vg" },
  state: "conflict",
  detail: "edited on disk since your fork was rendered",
  cause: "local-edit",
  ...over,
});

// Apply runs a plan, and a conflict has no ops behind it. Filing one under
// "Ready to apply" counts it as work a button can do and offers no button.
describe("a conflict in the Review card", () => {
  it("is not counted or headed as ready to apply", () => {
    const stale = conflict({ name: "rev", state: "stale" });
    const lists = reviewLists([conflict(), stale]);
    expect(lists.changes.map((one) => one.name)).toEqual(["rev"]);
    expect(lists.conflicts.map((one) => one.name)).toEqual(["gh"]);
    expect(
      renderToStaticMarkup(<ScopeChanges changes={lists.changes} />),
    ).not.toContain(">gh<");
  });

  it("is listed where its exits are, with the way to them", () => {
    const onOpen = vi.fn();
    const html = renderToStaticMarkup(
      <ScopeConflicts
        conflicts={mergeDriftRows([conflict()])}
        onOpen={onOpen}
      />,
    );
    expect(html).toContain("Waiting on you, on their own pages");
    expect(html).toContain("<button");
    expect(html).toContain(">gh<");
  });

  it("says nothing when there is no conflict", () => {
    expect(
      renderToStaticMarkup(<ScopeConflicts conflicts={[]} onOpen={() => {}} />),
    ).toBe("");
  });
});
