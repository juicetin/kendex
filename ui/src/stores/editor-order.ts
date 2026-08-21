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
