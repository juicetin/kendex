// Customization is per place. A package installed at User level and in two
// projects can be changed in one of them and untouched in the others, so
// every mark the app draws is about one place — never about the package as
// a whole. Three facts make a place yours: the manifest overlay that
// place's Customize tab writes, files edited by hand there, and a fork,
// which is that place's own copy of the package. This joins them, and says
// how far a read got rather than guessing when a fact is not in hand.

import { useMemo } from "react";
import type { ItemKind, Scope, UpdateRow } from "@/bindings";
import { isCustomized, itemCustomization } from "@/lib/customization";
import type { Draft } from "@/lib/editor-draft";
import { sameScope, scopeKey } from "@/lib/scope";
import { useEditorStore } from "@/stores/editor";
import type { PackageView } from "@/stores/nav";
import { useUpdatesStore } from "@/stores/updates";

/** What is known about one place's copy of a package. `unknown` is a real
 *  answer, not a default: a path or local source gets no update row, so
 *  nothing can say whether its files were edited by hand. `checking` is
 *  the answer while a read is still on its way — a place nobody has asked
 *  about yet has not been given up on. */
export type PlaceState = "customized" | "as-installed" | "checking" | "unknown";

/** What makes a place yours, and so where its mark leads: settings live on
 *  the Customize tab, files on the overview beside the edited-files notice
 *  that offers the decision. */
export type PlaceChange = "files" | "settings";

export interface PlaceStanding {
  scope: Scope;
  state: PlaceState;
  /** What made it `customized`, or null when nothing did. */
  change: PlaceChange | null;
  /** This place's copy is a fork of what the catalog carries, read from
   *  that place's own manifest — the same table `package_meta` reads, so
   *  the fact never depends on an update check having succeeded. */
  forked: boolean;
}

/** Everything the standings are read from, gathered once per screen. */
export interface PlacesSource {
  /** Each place's manifest as it stands on screen, keyed by scope. */
  manifests: Record<string, Draft>;
  /** Update rows keyed by {@link placeKey} — the per-place hand-edit fact,
   *  absent for the places the engine cannot speak about. */
  rows: Map<string, UpdateRow>;
  /** False while the update standing is stale — hand-edits are unread. */
  updatesLoaded: boolean;
  /** False until every place's manifest has been read once, so a manifest
   *  missing from {@link manifests} is told apart from one that failed. */
  manifestsLoaded: boolean;
}

const placeKey = (kind: ItemKind, name: string, scope: Scope): string =>
  `${kind}:${name}:${scopeKey(scope)}`;

export function placeStandings(
  source: PlacesSource,
  kind: ItemKind,
  name: string,
  scopes: Scope[],
): PlaceStanding[] {
  return scopes.map((scope) => {
    const manifest = source.manifests[scopeKey(scope)];
    const row = source.rows.get(placeKey(kind, name, scope));
    // Null is "could not be read", which is why neither reads as false: a
    // manifest that failed to load and a place with no update row both
    // leave the question open.
    const overlay = manifest
      ? isCustomized(itemCustomization(manifest, kind, name))
      : null;
    const handEdited = source.updatesLoaded
      ? (row?.blockedByLocalEdit ?? null)
      : null;
    const forked = manifest?.forks?.[kind]?.[name] != null;
    // A hand edit outranks an overlay: it is the one waiting on a decision.
    // A fork is the same kind of fact — this place's own bytes.
    const change: PlaceChange | null =
      handEdited === true || forked
        ? "files"
        : overlay === true
          ? "settings"
          : null;
    // A read still on its way is not a read that came back empty: saying
    // "not checked" of a place nobody has asked about yet names the wrong
    // cause, and this screen is the one that asks.
    const stillReading =
      (overlay === null && !source.manifestsLoaded) || !source.updatesLoaded;
    const state: PlaceState =
      change != null
        ? "customized"
        : stillReading
          ? "checking"
          : overlay === null || handEdited === null
            ? "unknown"
            : "as-installed";
    return { scope, state, change, forked };
  });
}

