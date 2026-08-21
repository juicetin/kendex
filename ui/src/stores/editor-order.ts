import type { Draft } from "@/lib/editor-draft";
import { keepIfSame } from "@/lib/same-read";

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
  return (
    previous: string[],
    read: [string, boolean][],
    token: number,
  ): string[] => {
    const next = new Set(previous);
    for (const [key, unread] of read) {
      if ((behind.get(key) ?? 0) > token) continue;
      behind.set(key, token);
      if (unread) next.add(key);
      else next.delete(key);
    }
    const grown = [...next].sort();
    // The same identity the manifests keep: a list that says what it said
    // before is the list already on screen, and the marks derived from it
    // do not re-render.
    return grown.length === previous.length &&
      grown.every((key, at) => key === previous[at])
      ? previous
      : grown;
  };
}
