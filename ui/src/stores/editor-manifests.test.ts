import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { whyUnread } from "./editor-order";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: {
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    getSettings: vi.fn(),
    capabilityTable: vi.fn(),
    windowZoomState: vi.fn(),
  },
}));

beforeEach(() => {
  useSettingsStore.setState({
    settings: { schema: 1, projects: ["/work/vg"] },
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
  useEditorStore.setState({
    held: {},
    scope: { scope: "global" },
    draft: null,
    saved: {},
    manifestsLoaded: false,
    unreadPlaces: {},
    manifestsReading: false,
  });
});

// The marks read a place's manifest, and a place missing from `saved` is
// one they cannot speak for. Which of the two it is — never asked for, or
// asked for and refused — is the difference between a wait and a problem.
describe("reading every place's manifest", () => {
  it("keeps the reason a place would not read, naming the place", async () => {
    vi.mocked(commands.getManifest)
      .mockResolvedValueOnce({
        status: "ok",
        data: { manifest: null, base: null },
      })
      .mockResolvedValueOnce({ status: "error", error: "expected a table" })
      // The pass ends by re-reading the open place, whose draft is clean.
      .mockResolvedValue({
        status: "ok",
        data: { manifest: null, base: null },
      });
    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.manifestsLoaded).toBe(true);
    expect(state.saved["/work/vg"]).toBeUndefined();
    expect(whyUnread(state)).toContain("/work/vg");
    expect(whyUnread(state)).toContain("expected a table");
  });

  it("says nothing when every place read", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: null },
    });
    await useEditorStore.getState().loadAll();
    expect(whyUnread(useEditorStore.getState())).toBe(null);
    expect(useEditorStore.getState().saved["/work/vg"]).toBeDefined();
  });
});

// The pass fires from three places that overlap — app start, every window
// focus, and the note's own retry — so which one lands last must not decide
// what every place's mark says.
// A project someone unregisters is read by no later pass, so anything kept
// for it can never be answered — the note would go on naming a place the
// app no longer has, with a retry that cannot reach it.
// Reported on #1569 by review.
describe("a project that is no longer there", () => {
  it("takes its manifest and its reason with it", async () => {
    vi.mocked(commands.getManifest)
      .mockResolvedValueOnce({
        status: "ok",
        data: { manifest: null, base: null },
      })
      .mockResolvedValueOnce({
        status: "error",
        error: "expected a table",
      });
    await useEditorStore.getState().loadAll();
    const failed = useEditorStore.getState();
    expect(Object.keys(failed.unreadPlaces)).toContain("/work/vg");
    expect(whyUnread(failed)).toContain("expected a table");

    // The project is unregistered, so the next pass asks only the rest.
    useSettingsStore.setState({ settings: { schema: 1, projects: [] } });
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: null },
    });
    await useEditorStore.getState().loadAll();

    const after = useEditorStore.getState();
    expect(Object.keys(after.unreadPlaces)).toEqual([]);
    expect(whyUnread(after)).toBeNull();
    expect(Object.keys(after.saved)).toEqual(["global"]);
  });
});
