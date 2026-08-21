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
  anyCustomized,
  customizedPlaces,
  headerStanding,
  indexRows,
  markTarget,
  type PlacesSource,
  placeStandings,
  rowIn,
  uncheckedPlaces,
} from "./customized-places";
import { type Draft, emptyDraft } from "./editor-draft";

// Everything a mark does once it is drawn: where it leads, which place it
// speaks for, and how many places it must not speak for.

describe("rowIn", () => {
  it("hands back one place's row, so a page reads it instead of scanning", () => {
    const places = source();
    expect(rowIn(places, "skill", "gh", VG)?.scope).toEqual(VG);
    expect(rowIn(places, "skill", "gh", { scope: "project", root: "/x" })).toBe(
      null,
    );
    expect(rowIn(places, "agent", "gh", VG)).toBe(null);
  });
});

describe("markTarget", () => {
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

describe("anyCustomized", () => {
  it("answers the colour key's question without building a list", () => {
    const standings = (manifests: Record<string, Draft>) =>
      placeStandings(source({ manifests }), "skill", "gh", EVERYWHERE);
    expect(anyCustomized(standings(plainManifests()))).toBe(false);
    expect(
      anyCustomized(standings({ ...plainManifests(), "/work/vg": changed() })),
    ).toBe(true);
  });
});

describe("uncheckedPlaces", () => {
  it("counts only the places a read came back unable to speak for", () => {
    const standings = (over: Partial<PlacesSource>) =>
      placeStandings(source(over), "skill", "gh", EVERYWHERE);
    const changedOne = {
      manifests: {
        global: emptyDraft(),
        "/work/vg": changed(),
        "/work/hyprtrade": emptyDraft(),
      },
    };
    expect(uncheckedPlaces(standings(changedOne))).toBe(0);
    // A read on its way is not one a mark must apologise for: every launch
    // would otherwise open by calling places unchecked and then take it back.
    expect(
      uncheckedPlaces(standings({ ...changedOne, updatesLoaded: false })),
    ).toBe(0);
    expect(
      uncheckedPlaces(
        standings({
          ...changedOne,
          rows: indexRows([updateRow("gh", null, { updateAvailable: false })]),
        }),
      ),
    ).toBe(1);
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
