// @vitest-environment node
import { existsSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { metadata, viewport } from "@/app/layout";
import { PUBLIC_DIR } from "../../../scripts/brand-assets.mjs";

function urls(value: unknown): string[] {
  if (typeof value === "string") return [value];
  if (value instanceof URL) return [value.pathname];
  if (Array.isArray(value)) return value.flatMap(urls);
  if (value && typeof value === "object") return "url" in value ? urls(value.url) : Object.values(value).flatMap(urls);
  return [];
}

describe("root layout metadata (DEC-203)", () => {
  it("points every icon, the manifest, and the share image at a committed file in public/", () => {
    const referenced = [...urls(metadata.icons), ...urls(metadata.manifest), ...urls(metadata.openGraph?.images), ...urls(metadata.twitter?.images)];
    expect(referenced).toEqual(expect.arrayContaining(["/favicon.svg", "/favicon.ico", "/apple-touch-icon.png", "/site.webmanifest", "/og-image.png"]));
    for (const url of referenced) expect(existsSync(path.join(PUBLIC_DIR, url)), url).toBe(true);
  });

  it("shares a generic Owlhead card at owlhead.ai, in navy", () => {
    expect(String(metadata.metadataBase)).toBe("https://owlhead.ai/");
    expect(metadata.openGraph).toMatchObject({ title: "Owlhead", images: [{ url: "/og-image.png", width: 1200, height: 630, alt: "Owlhead" }] });
    expect(metadata.twitter).toMatchObject({ card: "summary_large_image", title: "Owlhead", images: [{ url: "/og-image.png", alt: "Owlhead" }] });
    expect(viewport.themeColor).toBe("#183D73");
  });
});
