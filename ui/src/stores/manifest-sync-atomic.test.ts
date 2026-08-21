// The four windows the sync used to leave open. Three rounds of review
// closed one caller's ordering at a time; these hold the helper itself to
// the rule, so a caller cannot open them again: the place is refused before
// anything is awaited, and no read replaces typing that arrives while it is
// on its way.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { manifestRewritten } from "./manifest-sync";
import { useMarketplacesStore } from "./marketplaces";
import { useProblemsStore } from "./problems";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: {
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
    updateManifest: vi.fn(),
    marketplaceSubscribe: vi.fn(),
    marketplaceUnsubscribe: vi.fn(),
    sourceToggle: vi.fn(),
    marketplacesOverview: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), message: vi.fn(), info: vi.fn() },
}));

vi.mock("./audit", () => ({
  useAuditStore: { getState: () => ({ refresh: vi.fn() }) },
}));

vi.mock("./scan", () => ({
  useScanStore: { getState: () => ({ refresh: vi.fn() }) },
}));

const scope = { scope: "global" as const };
const typed = { schema: 1, install: {}, "skill-instructions": { gh: "mine" } };

const inventory = {
  declaredAgents: [],
  declaredSkills: [],
  availableSkills: [],
  harnesses: [],
  hookEvents: [],
};

/** Typing arrives in the Customize tab. */
const type = () => useEditorStore.setState({ draft: typed, dirty: true });

/** The save the user presses in the window under test. */
const press = () => void useEditorStore.getState().save();

const refused = () => {
  expect(commands.updateManifest).not.toHaveBeenCalled();
  expect(useProblemsStore.getState().dialog.title).toContain(
    "changed while you typed",
  );
};

describe("the sync refuses before it reads", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({ settings: { schema: 1, projects: [] } });
    useEditorStore.setState({
      scope,
      draft: { schema: 1, install: {} },
      dirty: false,
      outdated: null,
      saved: {},
    });
    useProblemsStore.getState().closeError();
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(commands.editorInventory).mockResolvedValue({
      status: "ok",
      data: inventory,
    });
    vi.mocked(commands.updateManifest).mockResolvedValue({
      status: "error",
      error: "should never be reached",
    });
    vi.mocked(commands.marketplacesOverview).mockResolvedValue({
      status: "ok",
      data: [],
    });
  });

  it("refuses a save pressed while the manifests are still being read", async () => {
    type();
    // Not awaited: the call is the moment the window used to open, and the
    // press lands inside it.
    const syncing = manifestRewritten(scope);
    press();
    await syncing;

    refused();
    expect(useEditorStore.getState().draft).toEqual(typed);
  });

  it("keeps typing that arrives while the re-read is on its way", async () => {
    // Read 1 answers loadAll; read 2 is the re-read of this place, and the
    // typing lands while it is in flight.
    let reads = 0;
    vi.mocked(commands.getManifest).mockImplementation(async () => {
      reads += 1;
      if (reads === 2) type();
      return { status: "ok", data: null };
    });

    await manifestRewritten(scope);

    const after = useEditorStore.getState();
    expect(after.draft).toEqual(typed);
    expect(after.dirty).toBe(true);
    expect(after.outdated).toBe("global");
    // The place's manifest still reached the marks, which is what the read
    // was for; only the draft was left alone.
    expect(after.saved.global).toBeDefined();
    press();
    refused();
  });

  it("takes the file back when the copy in hand is untouched", async () => {
    await manifestRewritten(scope);

    const after = useEditorStore.getState();
    expect(after.dirty).toBe(false);
    expect(after.outdated).toBeNull();
  });

  it("leaves a place it is not about alone", async () => {
    type();
    await manifestRewritten({ scope: "project", root: "/w/app" });

    expect(useEditorStore.getState().outdated).toBeNull();
  });
});

// Each of these rewrites the scope's kendex.toml and then re-reads the
// marketplace tables. The save pressed during that re-read is the window.
describe("a subscription mutation tells the editor before it re-reads", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({ settings: { schema: 1, projects: [] } });
    useEditorStore.setState({ scope, draft: typed, dirty: true, saved: {} });
    useEditorStore.setState({ outdated: null });
    useProblemsStore.getState().closeError();
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(commands.editorInventory).mockResolvedValue({
      status: "ok",
      data: inventory,
    });
    vi.mocked(commands.updateManifest).mockResolvedValue({
      status: "error",
      error: "should never be reached",
    });
    // The tables re-read after the manifest was rewritten: the save lands
    // in that window.
    vi.mocked(commands.marketplacesOverview).mockImplementation(async () => {
      press();
      return { status: "ok", data: [] };
    });
  });

  it("refuses the save after subscribing", async () => {
    vi.mocked(commands.marketplaceSubscribe).mockResolvedValue({
      status: "ok",
      data: {
        name: "kit",
        reference: "acme/kit",
        rev: null,
        notes: [],
        lead: null,
      },
    });
    await useMarketplacesStore.getState().subscribe(scope, "acme/kit", null);

    refused();
  });

  it("refuses the save after unsubscribing", async () => {
    vi.mocked(commands.marketplaceUnsubscribe).mockResolvedValue({
      status: "ok",
      data: null,
    });
    await useMarketplacesStore
      .getState()
      .unsubscribe(scope, "kit", false, false);

    refused();
  });

  it("refuses the save after turning a subscription off", async () => {
    vi.mocked(commands.sourceToggle).mockResolvedValue({
      status: "ok",
      data: [],
    });
    await useMarketplacesStore.getState().toggle(scope, "kit", false);

    refused();
  });
});
