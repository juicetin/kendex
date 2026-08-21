import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: { getManifest: vi.fn(), editorInventory: vi.fn() },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((keep, fail) => {
    resolve = keep;
    reject = fail;
  });
  return { promise, resolve, reject };
}

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
    scope: { scope: "global" },
    draft: null,
    saved: {},
    manifestsLoaded: false,
    manifestError: null,
    manifestsReading: false,
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

// The pass fires from three places that overlap — app start, every window
// focus, and the note's own retry — so which one lands last must not decide
// what every place's mark says.
describe("passes that overlap", () => {
  it("never lets an older pass revert a place a newer read answered for", async () => {
    const slow = deferred<Awaited<ReturnType<typeof commands.getManifest>>>();
    const issued = deferred<null>();
    let holding = true;
    vi.mocked(commands.getManifest).mockImplementation(() => {
      if (!holding) {
        return Promise.resolve({
          status: "ok",
          data: {
            schema: 1,
            install: {},
            "skill-instructions": { gh: "typed" },
          },
        });
      }
      issued.resolve(null);
      return slow.promise;
    });

    const pass = useEditorStore.getState().loadAll();
    await issued.promise;
    // A place read on its own while the pass is still in flight.
    holding = false;
    await useEditorStore.getState().setScope({
      scope: "project",
      root: "/work/vg",
    });
    slow.resolve({ status: "ok", data: null });
    await pass;

    expect(
      useEditorStore.getState().saved["/work/vg"]?.["skill-instructions"],
    ).toEqual({ gh: "typed" });
  });

  it("keeps a place's last good manifest when a later pass cannot read it", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { schema: 1, install: {}, "skill-instructions": { gh: "mine" } },
    });
    await useEditorStore.getState().loadAll();

    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "error",
      error: "expected a table",
    });
    await useEditorStore.getState().loadAll();

    const state = useEditorStore.getState();
    expect(state.saved["/work/vg"]?.["skill-instructions"]).toEqual({
      gh: "mine",
    });
    expect(state.manifestError).toContain("/work/vg");
  });

  it("treats a rejected read as a read that failed, not one still running", async () => {
    vi.mocked(commands.getManifest).mockRejectedValue(new Error("no channel"));
    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.manifestsReading).toBe(false);
    expect(state.manifestError).toContain("no channel");
    expect(state.manifestsLoaded).toBe(false);
  });

  it("hands back the same object when a re-read says the same thing", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { schema: 1, install: {}, "skill-instructions": { gh: "mine" } },
    });
    await useEditorStore.getState().loadAll();
    const first = useEditorStore.getState().saved;
    await useEditorStore.getState().loadAll();
    // Identity is what the screens joining on this memoize against, so an
    // equal copy would re-render every row for news that is not news.
    expect(useEditorStore.getState().saved).toBe(first);
  });
});
