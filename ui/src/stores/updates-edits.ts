import { toast } from "sonner";
import { commands, type Scope, type UpdateRow } from "@/bindings";
import { forkedToastLabel } from "@/lib/copy";
import {
  FORK_ERROR_TITLE,
  UNSAVED_FIRST_BODY,
  UNSAVED_FIRST_STEPS,
  UNSAVED_FIRST_TITLE,
} from "@/lib/copy-forks";
import { packageDisplayName } from "@/lib/labels";
import { sameScope } from "@/lib/scope";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";
import { useUpdatesStore } from "./updates";

/** The two ways out of an edited place, run under the updates store's
 *  busy flag so every control on the page waits on the same one — a fork
 *  or a discard rewrites the scope's manifest like any update does. */

const run = async (scope: Scope, work: () => Promise<string | null>) => {
  // Both of these rewrite this scope's kendex.toml, which the Customize tab
  // may be holding an older copy of. Saving that copy afterwards would put
  // the pre-fork contents back, and the fork record lives nowhere else.
  const editor = useEditorStore.getState();
  if (editor.dirty && sameScope(editor.scope, scope)) {
    useProblemsStore.getState().showError({
      title: UNSAVED_FIRST_TITLE,
      message: UNSAVED_FIRST_BODY,
      steps: UNSAVED_FIRST_STEPS,
    });
    return;
  }
  useUpdatesStore.setState({ busy: true });
  try {
    const error = await work();
    if (error !== null) {
      useProblemsStore
        .getState()
        .showError({ title: FORK_ERROR_TITLE, message: error });
      return;
    }
    await useUpdatesStore.getState().load();
    // A fork rewrites the manifest exactly as a save does, and the fork
    // fact every mark reads comes from that file — without this the badge
    // it just earned stays off until the window is refocused.
    await useEditorStore.getState().loadAll();
    // The pass fills `saved`; the copy in hand for the place being edited
    // is a different read, and every editor surface joins through it. Left
    // stale it hides the new fork, and a later Save would write the
    // pre-fork manifest back over it.
    //
    // The refusal above is a check at entry, and this runs seconds later
    // with nothing stopping someone typing in between — so the dirty flag
    // is read again here, not assumed. Typing that arrived since is kept
    // and the place is marked outdated instead: what is in hand was read
    // before this rewrite, and saving it would put the old file back over
    // the record just made. The save refuses and says so rather than this
    // choosing silently between the two losses.
    const editor = useEditorStore.getState();
    if (!sameScope(editor.scope, scope)) return;
    if (editor.dirty) editor.outdate(scope);
    else await editor.load(scope);
    await useScanStore.getState().refresh();
    await useAuditStore.getState().refresh({ force: true });
  } finally {
    useUpdatesStore.setState({ busy: false });
  }
};

/** Keep an edited place's files as a local fork of its own. Only some
 *  tools' renderings read back as source; the row names the edited one a
 *  fork can take, and the button is not offered without it. */
export const keepAsOwn = async (row: UpdateRow): Promise<void> => {
  const harness = row.forkableHarness;
  if (!harness) return;
  await run(row.scope, async () => {
    const response = await commands.packageFork(
      row.scope,
      row.kind,
      row.name,
      harness,
    );
    if (response.status === "error") return response.error;
    toast.success(forkedToastLabel(packageDisplayName(row)));
    return null;
  });
};

/** Drop an edited place's edits and take the newest version — moving the
 *  hold along when the place is held, in the same apply. */
export const takeNewVersion = async (row: UpdateRow): Promise<void> => {
  await run(row.scope, async () => {
    const response = await commands.applyDiscardEdits(
      row.scope,
      row.kind,
      row.name,
      // A held place moves to the newest only when that is its own hold
      // to move and the newest is known; otherwise the discard restores
      // what is resolved now.
      row.pinned && row.canTakeLatest ? (row.latest?.commit ?? null) : null,
    );
    return response.status === "error" ? response.error : null;
  });
};
