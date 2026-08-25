// Grouping for the two kinds of note that ride beside findings: rules that
// had nothing to read on a clean row, and render or parse warnings. Kept
// apart from the finding grouping, which is what a decision targets.
import type { ItemKind, ItemSafety, ItemWarning } from "@/bindings";

export interface SkipGroup {
  reason: string;
  /** The engine rule id behind the reason — a stable name where the reason
   *  text itself is dynamic (a hook's script path rides inside it). */
  rule: string;
  count: number;
  /** The shared kind, or null when the reason spans more than one kind. */
  kind: ItemKind | null;
}

// A skipped rule says a rule had no bytes to read, not that anything is
// wrong — and it says so whatever else the row carries: a hook whose
// script could not be read still has a command and an entry that scored,
// and findings on those must not hide the gap. The first skipped rule's
// reason stands in for the row, matching how a single row already
// summarizes "not fully checked" today.
export function groupSkipped(rows: ItemSafety[]): SkipGroup[] {
  const groups = new Map<string, SkipGroup>();
  for (const row of rows) {
    if (row.skipped.length === 0) continue;
    const { reason, rule } = row.skipped[0];
    const group = groups.get(reason);
    if (!group) groups.set(reason, { reason, rule, count: 1, kind: row.kind });
    else {
      group.count += 1;
      if (group.kind !== row.kind) group.kind = null;
    }
  }
  return [...groups.values()];
}

export interface WarningGroup {
  message: string;
  remediation: string | null;
  items: { kind: ItemKind; name: string }[];
}

/** Dedupes render/parse warnings across items by (message, remediation). */
export function groupWarnings(warnings: ItemWarning[]): WarningGroup[] {
  const groups = new Map<string, WarningGroup>();
  for (const warning of warnings) {
    const key = `${warning.message}::${warning.remediation ?? ""}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        message: warning.message,
        remediation: warning.remediation ?? null,
        items: [],
      };
      groups.set(key, group);
    }
    group.items.push({ kind: warning.kind, name: warning.name });
  }
  return [...groups.values()];
}
