import type { UpdateRow } from "@/bindings";
import { packageCount } from "@/lib/update-groups";

// Which rows are worth saying something about, and where. Pure over a list
// the store fetches, so every screen asks the same question the same way.

/** A row worth a line on the page: a newer version, a package gone from
 *  its source, or installs disagreeing on their version — each a standing
 *  fact someone can act on. */
const noteworthy = (row: UpdateRow): boolean =>
  row.updateAvailable || row.removedUpstream || row.mixed;

/** The sidebar badge's number: packages with news someone would want to
 *  hear, counted once however many places they are installed in. Ignored
 *  ones asked not to be counted; held ones still count — a hold is "not
 *  yet", not "never tell me". */
export const visibleUpdateCount = (rows: UpdateRow[]): number =>
  packageCount(visibleUpdates(rows));

/** The Updates page's main list: everything noteworthy that has not been
 *  muted. */
export const visibleUpdates = (rows: UpdateRow[]): UpdateRow[] =>
  rows.filter((row) => noteworthy(row) && !row.ignored);

/** The collapsed "hidden updates" section: muted packages whose news is
 *  still real — with the way back out. */
export const hiddenUpdates = (rows: UpdateRow[]): UpdateRow[] =>
  rows.filter((row) => noteworthy(row) && row.ignored);

/** The packages Home asks you to decide about: files edited by hand, with
 *  the decision still open. A fork edited since is not one of them — it has
 *  no source to refresh from and nothing left to keep as your own — and the
 *  package page shows it no notice either. */
export const awaitingForkDecision = (rows: UpdateRow[]): UpdateRow[] =>
  rows.filter((row) => row.blockedByLocalEdit && !row.forked);
