import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { updateRow } from "@/components/updates-test-rows";
import { PackagePage } from "./package";
import { freshWorld, type PageWorld } from "./package-test-world";

// The page's children are mocked to hand back the props they were given.
// What is pinned here is the page's own destructure, not the helper behind
// it: a test of the helper alone passes whether or not the page still asks
// it the right question, and asking the wrong one is how the page comes to
// describe a place the reader never opened.
const seen = vi.hoisted(() => ({
  body: null as Record<string, unknown> | null,
  header: null as Record<string, unknown> | null,
  actions: null as Record<string, unknown> | null,
  customize: null as Record<string, unknown> | null,
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
  ItemCustomize: (props: Record<string, unknown>) => {
    seen.customize = props;
    return null;
  },
}));
vi.mock("@/components/marks-note", () => ({ MarksNote: () => null }));
vi.mock("@/components/package/remove-dialog", () => ({
  RemoveDialog: () => null,
}));

const world = vi.hoisted(() => ({ at: null as unknown as PageWorld }));

vi.mock("@/components/package/use-package-data", async (importOriginal) => {
  const mod =
    await importOriginal<
      typeof import("@/components/package/use-package-data")
    >();
  return {
    ...mod,
    usePackageData: () => ({
      meta: world.at.meta,
      files: [],
      versions: world.at.versions,
      load: () => {},
    }),
    usePackageDiff: () => null,
    useManifestBusy: () => false,
  };
});

vi.mock("@/stores/nav", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/nav")>();
  const { stubbed } = await import("./package-test-world");
  return {
    ...mod,
    useNavStore: stubbed(mod.useNavStore, () => ({
      packageRef: { kind: "skill", name: "gh", scope: world.at.scope },
      packageView: world.at.opened,
      clearPackageView: () => {},
      back: () => {},
    })),
  };
});

vi.mock("@/stores/scan", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/scan")>();
  const { scanned, stubbed } = await import("./package-test-world");
  return {
    ...mod,
    useScanStore: stubbed(mod.useScanStore, () => ({ result: scanned() })),
  };
});

vi.mock("@/stores/editor", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/editor")>();
  const { stubbed } = await import("./package-test-world");
  return {
    ...mod,
    useEditorStore: stubbed(mod.useEditorStore, () => ({
      scope: world.at.editorScope,
      draft: null,
      saved: world.at.saved,
      held: world.at.held,
      saving: false,
      manifestsLoaded: true,
      manifestError: null,
      openScope: async () => {},
    })),
  };
});

vi.mock("@/stores/updates", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/updates")>();
  const { stubbed } = await import("./package-test-world");
  return {
    ...mod,
    useUpdatesStore: stubbed(mod.useUpdatesStore, () => ({
      rows: world.at.rows,
      loaded: true,
      checking: world.at.checking,
      busy: false,
      error: null,
    })),
  };
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
  seen.customize = null;
  world.at = freshWorld();
});

// When the page offers to bring this place up to date. The button applies
// the revision the last read named and writes the file the Customize tab
// may be holding, so it waits on both — a check still arriving, and an
// edit this place is holding back.
describe("the package page's Update button", () => {
  it("does not offer an update while a check is still on its way", () => {
    world.at.rows = [
      updateRow("gh", "/work/vg", { updateAvailable: true, canDiscard: true }),
    ];
    expect(render().actions.updateAvailable).toBe(true);

    world.at.checking = true;
    expect(render().actions.updateAvailable).toBe(false);
  });

  it("does not offer an update for a place its edits are holding", () => {
    // The control: everything the Update button needs is in place.
    world.at.rows = [
      updateRow("gh", "/work/vg", { updateAvailable: true, canDiscard: true }),
    ];
    expect(render().actions.updateAvailable).toBe(true);
    // The edit is what holds it, and the engine would refuse the apply.
    world.at.rows = [
      updateRow("gh", "/work/vg", {
        updateAvailable: true,
        blockedByLocalEdit: true,
        canDiscard: true,
      }),
    ];
    expect(render().actions.updateAvailable).toBe(false);
  });
});
