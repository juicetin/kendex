import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ObservedItem, Scope } from "@/bindings";
import { updateRow } from "@/components/updates-test-rows";
import { placeStandings } from "@/lib/customized-places";
import { groupItems } from "@/lib/derive";
import type { Draft } from "@/lib/editor-draft";
import {
  changed,
  EVERYWHERE,
  forkedHere,
  HYPR,
  plainManifests,
  source,
  VG,
} from "@/lib/places-test-source";
import { InstalledTable, markNav } from "./installed-table";

// Static rendering reads a zustand store's initial snapshot, never one set
// later, so both stores are wrapped to let a test stage what each place
// holds.
const stub = vi.hoisted(() => ({
  saved: {} as Record<string, unknown>,
  rows: [] as unknown[],
}));

vi.mock("@/stores/editor", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/editor")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useEditorStore.getState(),
      scope: { scope: "global" },
      draft: null,
      saved: stub.saved,
    };
    return selector ? selector(state) : state;
  };
  return { ...mod, useEditorStore: Object.assign(hook, mod.useEditorStore) };
});

vi.mock("@/stores/updates", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/updates")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useUpdatesStore.getState(),
      rows: stub.rows,
      loaded: true,
    };
    return selector ? selector(state) : state;
  };
  return { ...mod, useUpdatesStore: Object.assign(hook, mod.useUpdatesStore) };
});

const ROOTS = ["/work/vg", "/work/hyprtrade"];

const install = (scope: Scope): ObservedItem => ({
  kind: "skill",
  name: "gh",
  harness: "claude",
  scope,
  path: "/h/.claude/skills/gh",
  fileState: { state: "dir" },
  enabled: true,
  origin: null,
  description: null,
  tags: [],
  modifiedAt: null,
  vendor: null,
});

const render = () =>
  renderToStaticMarkup(
    <InstalledTable
      groups={groupItems([
        install({ scope: "global" }),
        ...ROOTS.map((root) => install({ scope: "project", root })),
      ])}
      provenance={[]}
      scanning={false}
      hasAnyItems={true}
      onClearFilters={() => {}}
      onBrowse={() => {}}
    />,
  );

beforeEach(() => {
  stub.saved = plainManifests();
  stub.rows = [null, ...ROOTS].map((root) =>
    updateRow("gh", root, { updateAvailable: false }),
  );
});

// The key to a colour is noise when nothing on screen carries it, and a
// missing key is a colour nobody can read.
describe("the Library table's colour key", () => {
  it("prints the key when a row is marked", () => {
    stub.saved = { ...stub.saved, "/work/vg": changed() };
    expect(render()).toContain("No changes of yours found");
  });

  it("leaves the key off when nothing is marked", () => {
    expect(render()).not.toContain("No changes of yours found");
  });
});

// The mark names a place and a surface. Both are what makes it worth
// clicking: the row already opens the package's first install.
describe("where the customized mark leads", () => {
  const nav = (manifests: Record<string, Draft>) =>
    markNav(
      { kind: "skill", name: "gh" },
      placeStandings(source({ manifests }), "skill", "gh", EVERYWHERE),
    );

  it("opens the Customize tab of the place whose settings were changed", () => {
    expect(nav({ ...plainManifests(), "/work/hyprtrade": changed() })).toEqual([
      { kind: "skill", name: "gh", scope: HYPR },
      { mode: "customize" },
    ]);
  });

  it("opens the overview where the change is in the files", () => {
    expect(nav({ ...plainManifests(), "/work/vg": forkedHere() })).toEqual([
      { kind: "skill", name: "gh", scope: VG },
      undefined,
    ]);
  });

  it("leads nowhere when no place is changed", () => {
    expect(nav(plainManifests())).toBe(null);
  });
});
