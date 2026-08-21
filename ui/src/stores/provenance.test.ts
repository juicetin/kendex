import { describe, expect, it, vi } from "vitest";
import type { ProvenanceRow } from "@/bindings";
import { commands } from "@/bindings";
import {
  indexOrigins,
  originFor,
  originLabel,
  originTitle,
  useProvenanceStore,
} from "./provenance";

vi.mock("@/bindings", () => ({ commands: { libraryProvenance: vi.fn() } }));

const ROWS: ProvenanceRow[] = [
  {
    scope: { scope: "global" },
    kind: "skill",
    name: "gh",
    harness: "claude",
    origin: { origin: "marketplace", source: "kendex", repo: "acme/kendex" },
  },
  {
    scope: { scope: "project", root: "/work/app" },
    kind: "skill",
    name: "gh",
    harness: "claude",
    origin: { origin: "own", forkedFrom: "kendex" },
  },
  {
    scope: { scope: "global" },
    kind: "agent",
    name: "gh",
    harness: "claude",
    origin: { origin: "unmanaged" },
  },
];

const INDEX = indexOrigins(ROWS);

describe("the From column's join", () => {
  it("matches by kind, name, and any of the group's scopes", () => {
    const origin = originFor(INDEX, "skill", "gh", [{ scope: "global" }]);
    expect(origin).toEqual({
      origin: "marketplace",
      source: "kendex",
      repo: "acme/kendex",
    });
    // The same name in another scope answers with that scope's origin —
    // a fork there does not relabel the global install.
    expect(
      originFor(INDEX, "skill", "gh", [
        { scope: "project", root: "/work/app" },
      ]),
    ).toEqual({ origin: "own", forkedFrom: "kendex" });
    // A same-named item of another kind never borrows this one's origin.
    expect(originFor(INDEX, "hook", "gh", [{ scope: "global" }])).toBeNull();
  });

  it("keeps the first row for a place, as the scan it replaced did", () => {
    const twice = indexOrigins([
      ...ROWS,
      { ...ROWS[0], origin: { origin: "own", forkedFrom: "later" } },
    ]);
    expect(originFor(twice, "skill", "gh", [{ scope: "global" }])).toEqual({
      origin: "marketplace",
      source: "kendex",
      repo: "acme/kendex",
    });
  });

  it("labels origins in product words with the detail on hover", () => {
    expect(
      originLabel({ origin: "marketplace", source: "kendex", repo: "r" }),
    ).toBe("kendex");
    expect(
      originTitle({ origin: "marketplace", source: "kendex", repo: "r" }),
    ).toBe("r");
    expect(originLabel({ origin: "own", forkedFrom: "kendex" })).toBe(
      "Your own",
    );
    expect(originTitle({ origin: "own", forkedFrom: "kendex" })).toBe(
      "forked from kendex",
    );
    expect(originTitle({ origin: "own", forkedFrom: null })).toBeUndefined();
    expect(originLabel({ origin: "unmanaged" })).toBe("Not managed");
    expect(originLabel(null)).toBe("");
  });
});

describe("re-reading the join", () => {
  it("hands back the same rows when a read says the same thing", async () => {
    vi.mocked(commands.libraryProvenance).mockResolvedValue({
      status: "ok",
      data: ROWS,
    });
    await useProvenanceStore.getState().load();
    const first = useProvenanceStore.getState().rows;
    // A fresh array off the wire, saying exactly what the last one did: the
    // table keys these per place and memoizes on identity.
    vi.mocked(commands.libraryProvenance).mockResolvedValue({
      status: "ok",
      data: ROWS.map((row) => ({ ...row })),
    });
    await useProvenanceStore.getState().load();
    expect(useProvenanceStore.getState().rows).toBe(first);
  });
});
