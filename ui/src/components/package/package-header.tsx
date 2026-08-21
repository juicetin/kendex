import type { ReactNode } from "react";
import type { ItemKind } from "@/bindings";
import { InlineMarkdown } from "@/components/inline-markdown";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { FORKED_BADGE_LABEL } from "@/lib/copy";
import { customizedInLabel, forkedInLabel } from "@/lib/copy-customize";
import { kindIcon } from "@/lib/kind-icon";

/** The package page's title block: what this is, what it says about itself,
 *  and the things you can do to it. */
export function PackageHeader({
  kind,
  displayName,
  description,
  forked,
  customized,
  place,
  action,
}: {
  kind: ItemKind;
  displayName: string;
  description: string | null;
  forked: boolean;
  customized: boolean;
  /** The place both marks are about — the one the Customize tab has open,
   *  or null while the page is still working out which that is. */
  place: string | null;
  action: ReactNode;
}) {
  const Icon = kindIcon(kind);
  return (
    <PageHeader
      wide
      title={
        // The icon centres on the text's own line box, not on the flex row:
        // a badge alongside makes the row taller than the words, and
        // centring against that visibly floats the icon off the title.
        <span className="flex items-baseline gap-2.5">
          <Icon className="size-5 shrink-0 translate-y-[0.1875rem] text-muted-foreground" />
          <span className="min-w-0 truncate">{displayName}</span>
          {forked && place ? (
            <Badge variant="outline" title={forkedInLabel([place])}>
              {FORKED_BADGE_LABEL}
            </Badge>
          ) : null}
          {customized && place ? (
            <Badge variant="customized">{customizedInLabel(place)}</Badge>
          ) : null}
        </span>
      }
      subtitle={
        description ? <InlineMarkdown source={description} /> : undefined
      }
      action={action}
    />
  );
}
