// The per-place marks ask whether a read of the update standing is running,
// so that a place with no row of its own reads as being looked at rather
// than as one nobody asked about. The project list starts an ordinary read
// — `load`, not `check` — and that is the read a project registered just
// now is waiting on.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { updatesReading, useUpdatesStore } from "./updates";

vi.mock("@/bindings", () => ({
  commands: {
    updatesOverview: vi.fn(),
    updatesRefresh: vi.fn(),
    updateSetIgnored: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((keep) => {
    resolve = keep;
  });
  return { promise, resolve };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

const empty = { status: "ok" as const, data: { rows: [], warnings: [] } };

const reading = () => updatesReading(useUpdatesStore.getState());

beforeEach(() => {
  vi.clearAllMocks();
  useUpdatesStore.setState({
    rows: [],
    warnings: [],
    busy: false,
    checking: false,
    reading: false,
    loaded: false,
    error: null,
  });
});

describe("whether a read of the standing is running", () => {
  it("counts the ordinary read the project list starts", async () => {
    const read = deferred<typeof empty>();
    vi.mocked(commands.updatesOverview).mockReturnValue(read.promise);

    expect(reading()).toBe(false);
    const done = useUpdatesStore.getState().load();
    await settle();
    expect(reading()).toBe(true);

    read.resolve(empty);
    await done;
    expect(reading()).toBe(false);
  });

  it("stays up until the last of several lands", async () => {
    const first = deferred<typeof empty>();
    const second = deferred<typeof empty>();
    vi.mocked(commands.updatesOverview)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const one = useUpdatesStore.getState().load();
    const two = useUpdatesStore.getState().load();
    await settle();
    expect(reading()).toBe(true);

    first.resolve(empty);
    await one;
    // One still running: the flag coming down here would call the place it
    // is about unchecked while its own read is on its way.
    expect(reading()).toBe(true);

    second.resolve(empty);
    await two;
    expect(reading()).toBe(false);
  });

  it("comes down when the read fails", async () => {
    vi.mocked(commands.updatesOverview).mockRejectedValue(
      new Error("the channel closed"),
    );

    await useUpdatesStore.getState().load();

    expect(reading()).toBe(false);
    expect(useUpdatesStore.getState().error).toContain("the channel closed");
  });

  // The buttons that apply a revision are gated on `checking`, and an
  // ordinary background read is no reason to take those away.
  it("is not the flag that holds the Update buttons", async () => {
    const read = deferred<typeof empty>();
    vi.mocked(commands.updatesOverview).mockReturnValue(read.promise);
    useUpdatesStore.setState({ loaded: true });

    const done = useUpdatesStore.getState().load();
    await settle();
    expect(useUpdatesStore.getState().checking).toBe(false);

    read.resolve(empty);
    await done;
  });
});

// Muting an update is a preference of this machine's, recorded in kendex's
// own settings. It touches no project's file, so the things owed to a file
// a save would write back are not owed here: a draft open in Customize
// does not block it, and it does not hold the Save bar down.
describe("muting an update", () => {
  it("is not refused by unsaved customization", async () => {
    vi.mocked(commands.updateSetIgnored).mockResolvedValue({
      status: "ok",
      data: { rows: [], warnings: [] },
    });
    // Typing waiting in the very place the row belongs to.
    useEditorStore.setState({
      scope: { scope: "global" },
      draft: { schema: 1, install: {}, "skill-instructions": { gh: "mine" } },
      dirty: true,
      held: {},
    });

    await useUpdatesStore.getState().setIgnored(
      {
        scope: { scope: "global" },
        kind: "skill",
        name: "gh",
        repo: "owner/catalog",
      } as never,
      true,
    );

    expect(commands.updateSetIgnored).toHaveBeenCalled();
  });
});
