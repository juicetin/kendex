import type { Scope } from "@/bindings";
import { scopeKey } from "@/lib/scope";

/** Which answer about the audit may speak, when several overlap.
 *
 *  Held here rather than in the store so the ordering can be read — and
 *  tested — as one thing, apart from what it happens to guard. */
export function auditTickets() {
  // Every answer takes a number. Without one, the last to land wins, and
  // which lands last is decided by the machine rather than by which
  // question was asked most recently.
  let reads = 0;
  // Per place, because two places are two files: a write to one says
  // nothing about the other, and one counter would have the second write
  // silence the first place's answer for no reason.
  const writes = new Map<string, number>();

  /** A read of every place at once. It may land only while nothing has
   *  begun since: whatever started later either wrote a file or read it
   *  afterwards, and either way this snapshot is the older account. */
  const read = () => {
    reads += 1;
    const mine = reads;
    return () => mine === reads;
  };

  /** One place's write. Its answer is the file as this write left it, so a
   *  read still in flight is the older account and stands down for it —
   *  but only until the same place is written again. */
  const write = (scope: Scope) => {
    reads += 1;
    const key = scopeKey(scope);
    const mine = (writes.get(key) ?? 0) + 1;
    writes.set(key, mine);
    return () => writes.get(key) === mine;
  };

  return { read, write };
}

/** The one audit store's ordering. */
export const { read: readTicket, write: writeTicket } = auditTickets();
