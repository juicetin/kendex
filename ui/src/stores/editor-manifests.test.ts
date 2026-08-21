import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
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
    held: {},
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
    expect(state.manifestError).toContain("/work/vg");
    expect(state.manifestError).toContain("expected a table");
  });

  it("says nothing when every place read", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: null },
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
            manifest: {
              schema: 1,
              install: {},
              "skill-instructions": { gh: "typed" },
            },
            base: "typed",
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
    slow.resolve({ status: "ok", data: { manifest: null, base: null } });
    await pass;

    expect(
      useEditorStore.getState().saved["/work/vg"]?.["skill-instructions"],
    ).toEqual({ gh: "typed" });
  });

  it("keeps a place's last good manifest when a later pass cannot read it", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: {
        manifest: {
          schema: 1,
          install: {},
          "skill-instructions": { gh: "mine" },
        },
        base: "mine-base",
      },
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
    // Every place refusing is still a pass that ran: each read answers for
    // its own place, and the reason is named per place.
    vi.mocked(commands.getManifest).mockRejectedValue(new Error("no channel"));
    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.manifestsReading).toBe(false);
    expect(state.manifestError).toContain("no channel");
    expect(state.manifestsLoaded).toBe(true);
  });

  it("says the pass itself failed when it could not even list the places", async () => {
    useSettingsStore.setState({ settings: null });
    vi.spyOn(useSettingsStore.getState(), "load").mockRejectedValue(
      new Error("settings unreadable"),
    );
    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.manifestsReading).toBe(false);
    expect(state.manifestError).toContain("settings unreadable");
    expect(state.manifestsLoaded).toBe(false);
  });

  it("hands back the same object when a re-read says the same thing", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: {
        manifest: {
          schema: 1,
          install: {},
          "skill-instructions": { gh: "mine" },
        },
        base: "mine-base",
      },
    });
    await useEditorStore.getState().loadAll();
    const first = useEditorStore.getState().saved;
    await useEditorStore.getState().loadAll();
    // Identity is what the screens joining on this memoize against, so an
    // equal copy would re-render every row for news that is not news.
    expect(useEditorStore.getState().saved).toBe(first);
  });

  it("keeps the places that read when one of them rejects", async () => {
    // Each read answers for its own place: one bad manifest taking the
    // whole batch down would make every readable place unknown.
    useSettingsStore.setState({
      settings: { schema: 1, projects: ["/work/vg", "/work/hyprtrade"] },
    });
    vi.mocked(commands.getManifest).mockImplementation((scope) =>
      scope.scope === "project" && scope.root === "/work/vg"
        ? Promise.reject(new Error("no channel"))
        : Promise.resolve({
            status: "ok",
            data: {
              manifest: {
                schema: 1,
                install: {},
                "skill-instructions": { gh: "read" },
              },
              base: "read",
            },
          }),
    );

    await useEditorStore.getState().loadAll();
    const state = useEditorStore.getState();
    expect(state.saved.global?.["skill-instructions"]).toEqual({ gh: "read" });
    expect(state.saved["/work/hyprtrade"]?.["skill-instructions"]).toEqual({
      gh: "read",
    });
    expect(state.saved["/work/vg"]).toBeUndefined();
    expect(state.manifestError).toContain("/work/vg");
    expect(state.manifestError).toContain("no channel");
    expect(state.manifestsLoaded).toBe(true);
  });
});
