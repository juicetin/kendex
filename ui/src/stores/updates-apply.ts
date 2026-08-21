import { toast } from "sonner";
import { commands, type UpdateRow } from "@/bindings";
import { UPDATE_ERROR_TITLE, updatedToastLabel } from "@/lib/copy";
import {
  nothingToUpdateToastLabel,
  updatedWithPlaceToastLabel,
} from "@/lib/copy-updates";
import { scopeKey } from "@/lib/scope";
import { placeName, skippedPlaces, updatablePlaces } from "@/lib/update-groups";
import { visibleUpdates } from "@/lib/update-rows";
import { bulkUpdateToast } from "@/lib/update-toasts";
import { useAuditStore } from "./audit";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { useUpdatesStore } from "./updates";

// Bringing places current, under the updates store's busy flag so every
// control on the page waits on the same one.

const showError = (title: string, message: string) =>
  useProblemsStore.getState().showError({ title, message });

const set = (partial: { busy: boolean }) => useUpdatesStore.setState(partial);
const reload = () => useUpdatesStore.getState().load();

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

/** Bring one place current. */
export const applyOne = async (row: UpdateRow): Promise<void> => {
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
};

/** Bring every updatable place among `rows` current. */
export const applyMany = async (wanted: UpdateRow[]): Promise<void> => {
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
        bulkUpdateToast(
          rows,
          skipped,
          visibleUpdates(useUpdatesStore.getState().rows),
        ),
      );
    await useScanStore.getState().refresh();
    await useAuditStore.getState().refresh({ force: true });
  } finally {
    set({ busy: false });
  }
};
