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
import { writing } from "./manifest-sync";
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
      const attempt = await writing(scope, () =>
        commands.dismissFindings(scope, tokens, reason),
      );
      // Three outcomes, not two. The command answers with whether it got
      // as far as writing; a rejection answers nothing at all, and saying
      // "nothing was changed" of that is the one wrong guess that costs
      // something — it invites a second attempt at a decision that may
      // already be recorded.
      const said = !attempt.ok
        ? {
            message: attempt.why,
            title: "Couldn't tell whether the finding was settled",
            step: "The refreshed list below is what is actually recorded — read it before deciding again",
          }
        : attempt.value.status === "ok"
          ? null
          : attempt.value.error.kind === "written"
            ? {
                message: attempt.value.error.message,
                title: "Your decision was recorded, but the undo is not there",
                step: "The finding is settled — what could not be read back is the way to take it back",
              }
            : {
                message: attempt.value.error.message,
                title: "Couldn't dismiss this finding",
                step: "Nothing was changed — read the finding again and decide again",
              };
      if (said) {
        set({ error: said.message });
        useProblemsStore.getState().showError({
          title: said.title,
          message: said.message,
          steps: [said.step],
        });
        // The refusal usually means the page was showing findings a minute
        // old; the fresh audit is what the person should decide on — and
        // where nothing could be told, it is the only thing that can say.
        await get().refresh({ force: true });
        return;
      }
      if (!attempt.ok || attempt.value.status !== "ok") return;
      const { view, records } = attempt.value.data;
      set({ views: replaceView(get().views, view), error: null });
      // How much of the undo is already done. It lives out here because a
      // retry runs the same closure again: each record's revoke is pinned
      // to the exact dismissal it takes back, so one already taken back
      // refuses and stops the loop before it reaches the record that
      // actually failed — a retry that can never finish what it offers.
      let taken = 0;
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
                // leaves the ones before it taken back on disk — which is
                // why the funnel tells the editor either way.
                for (const record of records.slice(taken)) {
                  latest = await commands.revokeDismissal(
                    scope,
                    record.key,
                    record.fingerprint,
                    record.dismissedAt,
                  );
                  if (latest.status !== "ok") break;
                  taken += 1;
                }
                return latest;
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
