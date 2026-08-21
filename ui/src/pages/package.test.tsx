import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { updateRow } from "@/components/updates-test-rows";
import type { PlaceStanding } from "@/lib/customized-places";
import { observedItem } from "@/lib/observed-test-item";
import { PackagePage } from "./package";

const VG = { scope: "project", root: "/work/vg" } as const;
const HYPR = { scope: "project", root: "/work/hyprtrade" } as const;

// The page's children are mocked to hand back the props they were given.
// What is pinned here is the page's own destructure — every one of these
// facts reverted green while only the helper behind them was covered.
const seen = vi.hoisted(() => ({
  body: null as Record<string, unknown> | null,
  header: null as Record<string, unknown> | null,
  actions: null as Record<string, unknown> | null,
}));
vi.mock("@/components/package/package-body", () => ({
  PackageBody: (props: Record<string, unknown>) => {
    seen.body = props;
    return null;
  },
}));
vi.mock("@/components/package/package-header", () => ({
  PackageHeader: (props: Record<string, unknown>) => {
    seen.header = props;
    return props.action as never;
  },
}));
vi.mock("@/components/package/package-actions", () => ({
  PackageActions: (props: Record<string, unknown>) => {
    seen.actions = props;
    return null;
  },
}));
vi.mock("@/components/customize/item-customize", () => ({
  ItemCustomize: () => null,
}));
vi.mock("@/components/marks-note", () => ({ MarksNote: () => null }));
vi.mock("@/components/package/remove-dialog", () => ({
  RemoveDialog: () => null,
}));

const stub = vi.hoisted(() => ({
  scope: { scope: "project", root: "/work/vg" } as unknown,
  editorScope: { scope: "project", root: "/work/hyprtrade" } as unknown,
  rows: [] as unknown[],
  saved: {} as Record<string, unknown>,
  // Enough for the Update button to be offered: a newer version exists and
  // the package's own record was read.
  meta: null as unknown,
  versions: [] as unknown[],
}));

vi.mock("@/components/package/use-package-data", async (importOriginal) => {
  const mod =
    await importOriginal<
      typeof import("@/components/package/use-package-data")
    >();
  return {
    ...mod,
    usePackageData: () => ({
      meta: stub.meta,
      files: [],
      versions: stub.versions,
      load: () => {},
    }),
    usePackageDiff: () => null,
    useManifestBusy: () => false,
  };
});

vi.mock("@/stores/nav", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/nav")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useNavStore.getState(),
      packageRef: { kind: "skill", name: "gh", scope: stub.scope },
      packageView: null,
      clearPackageView: () => {},
      back: () => {},
    };
    return selector ? selector(state) : state;
  };
  return { ...mod, useNavStore: Object.assign(hook, mod.useNavStore) };
});

vi.mock("@/stores/scan", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/scan")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useScanStore.getState(),
      result: {
        items: [
          observedItem({ name: "gh", scope: VG, path: "/work/vg/gh" }),
          observedItem({ name: "gh", scope: HYPR, path: "/work/hyprtrade/gh" }),
        ],
      },
    };
    return selector ? selector(state) : state;
  };
  return { ...mod, useScanStore: Object.assign(hook, mod.useScanStore) };
});

vi.mock("@/stores/editor", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/editor")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useEditorStore.getState(),
      scope: stub.editorScope,
      draft: null,
      saved: stub.saved,
      manifestsLoaded: true,
      manifestError: null,
      openScope: async () => {},
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
      error: null,
    };
    return selector ? selector(state) : state;
  };
  return { ...mod, useUpdatesStore: Object.assign(hook, mod.useUpdatesStore) };
});

const render = () => {
  renderToStaticMarkup(<PackagePage />);
  if (!seen.body || !seen.header || !seen.actions)
    throw new Error("the page rendered without its body, header or actions");
  return { body: seen.body, header: seen.header, actions: seen.actions };
};

beforeEach(() => {
  seen.body = null;
  seen.header = null;
  seen.actions = null;
  stub.scope = VG;
  stub.editorScope = HYPR;
  stub.rows = [];
  stub.saved = { "/work/vg": {}, "/work/hyprtrade": {} };
  stub.meta = { rev: null, fork: null };
  stub.versions = [
    {
      id: "b".repeat(40),
      label: "v2",
      date: "2026-08-01",
      summary: "newer",
      installed: false,
      newerThanInstalled: true,
    },
    {
      id: "a".repeat(40),
      label: "v1",
      date: "2026-07-01",
      summary: "installed",
      installed: true,
      newerThanInstalled: false,
    },
  ];
});

describe("what the package page is about", () => {
  it("takes its installation from the place it was opened at", () => {
    expect((render().body.primary as { path: string }).path).toBe(
      "/work/vg/gh",
    );
    stub.scope = HYPR;
    expect((render().body.primary as { path: string }).path).toBe(
      "/work/hyprtrade/gh",
    );
  });

  it("speaks for the place it was opened at, not the last one edited", () => {
    // openScope has not landed in a static render, so the editor's scope —
    // carried over from whatever package was open before — must not win.
    expect((render().header.place as PlaceStanding).scope).toEqual(VG);
  });

  it("reads its edited-files notice off the place it was opened at", () => {
    stub.rows = [
      updateRow("gh", "/work/vg", {
        updateAvailable: false,
        blockedByLocalEdit: true,
      }),
      updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
    ];
    expect((render().body.editedRow as { scope: unknown }).scope).toEqual(VG);
    stub.scope = HYPR;
    expect(render().body.editedRow).toBe(null);
  });

  it("does not offer an update for a place its edits are holding", () => {
    // The control: everything the Update button needs is in place.
    stub.rows = [
      updateRow("gh", "/work/vg", { updateAvailable: true, canDiscard: true }),
    ];
    expect(render().actions.updateAvailable).toBe(true);
    // The edit is what holds it, and the engine would refuse the apply.
    stub.rows = [
      updateRow("gh", "/work/vg", {
        updateAvailable: true,
        blockedByLocalEdit: true,
        canDiscard: true,
      }),
    ];
    expect(render().actions.updateAvailable).toBe(false);
  });
});
