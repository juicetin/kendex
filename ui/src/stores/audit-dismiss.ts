import { toast } from "sonner";
import {
  type AuditView,
  commands,
  type DismissReason,
  type Scope,
} from "@/bindings";
import {
  ignoredToast,
  TAKEN_BACK_TOAST,
  UNDO_LABEL,
} from "@/lib/copy-decisions";
import { replaceView } from "./audit";
import type { auditMutation } from "./audit-mutate";
import { manifestRewritten } from "./manifest-sync";
import { useProblemsStore } from "./problems";
import { refusesForUnsaved } from "./unsaved-first";

/** Settling a safety finding, and taking that back.
 *
 *  Apart from the rest because both halves write before they can say what
 *  they wrote: the dismissal is applied and then read back to build its
 *  undo, and the undo is one write per record. Either can fail with the
 *  file already changed, and the editor is owed that by the write rather
 *  than by the outcome. */
export function dismissFinding(
  set: (update: {
    views?: AuditView[];
    error?: string | null;
    busy?: boolean;
  }) => void,
  get: () => {
    views: AuditView[];
    refresh: (opts?: { force?: boolean }) => Promise<void>;
  },
  run: ReturnType<typeof auditMutation>,
) {
  return async (
    scope: Scope,
    tokens: string[],
    reason: DismissReason,
  ): Promise<void> => {
    // Settling a finding writes this place's kendex.toml like every other
    // action here, and went round the funnel that asks for them.
    if (refusesForUnsaved(scope)) return;
    set({ busy: true });
    try {
      const response = await commands.dismissFindings(scope, tokens, reason);
      if (response.status !== "ok") {
        // The write comes before the read that describes it, so a failure
        // here does not mean the file stood still. Told wrong, this says
        // nothing changed and leaves the editor holding a copy of a file
        // that moved — a copy whose save would put the decisions back.
        const landed = response.error.kind === "written";
        set({ error: response.error.message });
        useProblemsStore.getState().showError({
          title: landed
            ? "Your decision was recorded, but the undo is not there"
            : "Couldn't dismiss this finding",
          message: response.error.message,
          steps: [
            landed
              ? "The finding is settled — what could not be read back is the way to take it back"
              : "Nothing was changed — read the finding again and decide again",
          ],
        });
        // The editor is owed this by the write, not by the outcome: a
        // file that changed is one its copy predates either way.
        if (landed) await manifestRewritten(scope);
        // The refusal usually means the page was showing findings a minute
        // old; the fresh audit is what the person should decide on.
        await get().refresh({ force: true });
        return;
      }
      const { view, records } = response.data;
      set({ views: replaceView(get().views, view), error: null });
      await manifestRewritten(scope);
      toast.success(ignoredToast(records.length), {
        action: {
          label: UNDO_LABEL,
          onClick: () =>
            void run(
              scope,
              async () => {
                let latest: Awaited<
                  ReturnType<typeof commands.revokeDismissal>
                > = {
                  status: "error",
                  error: "nothing to take back",
                };
                // One undo per record, so a failure partway through
                // leaves the ones before it taken back on disk. The
                // editor is told about those whichever way this ends.
                let took = 0;
                for (const record of records) {
                  latest = await commands.revokeDismissal(
                    scope,
                    record.key,
                    record.fingerprint,
                    record.dismissedAt,
                  );
                  if (latest.status !== "ok") break;
                  took += 1;
                }
                return latest.status === "ok"
                  ? latest
                  : { ...latest, wrote: took > 0 };
              },
              {
                title: "Couldn't take the dismissal back",
                successMessage: TAKEN_BACK_TOAST,
              },
            ),
        },
      });
    } finally {
      set({ busy: false });
    }
  };
}
