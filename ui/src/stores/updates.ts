import { toast } from "sonner";
import { create } from "zustand";
import { commands, type ItemWarning, type UpdateRow } from "@/bindings";
import { UPDATE_ERROR_TITLE, updatedToastLabel } from "@/lib/copy";
import {
  nothingToUpdateToastLabel,
  updatedWithPlaceToastLabel,
} from "@/lib/copy-updates";
import { keepIfSame } from "@/lib/same-read";
import { scopeKey } from "@/lib/scope";
import { placeName, skippedPlaces, updatablePlaces } from "@/lib/update-groups";
import { visibleUpdates } from "@/lib/update-rows";
import { bulkUpdateToast } from "@/lib/update-toasts";
import { useAuditStore } from "./audit";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";

interface UpdatesState {
  rows: UpdateRow[];
  /** Packages whose standing could not be computed — shown, never treated
   *  as current. */
  warnings: ItemWarning[];
  busy: boolean;
  /** True while a mirror fetch is running — the explicit "check". */
  checking: boolean;
  loaded: boolean;
  /** Why the last read of the standing failed, or null. A load runs on its
   *  own at startup, so it cannot open the error modal a click may — the
   *  screens that read the standing say what happened instead. */
  error: string | null;
  load: () => Promise<void>;
  check: () => Promise<void>;
  updateOne: (row: UpdateRow) => Promise<void>;
  /** Bring every updatable place among `rows` current — the page-level
   *  button passes every visible row, a package's button its own places. */
  updateRows: (rows: UpdateRow[]) => Promise<void>;
  setAutoUpdate: (row: UpdateRow, auto: boolean) => Promise<void>;
  setIgnored: (row: UpdateRow, ignored: boolean) => Promise<void>;
}

