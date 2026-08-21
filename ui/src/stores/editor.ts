import { create } from "zustand";
import { commands, type EditorInventory, type Scope } from "@/bindings";
import { type Draft, emptyDraft, toDraft } from "@/lib/editor-draft";
import { sameScope, scopeKey } from "@/lib/scope";
import { useAuditStore } from "./audit";
import { useScanStore } from "./scan";
import { useSettingsStore } from "./settings";

interface EditorState {
  /** The single scope being edited — deliberately not the sidebar filter. */
  scope: Scope;
  draft: Draft | null;
  inventory: EditorInventory | null;
  /** Every scope's saved manifest, keyed by scope. What the Library and the
   *  Customize index read to mark what has been customized; `draft` above is
   *  the one copy being edited. */
  saved: Record<string, Draft>;
  /** Whether {@link loadAll} has finished a pass. Until it has, a scope
   *  missing from `saved` was never asked for; after it has, that scope's
   *  manifest could not be read. */
  manifestsLoaded: boolean;
  /** Why a place's manifest could not be read on the last {@link loadAll},
   *  naming the places. Null when every one of them was read. */
  manifestError: string | null;
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  error: string | null;
  setScope: (scope: Scope) => Promise<void>;
  /** Point the editor at a scope without discarding edits already in hand. */
  openScope: (scope: Scope) => Promise<void>;
  load: () => Promise<void>;
  /** Read every scope's manifest, for the marks drawn outside the editor. */
  loadAll: () => Promise<void>;
  edit: (change: (draft: Draft) => Draft) => void;
  save: () => Promise<void>;
}

export const useEditorStore = create<EditorState>((set, get) => {
  // Which read the state on screen belongs to. Two place switches can be in
  // flight at once — the chips are disabled while dirty, never while
  // loading — and the slower one landing last would leave `draft` holding
  // one place's manifest while `scope` names another. Every per-place mark
  // reads that pair, and a save would write the wrong file.
  let latest = 0;

  const load = async () => {
    const { scope } = get();
    latest += 1;
    const token = latest;
    const current = () => token === latest;
    set({ loading: true });
    let manifest: Awaited<ReturnType<typeof commands.getManifest>>;
    let inventory: Awaited<ReturnType<typeof commands.editorInventory>>;
    try {
      [manifest, inventory] = await Promise.all([
        commands.getManifest(scope),
        commands.editorInventory(scope),
      ]);
    } finally {
      if (current()) set({ loading: false });
    }
    if (manifest.status === "error") {
      if (current()) set({ draft: null, dirty: false, error: manifest.error });
      return;
    }
    // With no manifest here yet the editor still opens, on an empty one:
    // asking someone to press "create" before they can type is a step that
    // decides nothing. Saving is what writes the file.
    const draft = manifest.data ? toDraft(manifest.data) : emptyDraft();
    // A superseded read still knows its own place's manifest, so it keeps
    // feeding the marks — it just never becomes the draft on screen.
    if (!current()) {
      set((state) => ({ saved: { ...state.saved, [scopeKey(scope)]: draft } }));
      return;
    }
    set((state) => ({
      draft,
      inventory: inventory.status === "ok" ? inventory.data : state.inventory,
      saved: { ...state.saved, [scopeKey(scope)]: draft },
      dirty: false,
      error: inventory.status === "ok" ? null : inventory.error,
    }));
  };

  const write = async () => {
    // Scope and draft are one value: read apart, a place switch between the
    // two reads sends one place's manifest to another place's file.
    const { scope, draft } = get();
    if (!draft) return;
    set({ saving: true });
    let response: Awaited<ReturnType<typeof commands.updateManifest>>;
    try {
      response = await commands.updateManifest(scope, draft);
    } finally {
      set({ saving: false });
    }
    if (response.status === "error") {
      set({ error: response.error });
      return;
    }
    set({ error: null });
    await load();
    await useAuditStore.getState().refresh();
    await useScanStore.getState().refresh();
  };

  return {
    scope: { scope: "global" },
    draft: null,
    inventory: null,
    saved: {},
    manifestsLoaded: false,
    manifestError: null,
    dirty: false,
    loading: false,
    saving: false,
    error: null,

    setScope: async (scope) => {
      set({ scope, draft: null, dirty: false, error: null });
      await load();
    },

    openScope: async (scope) => {
      const state = get();
      if (state.draft && sameScope(state.scope, scope)) return;
      await state.setScope(scope);
    },

    load,

    loadAll: async () => {
      // Startup reads run side by side, so the project list may still be on
      // its way — without it this would mark only the global scope.
      const settings = useSettingsStore.getState();
      if (!settings.settings) await settings.load();
      const projects = useSettingsStore.getState().settings?.projects ?? [];
      const scopes: Scope[] = [
        { scope: "global" },
        ...projects.map((root) => ({ scope: "project" as const, root })),
      ];
      const loaded = await Promise.all(
        scopes.map((scope) => commands.getManifest(scope)),
      );
      const saved: Record<string, Draft> = {};
      // A place whose manifest would not load stays out of `saved`, which
      // the per-place marks read as "could not say". Keeping the reason
      // means the Library can name it rather than only implying it.
      const failed: string[] = [];
      for (const [index, response] of loaded.entries()) {
        if (response.status !== "ok") {
          failed.push(`${scopeKey(scopes[index])}: ${response.error}`);
          continue;
        }
        saved[scopeKey(scopes[index])] = response.data
          ? toDraft(response.data)
          : emptyDraft();
      }
      set({
        saved,
        manifestsLoaded: true,
        manifestError: failed.length > 0 ? failed.join("\n") : null,
      });
    },

    edit: (change) => {
      const { draft } = get();
      if (!draft) return;
      set({ draft: change(draft), dirty: true });
    },

    save: write,
  };
});
