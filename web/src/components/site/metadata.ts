import type { Metadata } from "next";

/**
 * The landing page's metadata (DEC-212): the only indexed page. The title stands alone, without the
 * app's " · Owlhead" template. The description is for search results only; the share card keeps the
 * brand's no-tagline rule (web/PRODUCT.md) and shows the name and the existing image.
 */
export const LANDING_DESCRIPTION = "An AI agent trades inside dollar limits you set. Every order is checked against your mandate first, and you can stop it at any time. Paper trading only.";

const SHARE_IMAGE = { url: "/og-image.png", width: 1200, height: 630, alt: "Owlhead" };

export const landingMetadata: Metadata = {
  title: { absolute: "Owlhead" },
  description: LANDING_DESCRIPTION,
  robots: { index: true, follow: true },
  openGraph: { type: "website", siteName: "Owlhead", title: "Owlhead", url: "/", images: [SHARE_IMAGE] },
  twitter: { card: "summary_large_image", title: "Owlhead", images: [SHARE_IMAGE.url] },
};
