import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Manifest_Serialize, Scope } from "@/bindings";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";

vi.mock("@/bindings", () => ({
  commands: {
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

const A: Scope = { scope: "project", root: "/work/a" };
const B: Scope = { scope: "project", root: "/work/b" };

const manifest = (note: string): Manifest_Serialize => ({
  schema: 1,
  install: {},
  "skill-instructions": { gh: note },
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((keep) => {
    resolve = keep;
  });
  return { promise, resolve };
}

beforeEach(() => {
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
    dirty: false,
    error: null,
  });
});

// The per-place marks read `scope` and `draft` as one answer, and a save
// sends that pair to a file. A read that lands after a newer one must not
// be able to make them disagree.
describe("switching place while a read is in flight", () => {
  it("never lets a superseded read become the draft on screen", async () => {
    const slow = deferred<Awaited<ReturnType<typeof commands.getManifest>>>();
    const quick = deferred<Awaited<ReturnType<typeof commands.getManifest>>>();
    vi.mocked(commands.getManifest)
      .mockImplementationOnce(() => slow.promise)
      .mockImplementationOnce(() => quick.promise);

    const first = useEditorStore.getState().setScope(A);
    const second = useEditorStore.getState().setScope(B);
    quick.resolve({ status: "ok", data: manifest("b") });
    await second;
    slow.resolve({ status: "ok", data: manifest("a") });
    await first;

    const state = useEditorStore.getState();
    expect(state.scope).toEqual(B);
    expect(state.draft?.["skill-instructions"]).toEqual({ gh: "b" });
    // The late read still knows its own place, so the marks keep it.
    expect(state.saved["/work/a"]?.["skill-instructions"]).toEqual({ gh: "a" });
  });

  it("never lets a superseded failure blank the draft on screen", async () => {
    const slow = deferred<Awaited<ReturnType<typeof commands.getManifest>>>();
    const quick = deferred<Awaited<ReturnType<typeof commands.getManifest>>>();
    vi.mocked(commands.getManifest)
      .mockImplementationOnce(() => slow.promise)
      .mockImplementationOnce(() => quick.promise);

    const first = useEditorStore.getState().setScope(A);
    const second = useEditorStore.getState().setScope(B);
    quick.resolve({ status: "ok", data: manifest("b") });
    await second;
    slow.resolve({ status: "error", error: "unreadable" });
    await first;

    const state = useEditorStore.getState();
    expect(state.draft?.["skill-instructions"]).toEqual({ gh: "b" });
    expect(state.error).toBe(null);
  });

  it("saves the place the draft on screen belongs to", async () => {
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: manifest("b"),
    });
    vi.mocked(commands.updateManifest).mockResolvedValue({
      status: "error",
      error: "stop here",
    });
    await useEditorStore.getState().setScope(B);
    await useEditorStore.getState().save();
    expect(vi.mocked(commands.updateManifest).mock.calls[0]?.[0]).toEqual(B);
  });
});
