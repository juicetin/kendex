import type { HarnessId, Origin, Scope } from "@/bindings";
import { HarnessBadge } from "@/components/harness-badge";
import { StatusDot } from "@/components/status-dot";
import { TagBadges } from "@/components/tag-badge";
import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { bundledWithLabel, FORKED_BADGE_LABEL, vendorHelp } from "@/lib/copy";
import {
  customizedPlacesLabel,
  forkedInLabel,
  placeStateLine,
  STATUS_LABELS,
} from "@/lib/copy-customize";
import {
  customizedPlaces,
  forkedPlaces,
  type PlaceStanding,
} from "@/lib/customized-places";
import {
  type GroupStatus,
  groupScopes,
  groupStatus,
  groupVendor,
  type ItemGroup,
} from "@/lib/derive";
import { kindIcon } from "@/lib/kind-icon";
import {
  describesItself,
  hookDisplayName,
  kindLabel,
  scopeName,
} from "@/lib/labels";
import { relativeTime } from "@/lib/relative-time";
import { cn } from "@/lib/utils";
import { originLabel, originTitle } from "@/stores/provenance";

const STATUS_TONES: Record<GroupStatus, "good" | "warning" | "critical"> = {
  active: "good",
  off: "warning",
  broken: "critical",
};

export function InstalledRow({
  group,
  origin,
  standings,
  onOpen,
  onOpenPlace,
}: {
  group: ItemGroup;
  origin: Origin | null;
  /** What is known about each place this package is installed in. */
  standings: PlaceStanding[];
  onOpen: () => void;
  /** Opens this package at one place, on what you changed there. */
  onOpenPlace: (scope: Scope) => void;
}) {
  const Icon = kindIcon(group.kind);
  const displayName =
    group.kind === "hook" ? hookDisplayName(group.name) : group.name;
  const vendor = groupVendor(group);
  const scopes = groupScopes(group);
  const status = groupStatus(group);
  const whereLabel =
    scopes.length === 1 ? scopeName(scopes[0]) : `${scopes.length} locations`;
  // The full path, so two projects sharing a folder name stay apart, and
  // what is known about each — including that nothing is.
  const whereTitle = standings
    .map((one) =>
      placeStateLine(
        one.scope.scope === "global" ? "Personal" : one.scope.root,
        one.state,
      ),
    )
    .join("\n");
  const changed = customizedPlaces(standings);
  const forks = forkedPlaces(standings);
  const unchecked = standings.filter((one) => one.state === "unknown").length;
  // The one thing the row says about your changes: which place, or how many
  // of them. Every other surface says it the same way.
  const mark =
    changed.length > 0
      ? customizedPlacesLabel(changed.map(scopeName), scopes.length, unchecked)
      : null;

  return (
    <TableRow onClick={onOpen} className="cursor-pointer">
      {/* Cells are nowrap by default; the description is the one column that
          wants to wrap rather than run out of the row and get cut mid-word. */}
      <TableCell className="max-w-[22rem] font-medium whitespace-normal">
        <span className="flex items-start gap-2">
          {/* The one place colour says something other than "which tool":
              the Library's legend names it, and the Where cell says the
              same in words for anyone who cannot see the difference. */}
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
                <Badge
                  variant="outline"
                  title={forkedInLabel(forks.map(scopeName))}
                >
                  {FORKED_BADGE_LABEL}
                </Badge>
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
      <TableCell className="align-top text-muted-foreground">
        {kindLabel(group.kind)}
      </TableCell>
      <TableCell className="align-top">
        <TagBadges tags={group.tags} />
      </TableCell>
      <TableCell>
        <span className="flex flex-wrap gap-1">
          {group.harnesses.map((h) => (
            <HarnessBadge key={h} harness={h as HarnessId} compact />
          ))}
          {group.shared ? (
            <Badge variant="secondary">Shared files</Badge>
          ) : null}
        </span>
      </TableCell>
      {/* Where a package is changed is where you go to see the change, so
          the mark is the way there — the row itself still opens the place
          it was installed from. */}
      <TableCell title={whereTitle} className="text-muted-foreground">
        {mark ? (
          <button
            type="button"
            className="max-w-[13rem] text-left whitespace-normal text-customized hover:underline"
            onClick={(event) => {
              event.stopPropagation();
              onOpenPlace(changed[0]);
            }}
          >
            {mark}
          </button>
        ) : (
          whereLabel
        )}
      </TableCell>
      <TableCell title={originTitle(origin)} className="text-muted-foreground">
        {originLabel(origin) || "—"}
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        {group.modifiedAt != null
          ? relativeTime(group.modifiedAt * 1000, Date.now())
          : "—"}
      </TableCell>
      {/* A dot, not a word: seven rows of "Active" say nothing the colour
          doesn't, and the words are back on hover for anyone who wants them. */}
      <TableCell>
        <Tooltip>
          <TooltipTrigger
            render={
              <span className="flex w-full justify-center py-1">
                <StatusDot tone={STATUS_TONES[status]} />
                <span className="sr-only">{STATUS_LABELS[status]}</span>
              </span>
            }
          />
          <TooltipContent side="left">{STATUS_LABELS[status]}</TooltipContent>
        </Tooltip>
      </TableCell>
    </TableRow>
  );
}
