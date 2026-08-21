// A redirected install writes into the project it was sent to, not into the
// personal subscription its packages were browsed from — so that project is
// the place whose manifest was rewritten, and the place the editor holding a
// whole copy of a manifest has to hear about.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { useMarketplacesStore } from "./marketplaces";
import { useProblemsStore } from "./problems";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: {
    marketplaceInstall: vi.fn(),
    marketplacesOverview: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), message: vi.fn() },
}));

vi.mock("./audit", () => ({
  useAuditStore: { getState: () => ({ refresh: vi.fn() }) },
}));

vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));

const personal = { scope: "global" as const };
const project = { scope: "project" as const, root: "/w/app" };

describe("an install redirected into a project", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      settings: { schema: 1, projects: ["/w/app"] },
    });
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: null },
    });
    vi.mocked(commands.editorInventory).mockResolvedValue({
      status: "ok",
      data: {
        declaredAgents: [],
        declaredSkills: [],
        availableSkills: [],
        harnesses: [],
        hookEvents: [],
      },
    });
    vi.mocked(commands.marketplaceInstall).mockResolvedValue({
      status: "ok",
      data: [],
    });
    // The project's Customize tab, open with unsaved typing in it.
    useEditorStore.setState({
      scope: project,
      draft: { schema: 1, install: {}, "skill-instructions": { gh: "mine" } },
      dirty: true,
      outdated: null,
      saved: {},
    });
    useProblemsStore.getState().closeError();
  });

  const install = () =>
    useMarketplacesStore.getState().install({
      scope: personal,
      source: "kit",
      items: [{ kind: "skill", name: "gh" }],
      destination: project,
    });

  it("marks the project it wrote, not the subscription it was browsed from", async () => {
    await install();

    expect(commands.marketplaceInstall).toHaveBeenCalledWith(
      personal,
      "kit",
      [{ kind: "skill", name: "gh" }],
      null,
      project,
      false,
    );
    expect(useEditorStore.getState().outdated).toBe("/w/app");
  });

  it("refuses the project's next save rather than putting the manifest back", async () => {
    await install();

    vi.mocked(commands.updateManifest).mockResolvedValue({
      status: "error",
      error: { kind: "failed", message: "should never be reached" },
    });
    await useEditorStore.getState().save();

    expect(commands.updateManifest).not.toHaveBeenCalled();
    expect(useProblemsStore.getState().dialog.title).toContain(
      "changed while you typed",
    );
  });
});
