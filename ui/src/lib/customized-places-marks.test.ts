import { describe, expect, it } from "vitest";
import { updateRow } from "@/components/updates-test-rows";
import {
  changed,
  EVERYWHERE,
  forkedHere,
  HYPR,
  source,
  VG,
} from "@/lib/places-test-source";
import {
  headerStanding,
  indexRows,
  markTarget,
  type PlacesSource,
  placeStandings,
  uncheckedPlaces,
} from "./customized-places";
import { type Draft, emptyDraft } from "./editor-draft";

// Everything a mark does once it is drawn: where it leads, which place it
// speaks for, and how many places it must not speak for.

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

describe("uncheckedPlaces", () => {
  it("counts every place a tally of customized ones cannot speak for", () => {
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
    // Still being read is as unsettled as read and unable to say.
    expect(
      uncheckedPlaces(standings({ ...changedOne, updatesLoaded: false })),
    ).toBe(2);
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
