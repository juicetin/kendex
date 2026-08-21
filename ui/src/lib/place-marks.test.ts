import { describe, expect, it } from "vitest";
import { updateRow } from "@/components/updates-test-rows";
import {
  changed,
  EVERYWHERE,
  forkedHere,
  HYPR,
  plainManifests,
  source,
  VG,
} from "@/lib/places-test-source";
import {
  customizedPlaces,
  indexRows,
  placeStandings,
  standingIn,
} from "./customized-places";
import { type Draft, emptyDraft } from "./editor-draft";
import {
  customizeNav,
  headerStanding,
  markTarget,
  packageMarks,
} from "./place-marks";

// Where a mark leads once it is drawn, and which place each of a package
// page's own marks speaks for.

describe("markTarget", () => {
  it("leads to the tab that holds the settings, even where the place is forked", () => {
    // A fork is the standing state of this place; instructions typed on the
    // Customize tab are the thing someone went and did. The mark must not
    // walk past them to the overview.
    const both = {
      ...forkedHere(),
      "skill-instructions": { gh: "use the CLI" },
    };
    const standings = placeStandings(
      source({ manifests: { ...plainManifests(), "/work/vg": both } }),
      "skill",
      "gh",
      EVERYWHERE,
    );
    expect(markTarget(standings)).toEqual({
      scope: VG,
      view: { mode: "customize" },
    });
    expect(standingIn(standings, VG)?.forked).toBe(true);
  });

  const targetFor = (manifests: Record<string, Draft>) =>
    markTarget(
      placeStandings(source({ manifests }), "skill", "gh", EVERYWHERE),
    );
  const plain = {
    global: emptyDraft(),
    "/work/vg": emptyDraft(),
    "/work/hyprtrade": emptyDraft(),
  };

  it("opens the Customize tab of the place whose settings were changed", () => {
    expect(targetFor({ ...plain, "/work/vg": changed() })).toEqual({
      scope: VG,
      view: { mode: "customize" },
    });
  });

  it("opens the overview where the change is in the files", () => {
    expect(targetFor({ ...plain, "/work/vg": forkedHere() })).toEqual({
      scope: VG,
    });
  });

  it("leads nowhere when no place is changed", () => {
    expect(targetFor(plain)).toBe(null);
  });
});

describe("markTarget", () => {
  it("opens the first place the mark names, so the label can say where", () => {
    const standings = placeStandings(
      source({
        manifests: {
          global: emptyDraft(),
          "/work/vg": changed(),
          "/work/hyprtrade": changed(),
        },
      }),
      "skill",
      "gh",
      EVERYWHERE,
    );
    // The Library's label names customizedPlaces[0]; the click must land
    // there, or the mark sends the reader somewhere it never mentioned.
    expect(markTarget(standings)?.scope).toEqual(
      customizedPlaces(standings)[0],
    );
    expect(customizedPlaces(standings)).toEqual([VG, HYPR]);
  });
});

describe("headerStanding", () => {
  const standings = () =>
    placeStandings(
      source({
        manifests: {
          global: emptyDraft(),
          "/work/vg": changed(),
          "/work/hyprtrade": emptyDraft(),
        },
      }),
      "skill",
      "gh",
      EVERYWHERE,
    );

  it("is the place this page was opened at until the editor points here", () => {
    expect(headerStanding(standings(), HYPR, null)?.scope).toEqual(HYPR);
    // The editor carries over the package last edited, so its scope is not
    // this page's answer until it names a place this package lives in.
    expect(headerStanding(standings(), HYPR, VG)?.scope).toEqual(VG);
    expect(
      headerStanding(standings(), HYPR, { scope: "project", root: "/nowhere" })
        ?.scope,
    ).toEqual(HYPR);
  });
});

describe("customizeNav", () => {
  it("opens the tab that wrote what the index is listing", () => {
    // Every row on the Customize index is an overlay written on that tab,
    // so landing on the overview would be landing away from it.
    expect(customizeNav({ kind: "skill", name: "gh", scope: VG })).toEqual([
      { kind: "skill", name: "gh", scope: VG },
      { mode: "customize" },
    ]);
  });
});

describe("packageMarks", () => {
  const marks = (opened: typeof VG, editing: typeof VG | null) =>
    packageMarks(
      source({
        manifests: { ...plainManifests(), "/work/vg": forkedHere() },
        rows: indexRows([
          updateRow("gh", null, { updateAvailable: false }),
          updateRow("gh", "/work/vg", { updateAvailable: false }),
          updateRow("gh", "/work/hyprtrade", {
            updateAvailable: false,
            blockedByLocalEdit: true,
          }),
        ]),
      }),
      "skill",
      "gh",
      EVERYWHERE,
      opened,
      editing,
    );

  it("speaks for the place the page was opened at until the editor points here", () => {
    expect(marks(HYPR, null).selected?.scope).toEqual(HYPR);
    expect(marks(HYPR, VG).selected?.scope).toEqual(VG);
  });

  it("reads the fork and the hand edit off the place the page is about", () => {
    // Both are about the opened place, never the one the header names.
    expect(marks(VG, HYPR).forkedHere).toBe(true);
    expect(marks(HYPR, VG).forkedHere).toBe(false);
    expect(marks(HYPR, VG).editedRow?.scope).toEqual(HYPR);
    expect(marks(VG, HYPR).editedRow).toBe(null);
  });
});
