import type { ProvenanceRow } from "@/bindings";
import { InstalledRow } from "@/components/library/installed-row";
import { InstalledSkeleton } from "@/components/library/installed-skeleton";
import { LibraryLegend } from "@/components/library/library-legend";
import { TableEmptyRow } from "@/components/library/table-empty";
import {
  Table,
  TableBody,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { TAGS_ROW_LABEL } from "@/lib/copy";
import {
  customizedPlaces,
  placeStandings,
  usePlacesSource,
} from "@/lib/customized-places";
import { groupScopes, type ItemGroup } from "@/lib/derive";
import { useNavStore } from "@/stores/nav";
import { originFor } from "@/stores/provenance";

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
  const rows = groups.map((group) => ({
    group,
    standings: placeStandings(
      places,
      group.kind,
      group.name,
      groupScopes(group),
    ),
  }));

  return (
    <>
      {rows.some((row) => customizedPlaces(row.standings).length > 0) ? (
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
          {rows.map(({ group, standings }) => {
            const primary = group.installations[0];
            return (
              <InstalledRow
                key={group.key}
                group={group}
                origin={originFor(
                  provenance,
                  group.kind,
                  group.name,
                  groupScopes(group),
                )}
                standings={standings}
                onOpen={() => {
                  if (!primary) return;
                  goToPackage({
                    kind: group.kind,
                    name: group.name,
                    scope: primary.scope,
                  });
                }}
                onOpenPlace={(scope) =>
                  goToPackage(
                    { kind: group.kind, name: group.name, scope },
                    { mode: "customize" },
                  )
                }
              />
            );
          })}
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