export const customizedPlaces = (standings: PlaceStanding[]): Scope[] =>
  standings.filter((one) => one.state === "customized").map((one) => one.scope);

export const forkedPlaces = (standings: PlaceStanding[]): Scope[] =>
  standings.filter((one) => one.forked).map((one) => one.scope);

/** How many places a mark cannot speak for: reads that came back unable to
 *  say. A read still on its way is not one of them — every launch would
 *  otherwise open by calling places unchecked and then take it back. */
export const uncheckedPlaces = (standings: PlaceStanding[]): number =>
  standings.filter((one) => one.state === "unknown").length;

/** Whether anything on this screen is marked, without building the list to
 *  find out — the Library asks once per group to decide on its colour key. */
export const anyCustomized = (standings: PlaceStanding[]): boolean =>
  standings.some((one) => one.state === "customized");

/** This place's update row, from the index every screen already shares —
 *  so a page reading one fact off a row never scans the whole list again. */
export const rowIn = (
  source: PlacesSource,
  kind: ItemKind,
  name: string,
  scope: Scope,
): UpdateRow | null => source.rows.get(placeKey(kind, name, scope)) ?? null;

export const standingIn = (
  standings: PlaceStanding[],
  scope: Scope,
): PlaceStanding | null =>
  standings.find((one) => sameScope(one.scope, scope)) ?? null;

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

/** Every place's manifest as it stands on screen: saved everywhere, and the
 *  draft in hand for the one place being edited — so a chip, a row, and the
 *  header badge never disagree about the place you are typing in. */
export function manifestsOnScreen(
  saved: Record<string, Draft>,
  scope: Scope,
  draft: Draft | null,
): Record<string, Draft> {
  if (!draft) return saved;
  return { ...saved, [scopeKey(scope)]: draft };
}

export const indexRows = (rows: UpdateRow[]): Map<string, UpdateRow> =>
  new Map(rows.map((row) => [placeKey(row.kind, row.name, row.scope), row]));

// One index per rows array, shared by every screen that reads it: the
// store replaces the array whenever the rows change, so identity is the
// whole test — nothing here can go stale behind the store.
let indexedRows: UpdateRow[] | null = null;
let index: Map<string, UpdateRow> = new Map();
const indexFor = (rows: UpdateRow[]): Map<string, UpdateRow> => {
  if (rows !== indexedRows) {
    indexedRows = rows;
    index = indexRows(rows);
  }
  return index;
};

function usePlaces(withDraft: boolean): PlacesSource {
  const saved = useEditorStore((s) => s.saved);
  const scope = useEditorStore((s) => s.scope);
  const draft = useEditorStore((s) => s.draft);
  const manifestsLoaded = useEditorStore((s) => s.manifestsLoaded);
  const rows = useUpdatesStore((s) => s.rows);
  const updatesLoaded = useUpdatesStore((s) => s.loaded);
  const manifests = useMemo(
    () => (withDraft ? manifestsOnScreen(saved, scope, draft) : saved),
    [withDraft, saved, scope, draft],
  );
  return useMemo(
    () => ({
      manifests,
      rows: indexFor(rows),
      updatesLoaded,
      manifestsLoaded,
    }),
    [manifests, rows, updatesLoaded, manifestsLoaded],
  );
}

/** What is actually customized: saved manifests and the update standing.
 *  The Library's question, and the one a mark on a row answers. */
export const usePlacesSource = (): PlacesSource => usePlaces(false);

/** The same, with the manifest being typed overlaid on the place it
 *  belongs to — so the header, the chips and the box you are typing in
 *  never disagree. Only the editor's own surfaces want this: text you have
 *  not saved has changed nothing yet. */
export const useEditingPlacesSource = (): PlacesSource => usePlaces(true);
