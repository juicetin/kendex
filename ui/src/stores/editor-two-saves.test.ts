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

    // The write that landed re-read its place, so the marks come off the
    // file as it now is rather than as it was before either press.
    expect(useEditorStore.getState().saved.global).toEqual({
      schema: 1,
      install: {},
      "skill-instructions": { gh: "mine" },
    });
  });
});
