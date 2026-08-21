import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PackageHeader } from "./package-header";

// The badge is about the place the Customize tab has open, which the
// project chips change under it — so it names that place or it is not there
// at all. "Customized" on its own answers a question nobody asked.
const header = (props: Partial<Parameters<typeof PackageHeader>[0]> = {}) =>
  renderToStaticMarkup(
    <PackageHeader
      kind="skill"
      displayName="gh"
      description={null}
      forked={false}
      customized={true}
      place="vg"
      action={null}
      {...props}
    />,
  );

describe("the package header's marks", () => {
  it("names the place the mark is about", () => {
    expect(header()).toContain("Customized in vg");
  });

  it("says nothing while the place is still being worked out", () => {
    expect(header({ place: null })).not.toContain("Customized");
  });

  it("leaves the mark off where nothing was changed", () => {
    expect(header({ customized: false })).not.toContain("Customized");
  });

  it("names the place a fork belongs to", () => {
    expect(header({ forked: true })).toContain("Forked in vg");
  });
});
