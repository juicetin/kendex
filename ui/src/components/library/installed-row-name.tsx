import type { Scope } from "@/bindings";
import { Badge } from "@/components/ui/badge";
import { TableCell } from "@/components/ui/table";
import { bundledWithLabel, vendorHelp } from "@/lib/copy";
import { forkedInLabel, forkedPlacesLabel } from "@/lib/copy-customize";
import { type PlaceStanding, uncheckedPlaces } from "@/lib/customized-places";
import type { ItemGroup } from "@/lib/derive";
import { kindIcon } from "@/lib/kind-icon";
import { describesItself, scopeName } from "@/lib/labels";
import { cn } from "@/lib/utils";
import { markClick } from "./row-click";

/** What the row calls this package: the kind icon carrying the
 *  customization colour, the name, the marks that belong beside it, and the
 *  description under all of it. */
export function NameCell({
  group,
  displayName,
  mark,
  vendor,
  forks,
  scopes,
  standings,
  onOpenFork,
}: {
  group: ItemGroup;
  displayName: string;
  /** What the Where cell says in words, repeated here as the icon's colour
   *  and its tooltip. Null where nothing here is changed. */
  mark: string | null;
  vendor: string | null;
  forks: Scope[];
  scopes: Scope[];
  standings: PlaceStanding[];
  onOpenFork: () => void;
}) {
  const Icon = kindIcon(group.kind);
  const named = forks.map((where) => scopeName(where, scopes));
  return (
    <TableCell className="max-w-[22rem] font-medium whitespace-normal">
      <span className="flex items-start gap-2">
        {/* The kind icon carries the customization colour, the legend
            above the table names what it means, and the Where cell in
            this same row repeats it in words. */}
        <span title={mark ?? undefined} className="mt-0.5 shrink-0">
          <Icon
            className={cn(
              "size-4",
              mark ? "text-customized" : "text-muted-foreground",
            )}
          />
        </span>
        <span className="min-w-0">
          <span className="flex items-center gap-1.5">
            <span className="block truncate">{displayName}</span>
            {forks.length > 0 ? (
              // The place is in the badge, not only in its tooltip: a mark
              // that says which place it is about says nothing to anyone
              // reading by touch or by keyboard otherwise. The tooltip
              // still carries the full list.
              <button
                type="button"
                // It names a place, so it opens that place. Left as text
                // the click reaches the row instead, which opens the row's
                // own first install — a badge saying one place and going
                // to another.
                onClick={markClick(onOpenFork)}
                title={forkedInLabel(named)}
              >
                <Badge variant="outline">
                  {forkedPlacesLabel(
                    named,
                    scopes.length,
                    uncheckedPlaces(standings),
                  )}
                </Badge>
              </button>
            ) : null}
            {vendor ? (
              <Badge variant="outline" title={vendorHelp(vendor)}>
                {bundledWithLabel(group.installations[0].harness)}
              </Badge>
            ) : null}
          </span>
          {group.description ? (
            <span
              className={cn(
                "line-clamp-2 text-xs font-normal text-muted-foreground",
                !describesItself(group.kind) && "font-mono text-[11px]",
              )}
            >
              {group.description}
            </span>
          ) : null}
        </span>
      </span>
    </TableCell>
  );
}
