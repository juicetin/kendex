import type { Scope } from "@/bindings";
import { sameScope } from "@/lib/scope";
import { useEditorStore } from "./editor";

/** Tell the editor that something outside it rewrote a place's kendex.toml.
 *
 *  Nearly every mutation does: forking, discarding edits, moving a hold,
 *  adopting, toggling, subscribing, installing. The editor holds a whole
 *  manifest read at some earlier moment, and a save writes that whole copy
 *  back — so without this, the next save silently undoes whatever was just
 *  recorded, and the marks drawn from `saved` stay stale until a refocus.
 *
 *  The copy in hand is re-read when nothing is unsaved. When something is,
 *  it is kept and the place marked outdated, so the save refuses with its
 *  reason rather than this choosing between losing the typing and losing
 *  the record. */
export async function manifestRewritten(scope: Scope): Promise<void> {
  const editor = useEditorStore.getState();
  await editor.loadAll();
  const after = useEditorStore.getState();
  if (!sameScope(after.scope, scope)) return;
  if (after.dirty) after.outdate(scope);
  else await after.load(scope);
}
