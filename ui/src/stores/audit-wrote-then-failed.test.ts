// Actions that write before they can say what they wrote. A dismissal is
// applied and then read back to build its undo, and an undo of several
// records is one write per record — so both can fail with the file already
// changed. Told only that the action failed, the app says nothing was
// changed and leaves the marks drawn from a manifest that moved. The save
// that follows is still refused on the file's own base, so nothing is
// overwritten; what is wrong is what the reader is told, and what the marks
// go on showing until something else re-reads.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditView, Manifest_Serialize } from "@/bindings";
import { commands } from "@/bindings";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";
import { useProblemsStore } from "./problems";

vi.mock("@/bindings", () => ({
  commands: {
    auditAll: vi.fn(),
    adoptItem: vi.fn(),
    dismissFindings: vi.fn(),
    revokeDismissal: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn(), message: vi.fn() },
}));
vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));
vi.mock("./settings", () => ({
  useSettingsStore: {
    getState: () => ({ settings: { schema: 1, projects: [] } }),
    setState: () => {},
  },
}));

const scope = { scope: "global" as const };

const view: AuditView = {
  scope,
  drift: [],
  plan: [],
  notes: [],
  warnings: [],
  safety: [],
  heldBack: [],
  queued: [],
};

/** The file as the dismissal left it — what the marks should end up on. */
const settled: Manifest_Serialize = {
  schema: 1,
  install: {},
  "skill-instructions": { gh: "after the dismissal" },
};

beforeEach(() => {
  vi.clearAllMocks();
  useAuditStore.setState({
    views: [],
    auditing: false,
    error: null,
    busy: false,
  });
  useEditorStore.setState({
    scope,
    draft: null,
    dirty: false,
    held: {},
    saved: { global: { schema: 1, install: {} } },
    base: "before",
    outdated: null,
    unreadPlaces: {},
  });
  useProblemsStore.getState().closeError();
  vi.mocked(commands.auditAll).mockResolvedValue({
    status: "ok",
    data: [view],
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
  // Whatever re-reads the place finds the file the write left.
  vi.mocked(commands.getManifest).mockResolvedValue({
    status: "ok",
    data: { manifest: settled, base: "after" },
  });
});

describe("a dismissal that landed and then could not be described", () => {
  it("tells the editor the file moved, and says the decision stands", async () => {
    vi.mocked(commands.dismissFindings).mockResolvedValue({
      status: "error",
      error: { kind: "written", message: "could not be read back" },
    });

    await useAuditStore.getState().dismiss(scope, ["gh:f1"], "intended");

    // The marks come off the file as it is now, not as it was before.
    expect(useEditorStore.getState().saved.global).toEqual({
      schema: 1,
      install: {},
      "skill-instructions": { gh: "after the dismissal" },
    });
    expect(useProblemsStore.getState().dialog.title).toContain(
      "decision was recorded",
    );
  });

  it("still says nothing changed when nothing was written", async () => {
    vi.mocked(commands.dismissFindings).mockResolvedValue({
      status: "error",
      error: { kind: "untouched", message: "the token did not parse" },
    });

    await useAuditStore.getState().dismiss(scope, ["gh:f1"], "intended");

    // The reader is told the truth about their decision.
    expect(useProblemsStore.getState().dialog.steps?.[0]).toContain(
      "Nothing was changed",
    );
    // And the place is left free to save. The sync runs either way, since
    // nothing in the answer proves the file stood still — but it compares
    // what it read, so a file that did stand still keeps no mark.
    expect(useEditorStore.getState().outdated).toBeNull();
  });
});
