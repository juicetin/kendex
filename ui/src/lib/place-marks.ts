import type {
  HarnessId,
  ItemKind,
  ObservedItem,
  Scope,
  UpdateRow,
} from "@/bindings";
import {
  editedRowIn,
  type PlaceStanding,
  type PlacesSource,
  placeStandings,
  standingIn,
} from "@/lib/customized-places";
import {
  groupItems,
  groupScopes,
  type ItemGroup,
  installationIn,
} from "@/lib/derive";
import { sameScope, scopeKey } from "@/lib/scope";
import type { PackageRef, PackageView } from "@/stores/nav";

// Where a mark leads. A mark that says a place is yours is only worth
// clicking if it opens the surface holding what it marks, so the fact that
// made the mark decides the destination — and every surface drawing one
// asks here rather than deciding for itself.

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

/** Where the fork mark leads: the place whose copy is its own, and the
 *  overview that holds it.
 *
 *  Its own question because the answer differs. A row can carry both marks
 *  — settings changed in one place, a fork kept in another — and the
 *  customized mark names the first place carrying any change, which is not
 *  the forked one. A badge that names a place and opens a different one is
 *  worse than a badge that does not open at all. */
export function forkNav(
  group: { kind: ItemKind; name: string },
  standings: PlaceStanding[],
): [PackageRef, PackageView | undefined] | null {
  const found = standings.find((one) => one.forked);
  if (!found) return null;
  return [
    { kind: group.kind, name: group.name, scope: found.scope },
    undefined,
  ];
}

/** The Customize index's Open. Every row it lists is an overlay written on
 *  the Customize tab, so it opens that tab rather than the overview the
 *  page would otherwise default to. */
export const customizeNav = (ref: PackageRef): [PackageRef, PackageView] => [
  ref,
  { mode: "customize" },
];

/** Everything a package page derives about one place before it renders:
 *  the installation it is about, the place its header speaks for, and the
 *  row behind its edited-files notice. All three are about the place the
 *  page was opened at — which a customized mark can name any of — so they
 *  are derived together and the page cannot say two things about one
 *  place. The Customize tab's chips move the editor, not the page: a title
 *  following a chip while the actions under it stay put is the same split
 *  this page exists to close. `primary` is null when nothing is installed
 *  at that place, which is the page's cue to leave the way the reader
 *  came. */
export function packageMarks(
  source: PlacesSource,
  group: ItemGroup,
  opened: Scope,
): {
  primary: ObservedItem | null;
  selected: PlaceStanding | null;
  editedRow: UpdateRow | null;
} {
  const { kind, name } = group;
  const standings = placeStandings(source, kind, name, groupScopes(group));
  return {
    primary: installationIn(group, opened),
    selected: standingIn(standings, opened),
    editedRow: editedRowIn(source, kind, name, opened),
  };
}

/** The group a page about one place may describe itself from.
 *
 *  A group folds `harnesses`, `tags`, `shared` and the modification time
 *  across its installations, and those span places — so a group built from
 *  all of them describes a union of places while the header names one. The
 *  full group still answers the question it is for: where else this lives.
 */
export function groupHere(group: ItemGroup, scope: Scope): ItemGroup | null {
  const mine = group.installations.filter((one) => sameScope(one.scope, scope));
  return groupItems(mine)[0] ?? null;
}

/** Which tools carry this package in each place, keyed by place.
 *
 *  The Customize tab's chips move between places, so one list for all of
 *  them would offer a place settings for a tool it does not install the
 *  package under — written to a rendering nobody has. */
export function carriedBy(group: ItemGroup): Record<string, HarnessId[]> {
  const byPlace: Record<string, HarnessId[]> = {};
  for (const one of group.installations) {
    const key = scopeKey(one.scope);
    const seen = byPlace[key] ?? [];
    if (!seen.includes(one.harness)) byPlace[key] = [...seen, one.harness];
  }
  return byPlace;
}
