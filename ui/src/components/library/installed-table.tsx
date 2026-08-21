import { useCallback, useMemo } from "react";
import type { ItemKind, ProvenanceRow } from "@/bindings";
import { InstalledRow } from "@/components/library/installed-row";
import { InstalledSkeleton } from "@/components/library/installed-skeleton";
import { LibraryLegend } from "@/components/library/library-legend";
import { TableEmptyRow } from "@/components/library/table-empty";
import { MarksNote } from "@/components/marks-note";
import {
  Table,
  TableBody,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { TAGS_ROW_LABEL } from "@/lib/copy";
import {
  anyCustomized,
  markTarget,
  type PlaceStanding,
  placeStandings,
  usePlacesSource,
} from "@/lib/customized-places";
import { groupScopes, type ItemGroup } from "@/lib/derive";
import { type PackageRef, type PackageView, useNavStore } from "@/stores/nav";
import { originFor } from "@/stores/provenance";

/** Where the customized mark goes, as the two arguments the nav takes: the
 *  place the mark names — never the row's own first install — and the
 *  surface holding what was changed there. Null when nothing is changed. */
export function markNav(
  group: { kind: ItemKind; name: string },
  standings: PlaceStanding[],
): [PackageRef, PackageView | undefined] | null {
  const target = markTarget(standings);
  if (!target) return null;
  return [
    { kind: group.kind, name: group.name, scope: target.scope },
    target.view,
  ];
}

/** The Library's table: one row per package, each carrying what is known
 *  about every place it is installed in. */
export function InstalledTable({
  groups,
  provenance,
  scanning,
  hasAnyItems,
  onClearFilters,
  onBrowse,
}: {
  groups: ItemGroup[];
  provenance: ProvenanceRow[];
  /** Nothing has been counted yet — distinct from "counted, found none". */
  scanning: boolean;
  hasAnyItems: boolean;
  onClearFilters: () => void;
  onBrowse: () => void;
}) {
  const goToPackage = useNavStore((s) => s.goToPackage);
  const places = usePlacesSource();
  // One join per change of its inputs, and every handler kept stable, so a
  // reload that moved nothing re-renders no row. That rests on both stores
  // handing back their previous value when a re-read says the same thing
  // (lib/same-read.ts): a fresh array of identical rows would defeat every
  // memo below it.
  const rows = useMemo(
    () =>
      groups.map((group) => {
        const scopes = groupScopes(group);
        return {
          group,
          standings: placeStandings(places, group.kind, group.name, scopes),
          origin: originFor(provenance, group.kind, group.name, scopes),
        };
      }),
    [groups, places, provenance],
  );
  const openRow = useCallback(
    (group: ItemGroup) => {
      const primary = group.installations[0];
      if (!primary) return;
      goToPackage({ kind: group.kind, name: group.name, scope: primary.scope });
    },
    [goToPackage],
  );
  const openMark = useCallback(
    (group: ItemGroup, standings: PlaceStanding[]) => {
      const nav = markNav(group, standings);
      if (nav) goToPackage(...nav);
    },
    [goToPackage],
  );

  return (
    <>
      <MarksNote />
      {rows.some((row) => anyCustomized(row.standings)) ? (
        <LibraryLegend />
      ) : null}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Type</TableHead>
            <TableHead>{TAGS_ROW_LABEL}</TableHead>
            <TableHead>Harnesses</TableHead>
            <TableHead>Where</TableHead>
            <TableHead>From</TableHead>
            <TableHead className="text-right">Updated</TableHead>
            <TableHead>Status</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map(({ group, standings, origin }) => (
            <InstalledRow
              key={group.key}
              group={group}
              origin={origin}
              standings={standings}
              onOpen={openRow}
              onOpenPlace={openMark}
            />
          ))}
          {scanning ? <InstalledSkeleton /> : null}
          {!scanning && groups.length === 0 ? (
            <TableEmptyRow
              hasAnyItems={hasAnyItems}
              onClearFilters={onClearFilters}
              onBrowse={onBrowse}
            />
          ) : null}
        </TableBody>
      </Table>
    </>
  );
}
