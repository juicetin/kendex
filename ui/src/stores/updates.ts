import { create } from "zustand";
import { commands, type ItemWarning, type UpdateRow } from "@/bindings";
import { UPDATE_ERROR_TITLE } from "@/lib/copy";
import { keepIfSame } from "@/lib/same-read";
import { manifestRewritten } from "./manifest-sync";
import { useProblemsStore } from "./problems";
import { applyMany, applyOne } from "./updates-apply";

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

/** Whether the rows on screen may be acted on. A read that failed keeps the
 *  last good rows rather than blanking the page, which is right for reading
 *  — but a button that applies a revision off a row nobody could confirm is
 *  the mark that called a place untouched when nobody had looked. `loaded`
 *  is the read having succeeded; `busy` is one already running. */
export const canApplyUpdates = (state: {
  loaded: boolean;
  busy: boolean;
}): boolean => state.loaded && !state.busy;

export const useUpdatesStore = create<UpdatesState>((set) => {
  const showError = (title: string, message: string) =>
    useProblemsStore.getState().showError({ title, message });

  // Every read of the standing takes a ticket. A read issued before a fork
  // or a discard lands, resolving after it, would otherwise put its
  // pre-resolution rows back — and the marks, the notice and the Review
  // count all read those rows, so a state someone just resolved reappears.
  //
  // Two kinds of read, and one counter cannot rank them. A poll reads what
  // the mirrors already say; Check for updates fetches first, so its answer
  // is the newer one however the two land. Sharing a ticket, a poll issued
  // during a check takes the newer number and commits the pre-fetch rows,
  // and the check the person asked for is thrown away with the screen still
  // showing what it was asked to replace.
  let reads = 0;
  let checks = 0;
  let checking = 0;
  const ticket = (fetched = false) => {
    reads += 1;
    const mine = reads;
    if (fetched) {
      checks += 1;
      checking += 1;
    }
    const mineCheck = checks;
    // Whether a fetch was already running when this read began. A poll that
    // started mid-fetch is reading the pre-fetch mirrors however long it
    // takes to return, so asking what is in flight when it *lands* accepts
    // exactly that poll once the fetch has finished — and puts the rows the
    // person asked to replace back over the fetched ones.
    const during = checking > 0;
    return fetched
      ? // Only a later fetch answers for one: a poll that started after it
        // is reading the older truth, whenever it happens to land.
        () => mineCheck === checks
      : // And a poll lands only while it is the newest read, with no fetch
        // running when it started and none started since.
        () => mine === reads && !during && mineCheck === checks;
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
      const newest = ticket(true);
      try {
        let response: Awaited<ReturnType<typeof commands.updatesRefresh>>;
        try {
          response = await commands.updatesRefresh();
        } catch (thrown) {
          // A rejected read is a read that failed. Left to reject, the
          // standing keeps its last successful values and the marks go on
          // presenting stale rows as a check that worked.
          if (newest()) {
            set({ loaded: false, error: String(thrown) });
            showError(UPDATE_ERROR_TITLE, String(thrown));
          }
          return;
        }
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
        checking -= 1;
        // The spinner belongs to every fetch, not to whichever finishes
        // first: two overlapping checks and the first to land would take it
        // down with the other still running.
        set({ checking: checking > 0 });
      }
    },

    updateOne: applyOne,

    updateRows: applyMany,

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
        } else {
          // Holding a package at a version, or letting it follow again,
          // writes that place's kendex.toml — before the tables re-read,
          // or a save of the copy the Customize tab holds puts the old
          // file back over what this just recorded.
          await manifestRewritten(row.scope);
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
