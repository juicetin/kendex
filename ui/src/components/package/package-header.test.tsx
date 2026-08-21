import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PlaceStanding } from "@/lib/customized-places";
import { VG } from "@/lib/places-test-source";
import { PackageHeader } from "./package-header";

// The badge is about the place the Customize tab has open, which the
// project chips change under it — so it names that place or it is not there
// at all. "Customized" on its own answers a question nobody asked.
const standing = (over: Partial<PlaceStanding> = {}): PlaceStanding => ({
  scope: VG,
  state: "customized",
  change: "settings",
  forked: false,
  ...over,
});

const header = (place: PlaceStanding | null = standing()) =>
  renderToStaticMarkup(
    <PackageHeader
      kind="skill"
      displayName="gh"
      description={null}
      place={place}
      scopes={[VG]}
      action={null}
    />,
  );

describe("the package header's marks", () => {
  it("names the place the mark is about", () => {
    expect(header()).toContain("Customized in vg");
  });

  it("says nothing while the place is still being worked out", () => {
    expect(header(null)).not.toContain("Customized");
    expect(header(null)).not.toContain("Forked");
  });

  it("leaves the mark off where nothing was changed", () => {
    expect(
      header(standing({ state: "as-installed", change: null })),
    ).not.toContain("Customized");
  });

  it("names the place a fork belongs to", () => {
    expect(header(standing({ forked: true }))).toContain("Forked in vg");
  });

  it("tells two projects sharing a folder name apart", () => {
    const twin = { scope: "project" as const, root: "/clients/vg" };
    expect(
      renderToStaticMarkup(
        <PackageHeader
          kind="skill"
          displayName="gh"
          description={null}
          place={standing()}
          scopes={[VG, twin]}
          action={null}
        />,
      ),
    ).toContain("Customized in work/vg");
  });
});
