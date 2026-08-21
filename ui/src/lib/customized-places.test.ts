import { describe, expect, it } from "vitest";
import type { Scope } from "@/bindings";
import { updateRow } from "@/components/updates-test-rows";
import {
  customizedPlaces,
  forkedPlaces,
  indexRows,
  manifestsOnScreen,
  type PlacesSource,
  placeStandings,
  standingIn,
} from "./customized-places";
import { type Draft, emptyDraft } from "./editor-draft";

const GLOBAL: Scope = { scope: "global" };
const VG: Scope = { scope: "project", root: "/work/vg" };
const HYPR: Scope = { scope: "project", root: "/work/hyprtrade" };
const EVERYWHERE = [GLOBAL, VG, HYPR];

const changed = (): Draft => ({
  ...emptyDraft(),
  "skill-instructions": { gh: "use the CLI" },
});

/** Every place readable and up to date, so each test names the one fact
 *  it is about. */
const source = (over: Partial<PlacesSource> = {}): PlacesSource => ({
  manifests: {
    global: emptyDraft(),
    "/work/vg": emptyDraft(),
    "/work/hyprtrade": emptyDraft(),
  },
  rows: indexRows(
    EVERYWHERE.map((scope) =>
      updateRow("gh", scope.scope === "global" ? null : scope.root, {
        updateAvailable: false,
      }),
    ),
  ),
  updatesLoaded: true,
  ...over,
});

const states = (over: Partial<PlacesSource> = {}) =>
  placeStandings(source(over), "skill", "gh", EVERYWHERE).map(
    (one) => one.state,
  );

describe("placeStandings", () => {
  it("marks the one place whose manifest changes the package", () => {
    expect(
      states({
        manifests: {
          global: emptyDraft(),
          "/work/vg": changed(),
          "/work/hyprtrade": emptyDraft(),
        },
      }),
    ).toEqual(["as-installed", "customized", "as-installed"]);
  });

  it("marks a place whose files were hand-edited while up to date", () => {
    const rows = indexRows([
      updateRow("gh", null, { updateAvailable: false }),
      updateRow("gh", "/work/vg", {
        updateAvailable: false,
        blockedByLocalEdit: true,
        editedHarnesses: ["claude"],
      }),
      updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
    ]);
    expect(states({ rows })).toEqual([
      "as-installed",
      "customized",
      "as-installed",
    ]);
  });

  it("leaves a place with no update row unknown, never as installed", () => {
    const rows = indexRows([
      updateRow("gh", null, { updateAvailable: false }),
      updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
    ]);
    expect(states({ rows })).toEqual([
      "as-installed",
      "unknown",
      "as-installed",
    ]);
  });

  it("leaves every place unknown while the update standing is stale", () => {
    expect(states({ updatesLoaded: false })).toEqual([
      "unknown",
      "unknown",
      "unknown",
    ]);
  });

  it("leaves a place whose manifest could not be read unknown", () => {
    expect(
      states({ manifests: { global: emptyDraft(), "/work/vg": emptyDraft() } }),
    ).toEqual(["as-installed", "as-installed", "unknown"]);
  });

  it("still marks a place the manifest changes when its files are unknown", () => {
    expect(
      states({
        manifests: {
          global: emptyDraft(),
          "/work/vg": changed(),
          "/work/hyprtrade": emptyDraft(),
        },
        rows: indexRows([updateRow("gh", null, { updateAvailable: false })]),
      }),
    ).toEqual(["as-installed", "customized", "unknown"]);
  });

  it("counts the places that are customized, not the installs", () => {
    const standings = placeStandings(
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
    expect(customizedPlaces(standings)).toEqual([VG]);
    expect(standingIn(standings, HYPR)?.state).toBe("as-installed");
    expect(standingIn(standings, { scope: "project", root: "/nowhere" })).toBe(
      null,
    );
  });

  it("matches a fork to the place it belongs to", () => {
    const rows = indexRows([
      updateRow("gh", null, { updateAvailable: false }),
      updateRow("gh", "/work/vg", { updateAvailable: false, forked: true }),
      updateRow("gh", "/work/hyprtrade", { updateAvailable: false }),
    ]);
    expect(
      forkedPlaces(placeStandings(source({ rows }), "skill", "gh", EVERYWHERE)),
    ).toEqual([VG]);
  });

  it("reads another package's places as its own, never this one's", () => {
    expect(
      placeStandings(source(), "skill", "orch", EVERYWHERE).map((s) => s.state),
    ).toEqual(["unknown", "unknown", "unknown"]);
  });
});

describe("manifestsOnScreen", () => {
  it("puts the draft in hand over the saved copy of the place being edited", () => {
    const saved = { global: emptyDraft(), "/work/vg": emptyDraft() };
    const manifests = manifestsOnScreen(saved, VG, changed());
    expect(manifests["/work/vg"]["skill-instructions"]).toEqual({
      gh: "use the CLI",
    });
    expect(manifests.global).toEqual(emptyDraft());
  });

  it("keeps every saved manifest when no draft is open", () => {
    const saved = { global: emptyDraft() };
    expect(manifestsOnScreen(saved, VG, null)).toBe(saved);
  });
});
