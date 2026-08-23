// Two reads of the settings at once. Any launch starts both: this store's
// own, and the manifest pass asking who the projects are. Each waits on its
// capability and zoom calls before committing, so which lands last is not
// which was asked last — and the older list landing over a project
// registered since leaves that project reading as never added.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: {
    getSettings: vi.fn(),
    capabilityTable: vi.fn(),
    windowZoomState: vi.fn(),
  },
  ZOOM: { default: 100, min: 50, max: 200, step: 10 },
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

const projects = (roots: string[]) => ({
  status: "ok" as const,
  data: { schema: 1, projects: roots },
});

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({ settings: null, capabilities: [] });
  vi.mocked(commands.capabilityTable).mockResolvedValue([]);
  vi.mocked(commands.windowZoomState).mockResolvedValue({
    percent: 100,
    launchRefused: false,
  });
});

describe("two reads of the settings at once", () => {
  it("keeps the newer list when the older one lands last", async () => {
    const older = deferred<ReturnType<typeof projects>>();
    const newer = deferred<ReturnType<typeof projects>>();
    vi.mocked(commands.getSettings)
      .mockReturnValueOnce(older.promise as never)
      .mockReturnValueOnce(newer.promise as never);

    const first = useSettingsStore.getState().load();
    const second = useSettingsStore.getState().load();
    await settle();

    // The newer read answers first: a project has been registered since.
    newer.resolve(projects(["/work/vg"]));
    await second;
    expect(useSettingsStore.getState().settings?.projects).toEqual([
      "/work/vg",
    ]);

    // And the older one lands afterwards, carrying the list from before it.
    older.resolve(projects([]));
    await first;
    expect(useSettingsStore.getState().settings?.projects).toEqual([
      "/work/vg",
    ]);
  });
});
