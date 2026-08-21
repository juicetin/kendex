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

const source = (over: Partial<PlacesSource> = {}): PlacesSource => ({
  manifests: {
    global: emptyDraft(),
    "/work/vg": emptyDraft(),
    "/work/hyprtrade": emptyDraft(),
  },
  rows: indexRows(
    [null, ...ROOTS].map((root) =>
      updateRow("gh", root, { updateAvailable: false }),
    ),
  ),
  updatesLoaded: true,
  ...over,
});

const render = (places: PlacesSource) => {
  const one = group();
  return renderToStaticMarkup(
    <Table>
      <TableBody>
        <InstalledRow
          group={one}
          origin={null}
          standings={placeStandings(
            places,
            one.kind,
            one.name,
            groupScopes(one),
          )}
          onOpen={() => {}}
          onOpenPlace={() => {}}
        />
      </TableBody>
    </Table>,
  );
};

const changedIn = (root: string) =>
  source({
    manifests: {
      global: emptyDraft(),
      "/work/vg": emptyDraft(),
      "/work/hyprtrade": emptyDraft(),
      [root]: { ...emptyDraft(), "skill-instructions": { gh: "use the CLI" } },
    },
  });

describe("the Library row's customized mark", () => {
  it("counts the places rather than claiming the package is changed", () => {
    const html = render(changedIn("/work/vg"));
    expect(html).toContain("Customized in 1 of 3 places");
    expect(html).toContain("vg — customized by you");
    expect(html).toContain("hyprtrade — as the author wrote it");
  });

  it("says nothing at all when no place is changed", () => {
    const html = render(source());
    expect(html).not.toContain("Customized");
    expect(html).toContain("3 locations");
  });

  it("names the place a local source leaves unaccounted for", () => {
    const html = render(
      source({
        rows: indexRows([
          updateRow("gh", null, { updateAvailable: false }),
          updateRow("gh", "/work/vg", { updateAvailable: false }),
        ]),
      }),
    );
    expect(html).toContain("not checked");
    expect(html).not.toContain("hyprtrade — as the author wrote it");
  });

  it("never lets the count imply a place it could not read", () => {
    const html = render({
      ...changedIn("/work/vg"),
      rows: indexRows([
        updateRow("gh", null, { updateAvailable: false }),
        updateRow("gh", "/work/vg", { updateAvailable: false }),
      ]),
    });
    expect(html).toContain("Customized in 1 of 3 places · 1 not checked");
  });

  it("marks a place whose files were hand-edited while up to date", () => {
    const html = render(
      source({
        rows: indexRows([
          updateRow("gh", null, { updateAvailable: false }),
          updateRow("gh", "/work/vg", {
            updateAvailable: false,
            blockedByLocalEdit: true,
            editedHarnesses: ["claude"],
          }),
          updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
        ]),
      }),
    );
    expect(html).toContain("Customized in 1 of 3 places");
  });

  it("carries a fork mark only for the places that hold a fork", () => {
    const html = render(
      source({
        rows: indexRows([
          updateRow("gh", null, { updateAvailable: false }),
          updateRow("gh", "/work/vg", { updateAvailable: false, forked: true }),
          updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
        ]),
      }),
    );
    expect(html).toContain("Forked in vg");
  });

  it("leaves the fork mark off when no place here holds one", () => {
    expect(render(source())).not.toContain("Forked");
  });
});
