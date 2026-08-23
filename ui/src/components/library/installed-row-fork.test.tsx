import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ObservedItem, Scope } from "@/bindings";
import { Table, TableBody } from "@/components/ui/table";
import { updateRow } from "@/components/updates-test-rows";
import {
  indexRows,
  type PlacesSource,
  placeStandings,
} from "@/lib/customized-places";
import { groupItems, groupScopes } from "@/lib/derive";
import { emptyDraft } from "@/lib/editor-draft";
import { source } from "@/lib/places-test-source";
import { InstalledRow } from "./installed-row";

const ROOTS = ["/work/vg", "/work/hyprtrade"];

const install = (scope: Scope): ObservedItem => ({
  kind: "skill",
  name: "gh",
  harness: "claude",
  scope,
  path: "/h/.claude/skills/gh",
  fileState: { state: "dir" },
  enabled: true,
  origin: null,
  description: null,
  tags: [],
  modifiedAt: null,
  vendor: null,
});

/** The package installed at User level and in two projects, with every
 *  place readable and current unless a test says otherwise. */
const group = () =>
  groupItems([
    install({ scope: "global" }),
    ...ROOTS.map((root) => install({ scope: "project", root })),
  ])[0];

const render = (places: PlacesSource) => {
  const one = group();
  return renderToStaticMarkup(
    <Table>
      <TableBody>
        <InstalledRow
          group={one}
          origin={null}
          originError={null}
          standings={placeStandings(
            places,
            one.kind,
            one.name,
            groupScopes(one),
          )}
          onOpen={() => {}}
          onOpenPlace={() => {}}
          onOpenFork={() => {}}
        />
      </TableBody>
    </Table>,
  );
};

// The fork mark on a Library row: which places it speaks for, what it will
// not claim about the ones it could not read, and that it can be pressed —
// a badge naming a place has to be able to open it.
describe("the Library row's fork mark", () => {
  it("carries a fork mark only for the places that hold a fork", () => {
    const html = render(
      source({
        manifests: {
          global: emptyDraft(),
          "/work/vg": {
            ...emptyDraft(),
            forks: { skill: { gh: { source: "cat", "forked-at": "2026" } } },
          },
          "/work/hyprtrade": emptyDraft(),
        },
      }),
    );
    expect(html).toContain("Forked in vg");
    expect(html).toContain("Customized in vg · 1 of 3 places");
  });

  // "1 of 3" says the other two are not forks. Where a place could not be
  // read there is no answer either way, and the mark says so rather than
  // counting it among the settled.
  it("says how many places it could not speak for beside the fork count", () => {
    const html = render(
      source({
        manifests: {
          global: emptyDraft(),
          "/work/vg": {
            ...emptyDraft(),
            forks: { skill: { gh: { source: "cat", "forked-at": "2026" } } },
          },
          "/work/hyprtrade": emptyDraft(),
        },
        // This place's manifest is last-known rather than read, and its
        // update row cannot stand in for it.
        unreadPlaces: new Set(["/work/hyprtrade"]),
        rows: indexRows([
          updateRow("gh", null, { updateAvailable: false }),
          updateRow("gh", "/work/vg", { updateAvailable: false }),
        ]),
      }),
    );
    expect(html).toContain("Forked in vg · 1 of 3 places · 1 not checked");
  });

  // The badge names a place, so it has to be able to open it. Left as
  // plain text the click reaches the row instead, which opens the row's own
  // first install — a badge saying one place and going to another. Static
  // markup carries no handlers, so what is pinned here is that the badge is
  // something you can press; `forkNav` is where it leads.
  it("makes the fork badge pressable rather than plain text", () => {
    const standings = placeStandings(
      source({
        manifests: {
          global: emptyDraft(),
          "/work/vg": emptyDraft(),
          "/work/hyprtrade": {
            ...emptyDraft(),
            forks: { skill: { gh: { source: "cat", "forked-at": "2026" } } },
          },
        },
      }),
      "skill",
      "gh",
      groupScopes(group()),
    );
    const html = renderToStaticMarkup(
      <Table>
        <TableBody>
          <InstalledRow
            group={group()}
            origin={null}
            originError={null}
            standings={standings}
            onOpen={() => {}}
            onOpenPlace={() => {}}
            onOpenFork={() => {}}
          />
        </TableBody>
      </Table>,
    );
    const at = html.indexOf("Forked in");
    expect(at).toBeGreaterThan(-1);
    const opened = html.lastIndexOf("<button", at);
    expect(opened).toBeGreaterThan(-1);
    expect(html.slice(opened, at)).not.toContain("</button>");
  });

  it("leaves the fork mark off when no place here holds one", () => {
    expect(render(source())).not.toContain("Forked");
  });
});
