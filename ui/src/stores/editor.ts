import { create } from "zustand";
import { commands, type EditorInventory, type Scope } from "@/bindings";
import { type Draft, emptyDraft, toDraft } from "@/lib/editor-draft";
import { keepIfSame } from "@/lib/same-read";
import { sameScope, scopeKey } from "@/lib/scope";
import { useAuditStore } from "./audit";
import { named, readManifests } from "./editor-scopes";
import { useScanStore } from "./scan";

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
  /** True while a {@link loadAll} pass is running, so a retry cannot be
   *  pressed on top of the read it is waiting for. */
  manifestsReading: boolean;
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  error: string | null;
  setScope: (scope: Scope) => Promise<void>;
  /** Point the editor at a scope without discarding edits already in hand. */
  openScope: (scope: Scope) => Promise<void>;
  /** Read one place's manifest — the open one, or the one named. */
  load: (scope?: Scope) => Promise<void>;
  /** Read every scope's manifest, for the marks drawn outside the editor. */
  loadAll: () => Promise<void>;
  edit: (change: (draft: Draft) => Draft) => void;
  save: () => Promise<void>;
}

export const useEditorStore = create<EditorState>((set, get) => {
  // Every manifest read takes a ticket. Three readers overlap — one place
  // at a time, every place at once, and the re-read a save ends with — and
  // without tickets the pass that happens to land last wins, reverting a
  // place someone just read or just saved.
  let reads = 0;
  // The ticket behind each place's saved manifest, so an older read landing
  // late is dropped for that place alone rather than for the whole pass.
  const behind = new Map<string, number>();
  let writes = 0;

  /** Fold freshly read manifests into the saved ones, skipping any place a
   *  later read already answered for, and keeping the object already on
   *  screen when nothing moved. */
  const fold = (
    previous: Record<string, Draft>,
    read: [string, Draft][],
    token: number,
  ): Record<string, Draft> => {
    const next = { ...previous };
    for (const [key, draft] of read) {
      if ((behind.get(key) ?? 0) > token) continue;
      behind.set(key, token);
      next[key] = draft;
    }
    return keepIfSame(previous, next);
  };

  const load = async (target?: Scope) => {
    const scope = target ?? get().scope;
    reads += 1;
    const token = reads;
    // This read speaks for the editor on screen only while it is the newest
    // and the editor still points at the place it read: a save ends by
    // re-reading the place it wrote, which may no longer be that one.
    const onScreen = () => token === reads && sameScope(get().scope, scope);
    if (onScreen()) set({ loading: true });
    let manifest: Awaited<ReturnType<typeof commands.getManifest>>;
    let inventory: Awaited<ReturnType<typeof commands.editorInventory>>;
    try {
      [manifest, inventory] = await Promise.all([
        commands.getManifest(scope),
        commands.editorInventory(scope),
      ]);
    } catch (thrown) {
      // A transport failure rejects rather than answering, and a read that
      // ends with nothing said is the silent failure this store exists to
      // avoid — the editor says it could not open rather than sitting empty.
      if (onScreen())
        set({
          loading: false,
          draft: null,
          dirty: false,
          error: String(thrown),
        });
      return;
    }
    if (onScreen()) set({ loading: false });
    if (manifest.status === "error") {
      if (onScreen()) set({ draft: null, dirty: false, error: manifest.error });
      return;
    }
    // With no manifest here yet the editor still opens, on an empty one:
    // asking someone to press "create" before they can type is a step that
    // decides nothing. Saving is what writes the file.
    const draft = manifest.data ? toDraft(manifest.data) : emptyDraft();
    const read: [string, Draft][] = [[scopeKey(scope), draft]];
    // A read that no longer speaks for the screen still knows its own
    // place's manifest, so it keeps feeding the marks.
    if (!onScreen()) {
      set((state) => ({ saved: fold(state.saved, read, token) }));
      return;
    }
    set((state) => ({
      draft,
      inventory: inventory.status === "ok" ? inventory.data : state.inventory,
      saved: fold(state.saved, read, token),
      dirty: false,
      error: inventory.status === "ok" ? null : inventory.error,
    }));
  };

  const write = async () => {
    // Scope and draft are one value: read apart, a place switch between the
    // two reads sends one place's manifest to another place's file.
    const { scope, draft } = get();
    if (!draft) return;
    writes += 1;
    const token = writes;
    const mine = () => token === writes;
    const onScreen = () => mine() && sameScope(get().scope, scope);
    set({ saving: true });
    let response: Awaited<ReturnType<typeof commands.updateManifest>>;
    try {
      response = await commands.updateManifest(scope, draft);
    } catch (thrown) {
      if (mine()) set({ saving: false, error: `${named(scope)}: ${thrown}` });
      return;
    }
    if (mine()) set({ saving: false });
    // A newer save owns what the screen says about saving.
    if (!mine()) return;
    if (response.status === "error") {
      // The note is about the place that was written, which may not be the
      // one on screen any more — so it names that place rather than letting
      // the reader assume the one in front of them.
      set({
        error: onScreen()
          ? response.error
          : `${named(scope)}: ${response.error}`,
      });
      return;
    }
    if (onScreen()) set({ error: null });
    // Re-read the place that was written, never whichever is open now, or
    // its saved manifest keeps the pre-save content and its mark with it.
    await load(scope);
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
    manifestsReading: false,
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
      reads += 1;
      const token = reads;
      set({ manifestsReading: true });
      try {
        const { read, failed } = await readManifests();
        set((state) => ({
          // A place whose manifest would not load keeps the last one that
          // did, rather than being dropped from `saved` and taking a mark
          // that was right with it.
          saved: fold(state.saved, read, token),
          manifestsLoaded: true,
          manifestError: failed.length > 0 ? failed.join("\n") : null,
          manifestsReading: false,
        }));
      } catch (thrown) {
        // A rejected read is a read that failed, not one still running: a
        // pass that says nothing leaves every place reading as in-flight
        // forever, with no note and no retry.
        set({
          manifestsLoaded: false,
          manifestError: String(thrown),
          manifestsReading: false,
        });
      }
    },

    edit: (change) => {
      const { draft } = get();
      if (!draft) return;
      set({ draft: change(draft), dirty: true });
    },

    save: write,
  };
});
