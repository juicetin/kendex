import { toast } from "sonner";
import type { AuditView, Scope } from "@/bindings";
import { replaceView } from "./audit";
import { writeTicket } from "./audit-order";
import { writing } from "./manifest-sync";
import { type ErrorAction, useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { refusesForUnsaved, takeTheRest } from "./unsaved-first";

/** How many writes are in flight. Two of them can be: a toast's Undo is
 *  pressed whenever the reader presses it, which can be while a package-wide
 *  action is still writing. */
let writers = 0;

/** Hold the store's busy flag down for the length of one write.
 *
 *  The flag is what keeps the Customize tab's Save bar down, so it has to
 *  stay down until the editor has been told its copy is stale. A plain
 *  boolean went up at whichever write finished first, releasing the bar over
 *  another still in progress, where a save passes the outdated check and
 *  writes the pre-action manifest back. Every writer of this flag holds it
 *  through here — one left out is the same bug again. */
export async function heldDown<T>(
  set: (update: { busy: boolean }) => void,
  call: () => Promise<T>,
): Promise<T> {
  writers += 1;
  set({ busy: true });
  try {
    return await call();
  } finally {
    writers -= 1;
    if (writers === 0) set({ busy: false });
  }
}

/** Everything the funnel writes back into the store it belongs to. */
interface MutationHost {
  views: AuditView[];
  error: string | null;
  busy: boolean;
}

/** The one way an audit action reaches the machine: apply, adopt, toggle,
 *  remove and dismiss all go through here, so what each of them owes —
 *  refusing while a draft is unsaved, holding the Save bar down until the
 *  editor knows, saying what happened either way — is owed once. */
export function auditMutation(
  set: (
    update:
      | Partial<MutationHost>
      | ((state: { views: AuditView[] }) => Partial<MutationHost>),
  ) => void,
  get: () => { views: AuditView[] },
) {
  // A row that vanishes with no word said is indistinguishable from a
  // button that did nothing — every outcome here speaks up, success or
  // failure, on top of the state update the page renders from. Failure is a
  // modal, not a toast: these are all user-initiated, so the user is looking
  // right at the button that just broke.
  const run = async (
    // Every action through here rewrites this scope's kendex.toml, and the
    // editor holds a whole copy of it that a save would write back.
    scope: Scope,
    action: () => Promise<
      { status: "ok"; data: AuditView } | { status: "error"; error: string }
    >,
    opts: { title: string; successMessage?: string; steps?: string[] },
    // Whether the machine took it. A caller running one action over
    // several places cannot read that off a void: it would carry on to the
    // next place after the first refused or failed, and leave the package
    // changed in some of them.
  ) => {
    // Apply, adopt, toggle and remove all rewrite this scope's kendex.toml,
    // so unsaved customization for it refuses them the way a fork or a
    // discard is refused — before anything is written, and wherever the
    // typing is waiting.
    if (refusesForUnsaved(scope)) return false;
    // Taken at entry, so it belongs to this write alone: anything started
    // after it — an Undo from a toast, another action entirely — finds
    // nothing left and retries itself.
    const theRest = takeTheRest();
    // What comes back is this place's file as this write left it. Landing
    // after a newer write of the same place, it would put that place's
    // pre-write account back over the newer one.
    const mayLand = writeTicket(scope);
    return await heldDown(set, async () => {
      const attempt = await writing(scope, action);
      const response = attempt.ok
        ? attempt.value
        : { status: "error" as const, error: attempt.why };
      if (response.status === "ok") {
        if (mayLand())
          set({ views: replaceView(get().views, response.data), error: null });
        if (opts.successMessage) toast.success(opts.successMessage);
        await useScanStore.getState().refresh();
        return true;
      }
      set({ error: response.error });
      const retry: ErrorAction = {
        label: "Retry",
        // Inside a package-wide action this place is not the whole job:
        // retrying it alone would report success with the places after it
        // never attempted. The one that knows what is left says so.
        onClick: theRest ?? (() => void run(scope, action, opts)),
      };
      useProblemsStore.getState().showError({
        title: opts.title,
        message: response.error,
        steps: opts.steps,
        actions: [retry],
      });
      return false;
    });
  };
  return run;
}
