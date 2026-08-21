import { commands } from "@/bindings";
import {
  OUTDATED_DRAFT_BODY,
  OUTDATED_DRAFT_TITLE,
  RELOAD_SETTINGS_LABEL,
} from "@/lib/copy-forks";
import { sameScope, scopeKey } from "@/lib/scope";
import { useAuditStore } from "./audit";
import { useEditorStore } from "./editor";
import { named } from "./editor-scopes";
import { useProblemsStore } from "./problems";
import { useScanStore } from "./scan";

// Which save the screen's saving state belongs to. Two can be in flight —
// nothing stops a second press once the first is away — and everything
// after the await is about the place that was written, not whichever place
// is open when the response lands.
let writes = 0;

/** Write the copy in hand to the place it was read from, then re-read that
 *  place. Refuses when what is in hand predates a rewrite of that same
 *  file: putting it back would undo the record that rewrite made. */
export const saveManifest = async (): Promise<void> => {
  // Scope and draft are one value: read apart, a place switch between the
  // two reads sends one place's manifest to another place's file.
  const { scope, draft, outdated, load } = useEditorStore.getState();
  if (!draft) return;
  // What is in hand was read before this place's manifest was rewritten,
  // so writing it would put the older file back over what was recorded —
  // a fork's own entry lives nowhere else. Refusing loudly beats choosing
  // silently between losing that and losing what was typed.
  if (outdated === scopeKey(scope)) {
    useProblemsStore.getState().showError({
      title: OUTDATED_DRAFT_TITLE,
      message: OUTDATED_DRAFT_BODY,
      actions: [
        {
          label: RELOAD_SETTINGS_LABEL,
          // Reloading is the deliberate act of taking the newer file over
          // what is on screen; every other read here leaves typing alone.
          onClick: () => void load(scope, { discardEdits: true }),
        },
      ],
    });
    return;
  }
  writes += 1;
  const token = writes;
  const mine = () => token === writes;
  const onScreen = () =>
    mine() && sameScope(useEditorStore.getState().scope, scope);
  useEditorStore.setState({ saving: true });
  let response: Awaited<ReturnType<typeof commands.updateManifest>>;
  try {
    response = await commands.updateManifest(scope, draft);
  } catch (thrown) {
    if (mine())
      useEditorStore.setState({
        saving: false,
        error: `${named(scope)}: ${thrown}`,
      });
    return;
  }
  if (mine()) useEditorStore.setState({ saving: false });
  // A newer save owns what the screen says about saving.
  if (!mine()) return;
  if (response.status === "error") {
    // The note is about the place that was written, which may not be the
    // one on screen any more — so it names that place rather than letting
    // the reader assume the one in front of them.
    useEditorStore.setState({
      error: onScreen() ? response.error : `${named(scope)}: ${response.error}`,
    });
    return;
  }
  if (onScreen()) useEditorStore.setState({ error: null });
  // What is on screen is what the file now holds, so it is no longer
  // unsaved — the Save bar comes down and the place chips come live. Only
  // for the copy that was written: typing that arrived while the write was
  // away is a newer draft, and it stays unsaved until its own save. `edit`
  // builds a new draft rather than mutating, so identity is that test, and
  // the re-read below leaves a newer draft alone for the same reason.
  if (onScreen() && useEditorStore.getState().draft === draft)
    useEditorStore.setState({ dirty: false });
  // Re-read the place that was written, never whichever is open now, or
  // its saved manifest keeps the pre-save content and its mark with it.
  await load(scope);
  await useAuditStore.getState().refresh();
  await useScanStore.getState().refresh();
};
