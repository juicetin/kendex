import { RefreshCw } from "lucide-react";
import { StatusNote } from "@/components/status-note";
import { Button } from "@/components/ui/button";
import { CHECK_FOR_UPDATES_LABEL } from "@/lib/copy";
import {
  MARKS_UNREAD_MANIFESTS,
  MARKS_UNREAD_TITLE,
  MARKS_UNREAD_UPDATES,
} from "@/lib/copy-customize";
import { cn } from "@/lib/utils";
import { useEditorStore } from "@/stores/editor";
import { useUpdatesStore } from "@/stores/updates";

/** Said out loud when a read behind the per-place marks failed. Without it
 *  a failed read renders as a table of packages with nothing marked, which
 *  reads as "nothing of yours is here" — the one thing it does not mean. */
export function MarksNote() {
  const updatesError = useUpdatesStore((s) => s.error);
  const checking = useUpdatesStore((s) => s.checking);
  const check = useUpdatesStore((s) => s.check);
  const manifestError = useEditorStore((s) => s.manifestError);
  const loadAll = useEditorStore((s) => s.loadAll);
  if (!updatesError && !manifestError) return null;
  return (
    <StatusNote
      tone="warning"
      title={MARKS_UNREAD_TITLE}
      className="mb-3"
      action={
        <Button
          size="sm"
          variant="outline"
          disabled={checking}
          onClick={() => {
            void check();
            void loadAll();
          }}
        >
          <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
          {CHECK_FOR_UPDATES_LABEL}
        </Button>
      }
    >
      <span className="flex flex-col gap-1">
        {updatesError ? <span>{MARKS_UNREAD_UPDATES}</span> : null}
        {manifestError ? <span>{MARKS_UNREAD_MANIFESTS}</span> : null}
        <span className="whitespace-pre-wrap text-xs">
          {[updatesError, manifestError].filter(Boolean).join("\n")}
        </span>
      </span>
    </StatusNote>
  );
}
