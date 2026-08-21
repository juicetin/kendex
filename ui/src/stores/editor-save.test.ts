// What a person sees after pressing Save. Every other test around this
// store pins a refusal or a race — which read wins, which save is refused —
// and none of them pressed Save on an ordinary draft and looked at what was
// left on screen. `dirty` is that answer: the Save bar renders on it and the
// place chips are disabled by it, so a save that leaves it up leaves the
// feature's main action unfinished.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AuditView_Serialize,
  Manifest_Serialize,
  Scope,
} from "@/bindings";
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

/** The Customize tab, opened and typed in. */
const typedIn = (note: string) => {
  useEditorStore.setState({
    scope,
    draft: { schema: 1, install: {} },
    dirty: false,
    outdated: null,
    saved: {},
    error: null,
  });
  useEditorStore
    .getState()
    .edit((draft) => setInstruction(draft, "skill-instructions", "gh", note));
  expect(useEditorStore.getState().dirty).toBe(true);
};

beforeEach(() => {
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
    data: manifest("mine"),
  });
  vi.mocked(commands.updateManifest).mockResolvedValue({
    status: "ok",
    data: audited(),
  });
});

describe("saving a customization", () => {
  it("writes what is on screen and leaves nothing unsaved", async () => {
    typedIn("mine");

    await useEditorStore.getState().save();

    const after = useEditorStore.getState();
    expect(commands.updateManifest).toHaveBeenCalledWith(scope, {
      schema: 1,
      install: {},
      "skill-instructions": { gh: "mine" },
    });
    // The Save bar renders on this, and the place chips are disabled by it.
    expect(after.dirty).toBe(false);
    expect(after.error).toBeNull();
    // And the marks read what the file now holds.
    expect(after.saved.global).toEqual(manifest("mine"));
  });

  it("keeps typing that arrived while the write was away", async () => {
    typedIn("first");
    const write =
      deferred<Awaited<ReturnType<typeof commands.updateManifest>>>();
    vi.mocked(commands.updateManifest).mockReturnValueOnce(write.promise);

    const saving = useEditorStore.getState().save();
    // A second thought, typed before the write answers.
    useEditorStore
      .getState()
      .edit((draft) =>
        setInstruction(draft, "skill-instructions", "gh", "second"),
      );
    write.resolve({ status: "ok", data: audited() });
    await saving;

    const after = useEditorStore.getState();
    expect(after.draft?.["skill-instructions"]).toEqual({ gh: "second" });
    // Newer than the file, so it is still the user's to save.
    expect(after.dirty).toBe(true);
  });

  it("leaves the draft unsaved when the write is refused", async () => {
    typedIn("mine");
    vi.mocked(commands.updateManifest).mockResolvedValue({
      status: "error",
      error: "the settings file would not parse",
    });

    await useEditorStore.getState().save();

    const after = useEditorStore.getState();
    expect(after.dirty).toBe(true);
    expect(after.error).toContain("would not parse");
  });
});
