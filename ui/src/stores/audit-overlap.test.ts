// An Undo offered by a toast is pressed whenever the reader presses it,
// which can be while a package-wide action is still writing. Both go through
// the one funnel and both lower the same flag on the way out, and that flag
// is what holds the Customize tab's Save bar down. Released early, a save
// passes the outdated check and writes the pre-action manifest back over the
// write still in progress.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditView } from "@/bindings";
import { commands } from "@/bindings";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";

vi.mock("@/bindings", () => ({
  commands: {
    auditAll: vi.fn(),
    toggleItem: vi.fn(),
    dismissFindings: vi.fn(),
    revokeDismissal: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));

const scope = { scope: "global" as const };

const emptyView: AuditView = {
  scope,
  drift: [],
  plan: [],
  notes: [],
  warnings: [],
  safety: [],
  heldBack: [],
  queued: [],
};

/** A promise this test decides the moment of. */
function held<T>() {
  let release!: (value: T) => void;
  const promise = new Promise<T>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("two writes in flight at once", () => {
  beforeEach(() => {
    useAuditStore.setState({ views: [], busy: false, error: null });
    useEditorStore.setState({ scope, draft: null, dirty: false, held: {} });
    vi.clearAllMocks();
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "error",
      error: "no manifest here",
    });
  });

  it("keeps the Save bar down until the last one is finished", async () => {
    const first = held<{ status: "ok"; data: AuditView }>();
    const second = held<{ status: "ok"; data: AuditView }>();
    vi.mocked(commands.toggleItem)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const slow = useAuditStore.getState().toggle(scope, "skill", "gh", false);
    const quick = useAuditStore.getState().toggle(scope, "skill", "rg", false);
    await settle();
    expect(useAuditStore.getState().busy).toBe(true);

    // The one pressed second is the one that finishes first.
    second.release({ status: "ok", data: emptyView });
    await quick;
    await settle();
    expect(useAuditStore.getState().busy).toBe(true);

    first.release({ status: "ok", data: emptyView });
    await slow;
    await settle();
    expect(useAuditStore.getState().busy).toBe(false);
  });

  // Settling a finding raises the same flag by its own hand rather than
  // through the funnel, so counting only the funnel's writers left this pair
  // lowering it over each other exactly as before.
  it("counts a dismissal beside a package-wide action", async () => {
    const toggling = held<{ status: "ok"; data: AuditView }>();
    const settling = held<{
      status: "ok";
      data: { view: AuditView; records: [] };
    }>();
    vi.mocked(commands.toggleItem).mockReturnValueOnce(toggling.promise);
    vi.mocked(commands.dismissFindings).mockReturnValueOnce(settling.promise);

    const slow = useAuditStore.getState().toggle(scope, "skill", "gh", false);
    const quick = useAuditStore
      .getState()
      .dismiss(scope, ["token"], "intended");
    await settle();
    expect(useAuditStore.getState().busy).toBe(true);

    settling.release({ status: "ok", data: { view: emptyView, records: [] } });
    await quick;
    await settle();
    expect(useAuditStore.getState().busy).toBe(true);

    toggling.release({ status: "ok", data: emptyView });
    await slow;
    await settle();
    expect(useAuditStore.getState().busy).toBe(false);
  });
});
