// Two saves in flight at once. Nothing stops a second press before the
// button disables, and the two race for the scope lock: whichever wins
// writes, and the other is refused. The newer save owns what the screen
// says about saving — but what a write did to the file is that write's to
// settle, whichever of them it was. Dropped, the place reads as unsaved on
// screen with its content already on disk, and the tables go on showing the
// file as it was before.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditView_Serialize, Scope } from "@/bindings";
import { commands } from "@/bindings";
import { setInstruction } from "@/lib/editor-draft";
import { useEditorStore } from "./editor";

vi.mock("@/bindings", () => ({
  commands: {
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn(), message: vi.fn() },
}));
vi.mock("./audit", () => ({
  useAuditStore: { getState: () => ({ refresh: async () => {} }) },
}));
vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: async () => {} }) },
}));

const scope: Scope = { scope: "global" };

const audited = (): AuditView_Serialize => ({
  scope,
  drift: [],
  plan: [],
  notes: [],
  warnings: [],
  safety: [],
  heldBack: [],
  queued: [],
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((keep) => {
    resolve = keep;
  });
  return { promise, resolve };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

beforeEach(async () => {
  vi.clearAllMocks();
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
  vi.mocked(commands.getManifest).mockResolvedValue({
    status: "ok",
    data: { manifest: null, base: "on-disk" },
  });
  useEditorStore.setState({
    outdated: null,
    saved: {},
    error: null,
    held: {},
    draft: null,
    base: null,
    dirty: false,
  });
  await useEditorStore.getState().setScope(scope);
  useEditorStore
    .getState()
    .edit((draft) => setInstruction(draft, "skill-instructions", "gh", "mine"));
});

describe("a second save pressed before the first answered", () => {
  // Both presses send the same draft. One wins the lock and settles the
  // place; the other is refused as stale and answers afterwards. Marking
  // the place then would refuse a save the file would take, and the way
  // out it offers is a reload over whatever has been typed since.
  it("does not mark a place its twin has already settled", async () => {
    const winner =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    const loser =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    vi.mocked(commands.updateManifest)
      .mockReturnValueOnce(winner.promise)
      .mockReturnValueOnce(loser.promise);

    const one = useEditorStore.getState().save();
    const two = useEditorStore.getState().save();
    await settle();

    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: {
        manifest: {
          schema: 1,
          install: {},
          "skill-instructions": { gh: "mine" },
        },
        base: "written",
      },
    });
    winner.resolve({
      status: "ok",
      data: { view: audited(), base: "written", wroteMore: false },
    });
    await one;
    expect(useEditorStore.getState().outdated).toBeNull();

    // And now the twin's refusal arrives, about a file that has moved on.
    loser.resolve({ status: "error", error: { kind: "stale" } });
    await two;

    const after = useEditorStore.getState();
    expect(after.outdated).toBeNull();
    expect(after.base).toBe("written");
  });

  it("settles the write that landed, even though it is not the newest", async () => {
    const first =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    const second =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    vi.mocked(commands.updateManifest)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const one = useEditorStore.getState().save();
    const two = useEditorStore.getState().save();
    await settle();

    // What the file holds once the first save's write has landed.
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: {
        manifest: {
          schema: 1,
          install: {},
          "skill-instructions": { gh: "mine" },
        },
        base: "written",
      },
    });
    // The first wins the lock; the second finds the file already moved.
    first.resolve({
      status: "ok",
      data: { view: audited(), base: "written", wroteMore: false },
    });
    second.resolve({
      status: "error",
      error: { kind: "stale" },
    });
    await Promise.all([one, two]);

    const after = useEditorStore.getState();
    // The write that landed re-read its place, so the marks come off the
    // file as it now is rather than as it was before either press.
    expect(after.saved.global).toEqual({
      schema: 1,
      install: {},
      "skill-instructions": { gh: "mine" },
    });
    // And the copy on screen is settled by it: the content is on disk, so
    // the Save bar comes down and the base is the file's rather than the
    // one from before either press.
    expect(after.dirty).toBe(false);
    expect(after.base).toBe("written");
  });
});

// A save's answer can arrive after the reader has moved to another place.
// The write is still about the place it wrote, and the copy it came from is
// parked, not gone — the store's own base by then describes the place now
// on screen. Compared against that, a refusal reads as a place someone else
// already settled, and the parked copy keeps no mark: reopened, it offers a
// draft the file will refuse all over again.
describe("a refusal that lands after the reader has moved on", () => {
  it("marks the place that was written, not the one now on screen", async () => {
    const elsewhere: Scope = { scope: "project", root: "/work/vg" };
    const late =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    vi.mocked(commands.updateManifest).mockReturnValueOnce(late.promise);

    const saving = useEditorStore.getState().save();
    await settle();

    // Moving parks the copy this save came from and reads the new place,
    // which brings its own base with it.
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: "another-place" },
    });
    await useEditorStore.getState().setScope(elsewhere);
    expect(useEditorStore.getState().base).toBe("another-place");

    late.resolve({
      status: "error",
      error: { kind: "stale" },
    });
    await saving;
    await settle();

    expect(useEditorStore.getState().outdated).toBe("global");
  });
});
