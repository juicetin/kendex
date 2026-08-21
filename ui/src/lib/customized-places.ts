// Customization is per place. A package installed at User level and in two
// projects can be changed in one of them and untouched in the others, so
// every mark the app draws is about one place — never about the package as
// a whole. Two facts make a place yours: the manifest overlay that place's
// Customize tab writes, and files edited by hand in that place. This joins
// both, and says "unknown" rather than guessing when either is unreadable.

import { useMemo } from "react";
import type { ItemKind, Scope, UpdateRow } from "@/bindings";
import { isCustomized, itemCustomization } from "@/lib/customization";
import type { Draft } from "@/lib/editor-draft";
import { sameScope, scopeKey } from "@/lib/scope";
import { useEditorStore } from "@/stores/editor";
import { useUpdatesStore } from "@/stores/updates";

/** What is known about one place's copy of a package. `unknown` is a real
 *  answer, not a default: a path or local source gets no update row, so
 *  nothing can say whether its files were edited by hand. */
export type PlaceState = "customized" | "as-installed" | "unknown";

export interface PlaceStanding {
  scope: Scope;
  state: PlaceState;
  /** This place's copy is a fork of what the catalog carries. */
  forked: boolean;
}

/** Everything the standings are read from, gathered once per screen. */
export interface PlacesSource {
  /** Each place's manifest as it stands on screen, keyed by scope. */
  manifests: Record<string, Draft>;
  /** Update rows keyed by {@link placeKey} — the per-place hand-edit and
   *  fork facts, absent for the places the engine cannot speak about. */
  rows: Map<string, UpdateRow>;
  /** False while the update standing is stale — hand-edits are unknown. */
  updatesLoaded: boolean;
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
    const known = source.updatesLoaded && row != null;
    const handEdited = known ? row.blockedByLocalEdit : null;
    const state: PlaceState =
      overlay === true || handEdited === true
        ? "customized"
        : overlay === null || handEdited === null
          ? "unknown"
          : "as-installed";
    return { scope, state, forked: known && row.forked };
  });
}

export const customizedPlaces = (standings: PlaceStanding[]): Scope[] =>
  standings.filter((one) => one.state === "customized").map((one) => one.scope);

export const forkedPlaces = (standings: PlaceStanding[]): Scope[] =>
  standings.filter((one) => one.forked).map((one) => one.scope);

export const standingIn = (
  standings: PlaceStanding[],
  scope: Scope,
): PlaceStanding | null =>
  standings.find((one) => sameScope(one.scope, scope)) ?? null;

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

/** The two stores every per-place mark reads, joined once per screen. */
export function usePlacesSource(): PlacesSource {
  const saved = useEditorStore((s) => s.saved);
  const scope = useEditorStore((s) => s.scope);
  const draft = useEditorStore((s) => s.draft);
  const rows = useUpdatesStore((s) => s.rows);
  const updatesLoaded = useUpdatesStore((s) => s.loaded);
  const manifests = useMemo(
    () => manifestsOnScreen(saved, scope, draft),
    [saved, scope, draft],
  );
  const indexed = useMemo(() => indexRows(rows), [rows]);
  return { manifests, rows: indexed, updatesLoaded };
}
