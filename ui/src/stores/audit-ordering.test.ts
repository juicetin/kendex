// Whose answer stands when several are in the air at once. A mutation
// writes one place and reads it back; a refresh reads every place. Landing
// order is the machine's to decide, so without a rank the last to arrive
// wins — and a read that began before a write puts the pre-write account
// of that place back, resurrecting findings someone just settled.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditView } from "@/bindings";
import { commands } from "@/bindings";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";

vi.mock("@/bindings", () => ({
  commands: {
    auditAll: vi.fn(),
    toggleItem: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("./problems", () => ({
  useProblemsStore: { getState: () => ({ showError: vi.fn() }) },
}));
vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));

const scope = { scope: "global" as const };

const viewWith = (notes: string[]): AuditView => ({
  scope,
  drift: [],
  plan: [],
  notes,
  warnings: [],
  safety: [],
  heldBack: [],
  queued: [],
});

function held<T>() {
  let release!: (value: T) => void;
  const promise = new Promise<T>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const notes = () => useAuditStore.getState().views[0]?.notes ?? [];

beforeEach(() => {
  useAuditStore.setState({
    // replaceView writes over the place's existing account rather than
    // adding one, so the place has to be on the books first.
    views: [viewWith(["before"])],
    busy: false,
    error: null,
    auditing: false,
    auditedAt: null,
  });
  useEditorStore.setState({ scope, draft: null, dirty: false, held: {} });
  vi.clearAllMocks();
  vi.mocked(commands.getManifest).mockResolvedValue({
    status: "error",
    error: "no manifest here",
  });
});

describe("a read that began before a write", () => {
  it("does not put the pre-write account of the place back", async () => {
    const reading = held<{ status: "ok"; data: AuditView[] }>();
    vi.mocked(commands.auditAll).mockReturnValueOnce(reading.promise);
    const refreshing = useAuditStore.getState().refresh({ force: true });
    await settle();

    vi.mocked(commands.toggleItem).mockResolvedValue({
      status: "ok",
      data: viewWith(["settled"]),
    });
    await useAuditStore.getState().toggle(scope, "skill", "gh", false);
    expect(notes()).toEqual(["settled"]);

    // The read was taken before the write and knows nothing of it.
    reading.release({ status: "ok", data: [viewWith(["not settled yet"])] });
    await refreshing;
    await settle();
    expect(notes()).toEqual(["settled"]);
  });
});

describe("two writes of one place", () => {
  it("keeps the newer answer when the older one lands last", async () => {
    const first = held<{ status: "ok"; data: AuditView }>();
    const second = held<{ status: "ok"; data: AuditView }>();
    vi.mocked(commands.toggleItem)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const older = useAuditStore.getState().toggle(scope, "skill", "gh", false);
    const newer = useAuditStore.getState().toggle(scope, "skill", "gh", true);
    await settle();

    second.release({ status: "ok", data: viewWith(["newer"]) });
    await newer;
    await settle();
    expect(notes()).toEqual(["newer"]);

    first.release({ status: "ok", data: viewWith(["older"]) });
    await older;
    await settle();
    expect(notes()).toEqual(["newer"]);
  });
});

// The rank decides which successful answer stands. It must not let a
// failure silence one: a write that lost the race can still have landed on
// disk, and the newer attempt that superseded it may bring nothing back at
// all. Left there, Review goes on showing findings the file no longer has.
describe("a newer write that fails after an older one succeeded", () => {
  it("does not leave the older success suppressed", async () => {
    const winner = held<{ status: "ok"; data: AuditView }>();
    const loser = held<{ status: "error"; error: string }>();
    vi.mocked(commands.toggleItem)
      .mockReturnValueOnce(winner.promise)
      .mockReturnValueOnce(loser.promise);
    // What the disk actually holds once the first write has landed.
    vi.mocked(commands.auditAll).mockResolvedValue({
      status: "ok",
      data: [viewWith(["settled on disk"])],
    });

    const older = useAuditStore.getState().toggle(scope, "skill", "gh", false);
    const newer = useAuditStore.getState().toggle(scope, "skill", "gh", true);
    await settle();

    // The older write won the lock and wrote; its answer stands down for
    // the newer one, which then brings nothing back.
    winner.release({ status: "ok", data: viewWith(["settled"]) });
    await older;
    await settle();
    loser.release({ status: "error", error: "the place was busy" });
    await newer;
    await settle();

    expect(notes()).toEqual(["settled on disk"]);
  });
});
