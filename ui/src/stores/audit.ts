import { toast } from "sonner";
import { create } from "zustand";
import {
  type AuditView,
  commands,
  type DismissReason,
  type HarnessId,
  type ItemKind,
  type Scope,
} from "@/bindings";
import { adoptedToastLabel } from "@/lib/copy";
import {
  ignoredToast,
  TAKEN_BACK_TOAST,
  UNDO_LABEL,
} from "@/lib/copy-decisions";
import { manifestRewritten } from "./manifest-sync";
import { type ErrorAction, useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { refusesForUnsaved } from "./unsaved-first";

interface AuditState {
  views: AuditView[];
  auditing: boolean;
  error: string | null;
  busy: boolean;
  /** The startup audit has already toasted its failure — suppresses repeat
   * toasts on every silent retry until one succeeds. */
  backgroundFailureAnnounced: boolean;
  /** Unix ms of the last audit that came back clean; null until one has. */
  auditedAt: number | null;
  refresh: (opts?: { force?: boolean }) => Promise<void>;
  applyPlan: (
    scope: Scope,
    removeOrphans: boolean,
    allowUnsafe?: string[],
  ) => Promise<void>;
  adopt: (
    scope: Scope,
    kind: ItemKind,
    name: string,
    harness: HarnessId,
    opts?: { silent?: boolean },
  ) => Promise<void>;
  toggle: (
    scope: Scope,
    kind: ItemKind,
    name: string,
    enabled: boolean,
  ) => Promise<void>;
  removeItem: (scope: Scope, kind: ItemKind, name: string) => Promise<void>;
  /** Rule that these findings are not problems. The toast offers Undo,
   *  which takes back exactly the records this call wrote. */
  dismiss: (
    scope: Scope,
    tokens: string[],
    reason: DismissReason,
  ) => Promise<void>;
}

/** How long an audit answers for before a visit pays for a fresh one. */
const AUDIT_FRESH_FOR_MS = 60_000;

function replaceView(views: AuditView[], fresh: AuditView): AuditView[] {
  return views.map((view) =>
    sameScope(view.scope, fresh.scope) ? fresh : view,
  );
}

export function sameScope(a: Scope, b: Scope): boolean {
  if (a.scope === "global" && b.scope === "global") return true;
  return a.scope === "project" && b.scope === "project" && a.root === b.root;
}

export const useAuditStore = create<AuditState>((set, get) => {
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
  ) => {
    // Apply, adopt, toggle and remove all rewrite this scope's kendex.toml,
    // so unsaved customization for it refuses them the way a fork or a
    // discard is refused — before anything is written, and wherever the
    // typing is waiting.
    if (refusesForUnsaved(scope)) return;
    set({ busy: true });
    // Busy is one of the flags holding the Customize tab's Save bar down, so
    // it stays up until the editor has been told its copy is stale — clearing
    // it any earlier leaves a window where a save passes the outdated check
    // and writes the pre-action manifest back.
    try {
      const response = await action();
      if (response.status === "ok") {
        set({ views: replaceView(get().views, response.data), error: null });
        if (opts.successMessage) toast.success(opts.successMessage);
        await manifestRewritten(scope);
        await useScanStore.getState().refresh();
        return;
      }
      set({ error: response.error });
      const retry: ErrorAction = {
        label: "Retry",
        onClick: () => void run(scope, action, opts),
      };
      useProblemsStore.getState().showError({
        title: opts.title,
        message: response.error,
        steps: opts.steps,
        actions: [retry],
      });
    } finally {
      set({ busy: false });
    }
  };

  return {
    views: [],
    auditedAt: null,
    auditing: false,
    error: null,
    busy: false,
    backgroundFailureAnnounced: false,

    // Every visit to Review used to re-audit the whole machine, which is
    // seconds of work to answer a question already on screen. A recent
    // answer is reused; anything the app itself changes refreshes the scope
    // it changed, and a stale window closes on its own inside a minute.
    refresh: async (opts) => {
      if (get().auditing) return;
      const auditedAt = get().auditedAt;
      const fresh =
        auditedAt != null && Date.now() - auditedAt < AUDIT_FRESH_FOR_MS;
      if (fresh && !opts?.force) return;
      set({ auditing: true });
      try {
        const response = await commands.auditAll();
        if (response.status === "ok") {
          set({
            views: response.data,
            auditedAt: Date.now(),
            error: null,
            backgroundFailureAnnounced: false,
          });
        } else {
          set({ error: response.error });
          if (!get().backgroundFailureAnnounced) {
            toast.error(response.error);
            set({ backgroundFailureAnnounced: true });
          }
        }
      } finally {
        set({ auditing: false });
      }
    },

    applyPlan: (scope, removeOrphans, allowUnsafe = []) =>
      run(scope, () => commands.applyPlan(scope, removeOrphans, allowUnsafe), {
        title: "Couldn't apply these changes",
        steps: [
          "Nothing was changed — try again",
          "If it keeps failing, check the project folder is writable",
        ],
      }),
    // A merged row adopts every one of its installations in one click —
    // each is its own backend call, but they're one thing to the user, so
    // only the first speaks up with a toast.
    adopt: (scope, kind, name, harness, opts) =>
      run(scope, () => commands.adoptItem(scope, kind, name, harness), {
        title: `Couldn't start managing ${name}`,
        successMessage: opts?.silent ? undefined : adoptedToastLabel(name),
        steps: ["Try again"],
      }),
    toggle: (scope, kind, name, enabled) =>
      run(scope, () => commands.toggleItem(scope, kind, name, enabled), {
        title: `Couldn't ${enabled ? "turn on" : "turn off"} ${name}`,
        steps: ["Try again"],
      }),
    removeItem: (scope, kind, name) =>
      run(scope, () => commands.removeItem(scope, kind, name), {
        title: `Couldn't remove ${name}`,
        steps: ["Try again"],
      }),
    // A dismissal is the one action whose success carries a way back on the
    // toast itself: the undo names the exact records that were written, so
    // an old toast can never take back a newer decision at the same key. It
    // is written into this place's kendex.toml like every other decision, so
    // busy stays up — the Save bar with it — until the editor has been told
    // the copy it holds is stale.
    dismiss: async (scope, tokens, reason) => {
      set({ busy: true });
      try {
        const response = await commands.dismissFindings(scope, tokens, reason);
        if (response.status !== "ok") {
          set({ error: response.error });
          useProblemsStore.getState().showError({
            title: "Couldn't dismiss this finding",
            message: response.error,
            steps: [
              "Nothing was changed — read the finding again and decide again",
            ],
          });
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
                  for (const record of records) {
                    latest = await commands.revokeDismissal(
                      scope,
                      record.key,
                      record.fingerprint,
                      record.dismissedAt,
                    );
                    if (latest.status !== "ok") break;
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
    },
  };
});
