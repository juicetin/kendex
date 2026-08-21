import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
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
import { InstalledRow, markClick } from "./installed-row";

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
    // The full path, so two projects sharing a folder name stay apart.
    expect(html).toContain("/work/vg — customized by you");
    expect(html).toContain("/work/hyprtrade — as the author wrote it");
  });

  it("colours the icon exactly when a place is changed", () => {
    expect(render(changedIn("/work/vg"))).toContain("text-customized");
    expect(render(source())).not.toContain("text-customized");
  });

  it("makes the mark the way to the place, and plain text when there is none", () => {
    expect(render(changedIn("/work/vg"))).toContain("<button");
    expect(render(source())).not.toContain("<button");
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
    expect(html).toContain("Customized in 1 of 3 places");
  });

  it("leaves the fork mark off when no place here holds one", () => {
    expect(render(source())).not.toContain("Forked");
  });

  it("opens the place it names, and never also the row's own", () => {
    const open = vi.fn();
    const stopPropagation = vi.fn();
    markClick(open)({ stopPropagation });
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledOnce();
  });
});