export const useUpdatesStore = create<UpdatesState>((set, get) => {
  const showError = (title: string, message: string) =>
    useProblemsStore.getState().showError({ title, message });

  // Every read of the standing takes a ticket. A read issued before a fork
  // or a discard lands, resolving after it, would otherwise put its
  // pre-resolution rows back — and the marks, the notice and the Review
  // count all read those rows, so a state someone just resolved reappears.
  let reads = 0;
  const ticket = () => {
    reads += 1;
    const mine = reads;
    return () => mine === reads;
  };

  const apply = async (row: UpdateRow): Promise<boolean> => {
    // Held packages move by moving the hold; following ones come current
    // by applying the scope — which is what following means, and brings
    // any other pending changes in that scope along.
    const response =
      row.pinned && row.latest
        ? await commands.packageSetRev(
            row.scope,
            row.kind,
            row.name,
            row.latest.commit,
          )
        : await commands.applyPlan(row.scope, false, []);
    if (response.status === "error") {
      showError(UPDATE_ERROR_TITLE, response.error);
      return false;
    }
    return true;
  };

  const reload = async () => {
    const newest = ticket();
    let response: Awaited<ReturnType<typeof commands.updatesOverview>>;
    try {
      response = await commands.updatesOverview();
    } catch (thrown) {
      // A rejected read is a read that failed. Left to reject it would end
      // the pass with nothing said, and every place would read as still
      // being checked with nothing running and no note to say otherwise.
      if (newest()) set({ loaded: false, error: String(thrown) });
      return;
    }
    if (!newest()) return;
    // A failed reload marks the data stale (loaded = false) rather than
    // leaving the last-good rows trusted — the package page gates the
    // Update button on `loaded`, and acting on rows we could not refresh
    // is exactly the fail-open this closes.
    if (response.status === "ok")
      set((state) => ({
        // A re-read that changed nothing hands back what is already on
        // screen: every screen joining on these rows memoizes on identity.
        rows: keepIfSame(state.rows, response.data.rows),
        warnings: keepIfSame(state.warnings, response.data.warnings),
        loaded: true,
        error: null,
      }));
    else set({ loaded: false, error: response.error });
  };

  return {
    rows: [],
    warnings: [],
    busy: false,
    checking: false,
    loaded: false,
    error: null,

    load: async () => {
      await reload();
    },

    check: async () => {
      set({ checking: true });
      const newest = ticket();
      try {
        const response = await commands.updatesRefresh();
        if (!newest()) return;
        if (response.status === "ok") {
          set({
            rows: response.data.rows,
            warnings: response.data.warnings,
            loaded: true,
            error: null,
          });
        } else {
          set({ loaded: false, error: response.error });
          showError(UPDATE_ERROR_TITLE, response.error);
        }
      } finally {
        set({ checking: false });
      }
    },

    updateOne: async (row) => {
      set({ busy: true });
      try {
        if (await apply(row)) {
          // A follower comes current by applying its scope, which brings
          // that scope's other followers along — the toast says so rather
          // than letting the extra changes look like a surprise.
          toast.success(
            row.pinned
              ? updatedToastLabel(row.name)
              : updatedWithPlaceToastLabel(row.name, placeName(row.scope)),
          );
          await reload();
          await useScanStore.getState().refresh();
          await useAuditStore.getState().refresh({ force: true });
        }
      } finally {
        set({ busy: false });
      }
    },

    updateRows: async (wanted) => {
      set({ busy: true });
      try {
        // Edited packages are held by the engine and cannot be updated
        // this way — they need the fork decision first, so they are left
        // out rather than silently surviving the click. Rows that are news
        // without an update (gone upstream, mixed installs) have nothing
        // for this button to do.
        const rows = updatablePlaces(wanted);
        const skipped = skippedPlaces(wanted).length;
        if (rows.length === 0) {
          toast.info(nothingToUpdateToastLabel(skipped));
          return;
        }
        // Move every hold first — each move applies its whole scope, so
        // that scope's followers are already current — then one apply per
        // scope no hold touched. Never two applies for one scope.
        let ok = true;
        const applied = new Set<string>();
        for (const row of rows.filter((row) => row.pinned)) {
          if (await apply(row)) applied.add(scopeKey(row.scope));
          else ok = false;
        }
        const scopes = new Map(
          rows
            .filter((row) => !row.pinned && !applied.has(scopeKey(row.scope)))
            .map((row) => [scopeKey(row.scope), row] as const),
        );
        for (const row of scopes.values()) {
          const response = await commands.applyPlan(row.scope, false, []);
          if (response.status === "error") {
            showError(UPDATE_ERROR_TITLE, response.error);
            ok = false;
          }
        }
        await reload();
        if (ok)
          toast.success(
            bulkUpdateToast(rows, skipped, visibleUpdates(get().rows)),
          );
        await useScanStore.getState().refresh();
        await useAuditStore.getState().refresh({ force: true });
      } finally {
        set({ busy: false });
      }
    },

    setAutoUpdate: async (row, auto) => {
      // Switching following OFF holds the package at what is installed now.
      // With nothing installed to hold at, there is nothing to switch —
      // never fall through to null, which means "follow" (the opposite).
      const hold = row.current?.commit ?? null;
      if (!auto && hold === null) return;
      set({ busy: true });
      try {
        const response = await commands.packageSetRev(
          row.scope,
          row.kind,
          row.name,
          auto ? null : hold,
        );
        if (response.status === "error") {
          showError(UPDATE_ERROR_TITLE, response.error);
        }
        await reload();
      } finally {
        set({ busy: false });
      }
    },

    setIgnored: async (row, ignored) => {
      const response = await commands.updateSetIgnored(
        row.scope,
        row.kind,
        row.name,
        row.repo,
        ignored,
      );
      if (response.status === "ok")
        set({
          rows: response.data.rows,
          warnings: response.data.warnings,
          loaded: true,
          error: null,
        });
      else showError(UPDATE_ERROR_TITLE, response.error);
    },
  };
});
