import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessId, Scope, UpdateRow } from "@/bindings";
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
  manifestsLoaded: true,
  manifestError: null as string | null,
  saving: false,
  rows: [] as unknown[],
  updatesLoaded: true,
  updatesError: null as string | null,
  inventory: null as unknown,
  scope: { scope: "project", root: "/work/vg" } as unknown,
}));

vi.mock("@/stores/editor", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/stores/editor")>();
  const hook = (selector?: (state: unknown) => unknown) => {
    const state = {
      ...mod.useEditorStore.getState(),
      scope: stub.scope,
      draft: stub.saved["/work/vg"] ?? null,
      saved: stub.saved,
      manifestsLoaded: stub.manifestsLoaded,
      manifestError: stub.manifestError,
      saving: stub.saving,
      inventory: stub.inventory,
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
      loaded: stub.updatesLoaded,
      error: stub.updatesError,
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
    <ItemCustomize kind="skill" name="gh" scopes={[VG, HYPR]} harnesses={{}} />,
  );

beforeEach(() => {
  stub.saved = { "/work/vg": changed(), "/work/hyprtrade": emptyDraft() };
  stub.manifestsLoaded = true;
  stub.manifestError = null;
  stub.saving = false;
  stub.updatesLoaded = true;
  stub.updatesError = null;
  stub.rows = [current(VG), current(HYPR)];
  stub.scope = VG;
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

  it("says a read failed rather than promising one that is still coming", () => {
    // A failed check will not retry on its own, so calling it in flight
    // leaves the chips waiting on something nobody is doing.
    stub.updatesLoaded = false;
    stub.updatesError = "no network";
    stub.saved = { "/work/vg": changed(), "/work/hyprtrade": emptyDraft() };
    const html = render();
    expect(html).toContain("hyprtrade — not checked for your changes");
    expect(html).not.toContain("still being checked");
  });

  it("says a place is still being checked rather than blaming the read", () => {
    // Arriving from anywhere but the Library, only the open place's
    // manifest is in hand at first — the others were never asked for.
    stub.saved = { "/work/vg": changed() };
    stub.manifestsLoaded = false;
    const html = render();
    expect(html).toContain("hyprtrade — still being checked");
    expect(html).not.toContain("hyprtrade — not checked for your changes");
  });

  it("says in plain sight what the dot on the open chip means", () => {
    // The chips carry a colour and a hover; a touch reader has neither.
    expect(render()).toContain(
      '<p class="text-xs text-muted-foreground">vg — customized by you</p>',
    );
  });

  it("marks only the chip whose place carries changes", () => {
    const html = render();
    const chips = html.split("<button").slice(1, 3);
    expect(chips[0]).toContain("bg-customized");
    expect(chips[1]).not.toContain("bg-customized");
  });

  it("tells two projects sharing a folder name apart", () => {
    stub.saved = {
      "/work/one/api": changed(),
      "/work/two/api": emptyDraft(),
    };
    stub.rows = [
      current({ scope: "project", root: "/work/one/api" }),
      current({ scope: "project", root: "/work/two/api" }),
    ];
    const html = renderToStaticMarkup(
      <ItemCustomize
        kind="skill"
        name="gh"
        scopes={[
          { scope: "project", root: "/work/one/api" },
          { scope: "project", root: "/work/two/api" },
        ]}
        harnesses={{}}
      />,
    );
    expect(html).toContain(">one/api<");
    expect(html).toContain(">two/api<");
  });

  // A save in flight is about one place, so landing its outcome on another
  // would attribute it to a place it is not about. Unsaved typing is not a
  // reason to shut a chip: the move carries it to its own place.
  it("shuts the chips only while a save is in flight", () => {
    const shut = () =>
      render()
        .split("<button")
        .slice(1, 3)
        .map((chip) => chip.slice(0, chip.indexOf(">")));
    // The attribute is the test, not the `disabled:` utility classes every
    // chip carries either way.
    expect(shut()[1]).not.toContain(' disabled=""');
    stub.saving = true;
    const chips = shut();
    // The open place stays clickable — going there changes nothing.
    expect(chips[0]).not.toContain(' disabled=""');
    expect(chips[1]).toContain(' disabled=""');
  });
});

// The chips move between places, and a package can be installed for one
// tool in one place and another elsewhere. The settings offered are for
// the tools that carry it *here* — settings for a tool this place does not
// install it under would be written to a rendering nobody has.
describe("the tools the Customize tab offers", () => {
  const offered = (harnesses: Record<string, HarnessId[]>) => {
    stub.inventory = {
      declaredAgents: [],
      declaredSkills: [],
      availableSkills: [],
      harnesses: ["claude", "opencode"],
      hookEvents: [],
    };
    return renderToStaticMarkup(
      <ItemCustomize
        kind="agent"
        name="rev"
        scopes={[VG, HYPR]}
        harnesses={harnesses}
      />,
    );
  };

  it("offers only the tools this place carries it under", () => {
    // The editor is pointed at /work/vg, which carries it under Claude
    // alone; the other place's OpenCode is not this place's business.
    const html = offered({
      "/work/vg": ["claude"],
      "/work/hyprtrade": ["opencode"],
    });
    expect(html).toContain("Claude Code");
    expect(html).not.toContain("OpenCode");
  });

  it("offers none where this place carries it under nothing", () => {
    const html = offered({ "/work/hyprtrade": ["opencode"] });
    expect(html).not.toContain("OpenCode");
    expect(html).not.toContain("Claude Code");
  });

  // The chips move the tab between places, so the answer has to be there
  // for every place they reach — not only the one the page was opened at.
  it("answers for the place a chip moved to", () => {
    stub.scope = HYPR;
    const html = offered({
      "/work/vg": ["claude"],
      "/work/hyprtrade": ["opencode"],
    });
    expect(html).toContain("OpenCode");
    expect(html).not.toContain("Claude Code");
  });
});
