import { toast } from "sonner";
import { commands } from "@/bindings";
import { writing } from "./manifest-sync";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { refusesForUnsaved } from "./unsaved-first";

// How many of these writes are in flight. The busy flag belongs to all of
// them rather than to whichever finishes first: the offer is made per
// project and two can stand open at once, so the first to land would take
// the Customize Save bar's gate off while the second is still writing.
let running = 0;

/** Offer to install the session drift report in a project just added.
 *
 *  An offer rather than an auto-install: it declares a hook that injects
 *  into agent context, which is not something to do to someone's project
 *  without asking. Taking it writes that project's kendex.toml, so it owes
 *  what every other writer of that file owes — refusing while unsaved
 *  customization for the place is waiting, and holding the Save bar down
 *  until the editor has been told. */
export function offerDriftHook(
  root: string,
  added: string,
  set: (partial: { busy: boolean }) => void,
): void {
  toast.success(`Added ${added}`, {
    action: {
      label: "Add session drift report",
      onClick: () => {
        if (refusesForUnsaved({ scope: "project", root })) return;
        // Down only once the editor has been told, below: clearing it any
        // earlier leaves a window where a save passes the outdated check
        // and writes the pre-hook file back over the declaration.
        running += 1;
        set({ busy: true });
        const scope = { scope: "project" as const, root };
        void writing(scope, () => commands.installDriftHook(scope))
          .then((attempt) => {
            // The declaration can be committed before the command's own
            // error, and a rejection says less still — so the editor is
            // told either way, which is what `writing` is for.
            if (!attempt.ok) {
              useProblemsStore.getState().showError({
                title: "Couldn't install the drift report",
                message: attempt.why,
              });
              return;
            }
            if (attempt.value.status !== "ok") {
              useProblemsStore.getState().showError({
                title: "Couldn't install the drift report",
                message: attempt.value.error,
              });
              return;
            }
            // False: the scope had other pending changes, so only the
            // declaration landed — nothing is applied unreviewed.
            toast.success(
              attempt.value.data
                ? "Drift report installed"
                : "Drift report added — finish by applying changes in Review",
            );
            void useScanStore.getState().refresh();
          })
          .finally(() => {
            running -= 1;
            if (running === 0) set({ busy: false });
          });
      },
    },
  });
}
