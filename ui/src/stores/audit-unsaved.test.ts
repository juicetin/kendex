import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditView } from "@/bindings";
import { commands } from "@/bindings";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";
import { useProblemsStore } from "./problems";

vi.mock("@/bindings", () => ({
  commands: {
    auditAll: vi.fn(),
    applyPlan: vi.fn(),
    adoptItem: vi.fn(),
    toggleItem: vi.fn(),
    removeItem: vi.fn(),
    dismissFindings: vi.fn(),
  },
}));

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));

const globalScope = { scope: "global" as const };

const emptyView: AuditView = {
  scope: globalScope,
  drift: [],
  plan: [],
  notes: [],
  warnings: [],
  safety: [],
  heldBack: [],
  queued: [],
};

// Moving between places parks typing rather than dropping it, so the copy
// that a write would strand can be waiting behind another place. Apply,
// adopt, toggle and remove all rewrite the file that copy came from.
describe("an audit mutation beside unsaved customization", () => {
  beforeEach(() => {
    useAuditStore.setState({
      views: [],
      auditing: false,
      error: null,
      busy: false,
    });
    useEditorStore.setState({
      scope: { scope: "project", root: "/work/vg" },
      draft: null,
      dirty: false,
      held: {},
    });
    vi.clearAllMocks();
  });

  it("refuses while typing for that place waits behind another one", async () => {
    useEditorStore.setState({
      held: {
        global: {
          scope: globalScope,
          draft: { schema: 1, install: {} },
          base: "read-earlier",
        },
      },
    });
    await useAuditStore.getState().toggle(globalScope, "skill", "gh", false);
    expect(commands.toggleItem).not.toHaveBeenCalled();
    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.title).toContain("Save your");
    // The unsaved copy is not on screen, so the way back to it is named.
    expect(dialog.steps?.[0]).toContain("Personal");
  });

  it("goes ahead when the unsaved typing is about another place", async () => {
    useEditorStore.setState({
      scope: { scope: "project", root: "/work/vg" },
      draft: { schema: 1, install: {} },
      dirty: true,
      held: {},
    });
    vi.mocked(commands.toggleItem).mockResolvedValue({
      status: "ok",
      data: emptyView,
    });
    await useAuditStore.getState().toggle(globalScope, "skill", "gh", false);
    expect(commands.toggleItem).toHaveBeenCalled();
  });
});

// Settling a finding writes the same file the rest of them write, and it
// went round the funnel rather than through it.
describe("a dismissal beside unsaved customization", () => {
  it("refuses while typing for that place waits behind another one", async () => {
    useAuditStore.setState({ views: [], busy: false, error: null });
    useEditorStore.setState({
      scope: { scope: "project", root: "/work/vg" },
      draft: null,
      dirty: false,
      held: {
        global: {
          scope: globalScope,
          draft: { schema: 1, install: {} },
          base: "read-earlier",
        },
      },
    });
    vi.clearAllMocks();
    await useAuditStore.getState().dismiss(globalScope, ["t"], "intended");
    expect(commands.dismissFindings).not.toHaveBeenCalled();
    expect(useAuditStore.getState().busy).toBe(false);
  });
});
