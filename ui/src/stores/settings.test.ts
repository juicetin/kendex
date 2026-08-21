import { toast } from "sonner";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "@/bindings";
import { commands } from "@/bindings";
import { useEditorStore } from "./editor";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { useSettingsStore } from "./settings";

vi.mock("@/bindings", () => ({
  commands: {
    getSettings: vi.fn(),
    capabilityTable: vi.fn(),
    updateSettings: vi.fn(),
    registerProject: vi.fn(),
    unregisterProject: vi.fn(),
    discoverProjects: vi.fn(),
    scanMachine: vi.fn(),
    windowSetZoom: vi.fn(),
    windowZoomState: vi.fn(),
    saveZoom: vi.fn(),
    installDriftHook: vi.fn(),
    getManifest: vi.fn(),
    editorInventory: vi.fn(),
  },
  ZOOM: { min: 50, max: 200, step: 10, default: 100 },
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const settings: AppSettings = {
  schema: 1,
  appearance: "system",
  safety: { "warn-below": 80, "block-below": 60 },
  "harness-roots": {},
  projects: [],
  zoom: 100,
};

describe("settings store", () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: null, capabilities: [] });
    useScanStore.setState({
      result: null,
      scanning: false,
      error: null,
      lastScanAt: null,
      backgroundFailureAnnounced: false,
    });
    useProblemsStore.setState({
      dialog: { open: false, title: "", steps: [], actions: [] },
    });
    vi.clearAllMocks();
    vi.mocked(commands.scanMachine).mockResolvedValue({
      status: "ok",
      data: { harnesses: [], items: [], missingProjects: [], warnings: [] },
    });
    vi.mocked(commands.windowZoomState).mockResolvedValue({
      percent: settings.zoom ?? 100,
      launchRefused: false,
    });
  });

  it("shows the error modal with the backend message and leaves settings untouched when a save fails", async () => {
    useSettingsStore.setState({ settings });
    vi.mocked(commands.updateSettings).mockResolvedValue({
      status: "error",
      error: "disk is full",
    });

    await useSettingsStore.getState().setAppearance("dark");

    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.open).toBe(true);
    expect(dialog.title).toBe("Couldn't change the appearance");
    expect(dialog.message).toBe("disk is full");
    expect(useSettingsStore.getState().settings).toBe(settings);
  });

  it("saves settings silently on success — no toast, no modal for an instant, visible change", async () => {
    const updated = { ...settings, appearance: "dark" as const };
    useSettingsStore.setState({ settings });
    vi.mocked(commands.updateSettings).mockResolvedValue({
      status: "ok",
      data: updated,
    });

    await useSettingsStore.getState().setAppearance("dark");

    expect(toast.success).not.toHaveBeenCalled();
    expect(useProblemsStore.getState().dialog.open).toBe(false);
    expect(useSettingsStore.getState().settings).toEqual(updated);
  });

  it("toasts success naming the folder when a project is added, and resolves true", async () => {
    vi.mocked(commands.registerProject).mockResolvedValue({
      status: "ok",
      data: { ...settings, projects: ["/home/x/acme-web"] },
    });

    const ok = await useSettingsStore
      .getState()
      .registerProject("/home/x/acme-web");

    expect(ok).toBe(true);
    // The success toast also offers the session drift report — an offer at
    // registration, never an auto-install.
    expect(toast.success).toHaveBeenCalledWith(
      "Added acme-web",
      expect.objectContaining({
        action: expect.objectContaining({ label: "Add session drift report" }),
      }),
    );
  });

  it("shows the error modal and resolves false when adding a project fails, without touching settings", async () => {
    useSettingsStore.setState({ settings });
    vi.mocked(commands.registerProject).mockResolvedValue({
      status: "error",
      error: "project already registered: /home/x/acme-web",
    });

    const ok = await useSettingsStore
      .getState()
      .registerProject("/home/x/acme-web");

    expect(ok).toBe(false);
    expect(toast.success).not.toHaveBeenCalled();
    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.open).toBe(true);
    expect(dialog.title).toBe("Couldn't add the project");
    expect(dialog.message).toBe("project already registered: /home/x/acme-web");
    expect(useSettingsStore.getState().settings).toBe(settings);
  });

  it("shows the error modal without a success toast when removing a project fails", async () => {
    useSettingsStore.setState({ settings });
    vi.mocked(commands.unregisterProject).mockResolvedValue({
      status: "error",
      error: "project not registered: /home/x/gone",
    });

    await useSettingsStore.getState().unregisterProject("/home/x/gone");

    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.open).toBe(true);
    expect(dialog.title).toBe("Couldn't stop tracking the project");
    expect(dialog.message).toBe("project not registered: /home/x/gone");
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("shows the error modal on a failed load instead of storing a buried error", async () => {
    vi.mocked(commands.getSettings).mockResolvedValue({
      status: "error",
      error: "cannot locate the home directory on this system",
    });
    vi.mocked(commands.capabilityTable).mockResolvedValue([]);

    await useSettingsStore.getState().load();

    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.open).toBe(true);
    expect(dialog.title).toBe("Couldn't load your settings");
    expect(dialog.message).toBe(
      "cannot locate the home directory on this system",
    );
    expect(useSettingsStore.getState().settings).toBeNull();
  });

  it("shows the error modal and returns an empty list when discovering projects fails", async () => {
    vi.mocked(commands.discoverProjects).mockResolvedValue({
      status: "error",
      error: "/nope is not a directory",
    });

    const found = await useSettingsStore.getState().discoverProjects("/nope");

    expect(found).toEqual([]);
    const dialog = useProblemsStore.getState().dialog;
    expect(dialog.open).toBe(true);
    expect(dialog.title).toBe("Couldn't search that folder");
    expect(dialog.message).toBe("/nope is not a directory");
  });
});

// Adding the session drift report declares a hook in that project's
// kendex.toml, so the Customize tab holding a copy of that file has to
// hear about it like it does for every other write.
describe("adding the drift report to a project", () => {
  const root = "/work/vg";

  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({ settings: null });
    useEditorStore.setState({
      scope: { scope: "project", root },
      draft: { schema: 1, install: {}, "skill-instructions": { gh: "mine" } },
      dirty: true,
      outdated: null,
      saved: {},
    });
    vi.mocked(commands.registerProject).mockResolvedValue({
      status: "ok",
      data: { ...settings, projects: [root] },
    });
    vi.mocked(commands.scanMachine).mockResolvedValue({
      status: "ok",
      data: { harnesses: [], items: [], missingProjects: [], warnings: [] },
    });
    vi.mocked(commands.installDriftHook).mockResolvedValue({
      status: "ok",
      data: true,
    });
    vi.mocked(commands.getManifest).mockResolvedValue({
      status: "ok",
      data: { manifest: null, base: null },
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
  });

  it("tells the editor the project's settings were written", async () => {
    await useSettingsStore.getState().registerProject(root);
    // The offer is a toast action, and taking it is what writes.
    const offer = vi.mocked(toast.success).mock.calls.at(-1)?.[1] as
      | { action?: { onClick: () => void } }
      | undefined;
    offer?.action?.onClick();
    await vi.waitUntil(() => useEditorStore.getState().outdated !== null);

    expect(commands.installDriftHook).toHaveBeenCalled();
    // Unsaved typing is kept and the place is marked, so the next save is
    // refused rather than writing the hook declaration back out.
    expect(useEditorStore.getState().outdated).toBe(root);
    expect(useEditorStore.getState().dirty).toBe(true);
  });
});
