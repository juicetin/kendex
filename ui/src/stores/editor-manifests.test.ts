import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: { getManifest: vi.fn() },
}));

beforeEach(() => {
  useSettingsStore.setState({
    settings: { schema: 1, projects: ["/work/vg"] },
  });
  useEditorStore.setState({
    saved: {},
    manifestsLoaded: false,
    manifestError: null,
  });
});

// The marks read a place's manifest, and a place missing from `saved` is
// one they cannot speak for. Which of the two it is — never asked for, or
// asked for and refused — is the difference between a wait and a problem.
describe("reading every place's manifest", () => {
  it("keeps the reason a place would not read, naming the place", async () => {
    vi.mocked(commands.getManifest)
      .mockResolvedValueOnce({ status: "ok", data: null })
      .mockResolvedValueOnce({ status: "error", error: "expected a table" });
    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.manifestsLoaded).toBe(true);
    expect(state.saved["/work/vg"]).toBeUndefined();
    expect(state.manifestError).toContain("/work/vg");
    expect(state.manifestError).toContain("expected a table");
  });

  it("says nothing when every place read", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: null,
    });
    await useEditorStore.getState().loadAll();
    expect(useEditorStore.getState().manifestError).toBe(null);
    expect(useEditorStore.getState().saved["/work/vg"]).toBeDefined();
  });
});
