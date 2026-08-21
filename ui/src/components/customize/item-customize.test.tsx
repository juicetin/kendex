import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Scope, UpdateRow } from "@/bindings";
import { updateRow } from "@/components/updates-test-rows";
import { type Draft, emptyDraft } from "@/lib/editor-draft";
import { ItemCustomize } from "./item-customize";

const VG: Scope = { scope: "project", root: "/work/vg" };
const HYPR: Scope = { scope: "project", root: "/work/hyprtrade" };

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
      scope: { scope: "project", root: "/work/vg" },
      draft: stub.saved["/work/vg"],
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

const changed = (): Draft => ({
  ...emptyDraft(),
  "skill-instructions": { gh: "use the CLI" },
});

const current = (scope: Scope): UpdateRow =>
  updateRow("gh", scope.scope === "project" ? scope.root : null, {
    updateAvailable: false,
  });

const render = () =>
  renderToStaticMarkup(
    <ItemCustomize kind="skill" name="gh" scopes={[VG, HYPR]} harnesses={[]} />,
  );

beforeEach(() => {
  stub.saved = { "/work/vg": changed(), "/work/hyprtrade": emptyDraft() };
  stub.rows = [current(VG), current(HYPR)];
});

// Switching places is how you reach a customization, so the chips carry the
// answer before the click rather than after it.
describe("the Customize tab's place chips", () => {
  it("says on each chip what is known about that place", () => {
    const html = render();
    expect(html).toContain("vg — customized by you");
    expect(html).toContain("hyprtrade — as the author wrote it");
  });

  it("says a place is not checked rather than calling it untouched", () => {
    stub.rows = [current(VG)];
    expect(render()).toContain("hyprtrade — not checked for your changes");
  });
});
