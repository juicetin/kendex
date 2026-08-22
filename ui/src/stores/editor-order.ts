import type { Draft } from "@/lib/editor-draft";
import { keepIfSame } from "@/lib/same-read";

/** Every place whose manifest would not read, with what it said. */
export type Unread = Readonly<Record<string, string>>;

/** Which read each place's saved manifest came from, so an older one
 *  landing late is dropped for that place alone rather than for the whole
 *  pass. Three readers overlap — one place at a time, every place at once,
 *  and the re-read a save ends with — and without this the one that happens
 *  to land last wins, reverting a place someone just read or just saved. */
export function manifestFold() {
  const behind = new Map<string, number>();
  /** Fold freshly read manifests into the saved ones, skipping any place a
   *  later read already answered for, and keeping the object already on
   *  screen when nothing moved. */
  return (
    previous: Record<string, Draft>,
    read: [string, Draft][],
    token: number,
  ): Record<string, Draft> => {
    const next = { ...previous };
    for (const [key, draft] of read) {
      if ((behind.get(key) ?? 0) > token) continue;
      behind.set(key, token);
      next[key] = draft;
    }
    return keepIfSame(previous, next);
  };
}

/** Fold how each place's read went, under the same per-place ordering the
 *  manifests get.
 *
 *  The mark says a manifest in hand answers for a moment nobody re-checked,
 *  so it travels with the reads themselves: set when a place's read fails,
 *  cleared when one lands. Kept as a whole-list replacement it went wrong
 *  twice over — a pass that failed put back marks a newer targeted read had
 *  already cleared, and a single place's failure never set one at all,
 *  leaving its stale manifest reading as current. Per place, newest wins,
 *  exactly as `saved` does. */
export function unreadFold() {
  const behind = new Map<string, number>();
  /** `read` gives each place either why it would not read, or null for one
   *  that did. The reason is folded with the mark rather than kept beside
   *  it: two lists saying the same thing can disagree, and the one that
   *  went stale was the note on screen — still naming a place that had
   *  already read again, with a retry for a failure that was over. */
  return (
    previous: Unread,
    read: [string, string | null][],
    token: number,
  ): Unread => {
    const next = { ...previous };
    for (const [key, why] of read) {
      if ((behind.get(key) ?? 0) > token) continue;
      behind.set(key, token);
      if (why === null) delete next[key];
      else next[key] = why;
    }
    return keepIfSame(previous, next);
  };
}

/** What to say about the manifests behind the marks: why the last whole
 *  pass could not run at all, and why each place still unread would not
 *  read — each reason said once, since a pass that reached nowhere gives
 *  every place the same one.
 *
 *  Derived rather than kept. Held as its own string it went stale the one
 *  way that matters: a place recovering on a targeted read cleared its
 *  mark and left the note still naming it, still offering a retry for a
 *  failure that was over. */
export function whyUnread(state: {
  passError: string | null;
  unreadPlaces: Unread;
}): string | null {
  const said = [
    ...new Set([state.passError, ...Object.values(state.unreadPlaces)]),
  ].filter((why): why is string => why !== null);
  return said.length > 0 ? said.join("\n") : null;
}
