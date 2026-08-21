import type { ItemKind, Scope, UpdateRow } from "@/bindings";
import {
  editedRowIn,
  type PlaceStanding,
  type PlacesSource,
  placeStandings,
  standingIn,
} from "@/lib/customized-places";
import type { PackageRef, PackageView } from "@/stores/nav";

// Where a mark leads. A mark that says a place is yours is only worth
// clicking if it opens the surface holding what it marks, so the fact that
// made the mark decides the destination — and every surface drawing one
// asks here rather than deciding for itself.

/** The place a package page's header marks are about: the one the
 *  Customize tab has open, once the editor is pointed at this package, and
 *  the place the page was opened at until then — the editor carries over
 *  the last package edited, which is not this one. */
export const headerStanding = (
  standings: PlaceStanding[],
  opened: Scope,
  editing: Scope | null,
): PlaceStanding | null =>
  (editing ? standingIn(standings, editing) : null) ??
  standingIn(standings, opened);

/** Where a mark leads: the first place carrying a change, and what the
 *  package page opens showing — the surface that holds that change, not
 *  whichever tab the page defaults to. Null when nothing is changed. */
export function markTarget(
  standings: PlaceStanding[],
): { scope: Scope; view?: PackageView } | null {
  const found = standings.find((one) => one.change != null);
  if (!found) return null;
  if (found.change === "settings")
    return { scope: found.scope, view: { mode: "customize" } };
  return { scope: found.scope };
}

/** The customized mark's destination as the two arguments the nav takes:
 *  the place the mark names — never the row's own first install — and the
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

/** The Customize index's Open. Every row it lists is an overlay written on
 *  the Customize tab, so it opens that tab rather than the overview the
 *  page would otherwise default to. */
export const customizeNav = (ref: PackageRef): [PackageRef, PackageView] => [
  ref,
  { mode: "customize" },
];

/** Everything a package page's marks are about, from one join: the place
 *  its header speaks for, whether the place it was opened at holds a fork,
 *  and that place's row when its files were edited by hand. One owner, so
 *  the page cannot say two things about one place. */
export function packageMarks(
  source: PlacesSource,
  kind: ItemKind,
  name: string,
  scopes: Scope[],
  opened: Scope,
  editing: Scope | null,
): {
  selected: PlaceStanding | null;
  forkedHere: boolean;
  editedRow: UpdateRow | null;
} {
  const standings = placeStandings(source, kind, name, scopes);
  return {
    selected: headerStanding(standings, opened, editing),
    forkedHere: standingIn(standings, opened)?.forked === true,
    editedRow: editedRowIn(source, kind, name, opened),
  };
}
