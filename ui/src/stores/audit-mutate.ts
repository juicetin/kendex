import { toast } from "sonner";
import type { AuditView, Scope } from "@/bindings";
import { replaceView } from "./audit";
import { manifestRewritten } from "./manifest-sync";
import { type ErrorAction, useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { refusesForUnsaved, retryTheRest } from "./unsaved-first";

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
/** Run the command and turn a transport failure into an answer. Left to
 *  reject it would leave the funnel by a path that owes nothing — no
 *  message, no sync, and the busy flag coming down on a file that may have
 *  moved. */
const attempt = async (
  action: () => Promise<
    { status: "ok"; data: AuditView } | { status: "error"; error: string }
  >,
): Promise<
  { status: "ok"; data: AuditView } | { status: "error"; error: string }
> => {
  try {
    return await action();
  } catch (thrown) {
    return { status: "error", error: String(thrown) };
  }
};

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
    set({ busy: true });
    // Busy is one of the flags holding the Customize tab's Save bar down, so
    // it stays up until the editor has been told its copy is stale — clearing
    // it any earlier leaves a window where a save passes the outdated check
    // and writes the pre-action manifest back.
    try {
      // Whatever the outcome. Several of these commands write in stages —
      // a fork captures and records before it renders, an adoption moves
      // files before it plans — so an error can arrive with the file
      // already changed, and nothing in the answer says which. Asking is
      // the only way to know, and asking costs a read: the sync re-reads
      // and compares, so where nothing moved it takes its own mark back
      // off. Guessing costs the record, since the copy the Customize tab
      // holds would write over it.
      const response = await attempt(action);
      await manifestRewritten(scope);
      if (response.status === "ok") {
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
        onClick: retryTheRest() ?? (() => void run(scope, action, opts)),
      };
      useProblemsStore.getState().showError({
        title: opts.title,
        message: response.error,
        steps: opts.steps,
        actions: [retry],
      });
      return false;
    } finally {
      set({ busy: false });
    }
  };
  return run;
}
