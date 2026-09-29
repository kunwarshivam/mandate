import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { LANDING_DESCRIPTION, landingMetadata } from "./metadata";

const layout = readFileSync(resolve(__dirname, "../../app/layout.tsx"), "utf8");

describe("the landing page's metadata", () => {
  it("is titled Owlhead alone, without the app's title template", () => {
    expect(landingMetadata.title).toEqual({ absolute: "Owlhead" });
  });

  it("is the page search engines may index and follow", () => {
    expect(landingMetadata.robots).toEqual({ index: true, follow: true });
  });

  it("describes the product plainly, with no performance and no promise", () => {
    expect(landingMetadata.description).toBe(LANDING_DESCRIPTION);
    expect(LANDING_DESCRIPTION.length).toBeLessThanOrEqual(160);
    expect(LANDING_DESCRIPTION).toMatch(/paper/i);
    expect(LANDING_DESCRIPTION).not.toMatch(/\d\s?%|profit|guarantee|\bearn|beat the market|returns?\b|[—–!]/i);
    expect(LANDING_DESCRIPTION).not.toMatch(/\bMandate\b/);
  });

  it("reuses the existing share image, and the share card carries no tagline", () => {
    expect(layout).toContain('url: "/og-image.png", width: 1200, height: 630, alt: "Owlhead"');
    const og = landingMetadata.openGraph as Record<string, unknown>;
    expect(og).toMatchObject({ type: "website", siteName: "Owlhead", title: "Owlhead", url: "/" });
    expect(og.images).toEqual([{ url: "/og-image.png", width: 1200, height: 630, alt: "Owlhead" }]);
    expect(og).not.toHaveProperty("description");
    const twitter = landingMetadata.twitter as Record<string, unknown>;
    expect(twitter).toMatchObject({ card: "summary_large_image", title: "Owlhead", images: ["/og-image.png"] });
    expect(twitter).not.toHaveProperty("description");
  });
});
