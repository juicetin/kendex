import type { Scope } from "@/bindings";
import type { Draft } from "@/lib/editor-draft";
import { scopeKey } from "@/lib/scope";

/** Typing waiting at the place it was read from, with the base it was read
 *  against — so a file rewritten while it waited still refuses its save,
 *  the same refusal it would have met without the wait. */
export interface HeldDraft {
  scope: Scope;
  draft: Draft;
  base: string | null;
}

/** Every place holding typing the editor is not showing, keyed by place. */
export type Held = Record<string, HeldDraft>;

/** The part of the editor a move between places rewrites. */
interface Pointed {
  scope: Scope;
  draft: Draft | null;
  base: string | null;
  dirty: boolean;
  held: Held;
}

/** Point the editor at another place.
 *
 *  A manifest belongs to one place, so the copy in hand belongs to the
 *  place it was read from and not to the one being opened. It waits there
 *  instead of being dropped, and whatever was already waiting at the place
 *  being opened comes back out. Crossing places is how the per-place marks
 *  are meant to be used — every mark is a link to another place — so the
 *  move itself must never cost someone what they typed. */
export function pointAt(state: Pointed, scope: Scope): Pointed {
  const held = { ...state.held };
  if (state.dirty && state.draft)
    held[scopeKey(state.scope)] = {
      scope: state.scope,
      draft: state.draft,
      base: state.base,
    };
  const waiting = held[scopeKey(scope)];
  // In hand and waiting would be the same copy counted twice, and the note
  // about typing left elsewhere would name the place on screen.
  delete held[scopeKey(scope)];
  return {
    scope,
    held,
    draft: waiting?.draft ?? null,
    base: waiting?.base ?? null,
    // Typing that comes back out is still unsaved: the Save bar stays up,
    // and the read that follows leaves it alone rather than reading over it.
    dirty: waiting !== undefined,
  };
}
