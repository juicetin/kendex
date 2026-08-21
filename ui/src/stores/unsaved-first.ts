import type { Scope } from "@/bindings";
import {
  UNSAVED_FIRST_BODY,
  UNSAVED_FIRST_TITLE,
  unsavedFirstSteps,
} from "@/lib/copy-forks";
import { scopeName } from "@/lib/labels";
import { sameScope, scopeKey } from "@/lib/scope";
import { useEditorStore } from "./editor";
import { useProblemsStore } from "./problems";

/** Whether unsaved customization for a place refuses a write to it, saying
 *  so if it does.
 *
 *  Every mutation that rewrites a place's kendex.toml asks this before it
 *  starts. The editor holds a whole copy of that file and a save writes the
 *  whole copy back, so a write that lands underneath one makes the copy
 *  unsavable — and the only way on from there discards what was typed.
 *
 *  The typing may be on screen or parked behind another place: moving
 *  between places keeps typing rather than dropping it, and a move is not a
 *  ruling on it. Reaching only the copy on screen would let where someone
 *  happens to be standing decide whether the write is refused, which is not
 *  a choice anyone made. Anything parked is unsaved by construction.
 *
 *  This lives in one place because the list of writers is not fixed: a new
 *  one that forgot to ask would be a control that stays live while the file
 *  moves under it, which is exactly the fault this closes. */
export function refusesForUnsaved(scope: Scope): boolean {
  const editor = useEditorStore.getState();
  const parked = editor.held[scopeKey(scope)] !== undefined;
  if (!parked && !(editor.dirty && sameScope(editor.scope, scope)))
    return false;
  useProblemsStore.getState().showError({
    title: UNSAVED_FIRST_TITLE,
    message: UNSAVED_FIRST_BODY,
    // Parked typing is not on screen, so the way back to it is named.
    steps: unsavedFirstSteps(parked ? scopeName(scope) : null),
  });
  return true;
}

/** The same question about several places at once, for one click that
 *  writes them all.
 *
 *  Asked per place inside the loop, the first refusal stops that place and
 *  the loop carries on writing the others — one click leaving the package
 *  changed in two projects and not the third, with nothing said about
 *  which. A package-wide action either goes everywhere or nowhere. */
export function refusesForUnsavedIn(scopes: Scope[]): boolean {
  const editor = useEditorStore.getState();
  const held = scopes.find(
    (scope) => editor.held[scopeKey(scope)] !== undefined,
  );
  const onScreen = scopes.find(
    (scope) => editor.dirty && sameScope(editor.scope, scope),
  );
  const blocking = held ?? onScreen;
  if (!blocking) return false;
  useProblemsStore.getState().showError({
    title: UNSAVED_FIRST_TITLE,
    message: UNSAVED_FIRST_BODY,
    steps: unsavedFirstSteps(held ? scopeName(blocking) : null),
  });
  return true;
}
