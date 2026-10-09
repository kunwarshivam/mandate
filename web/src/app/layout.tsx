import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import "@fontsource-variable/public-sans";
import "@fontsource-variable/pixelify-sans";
import "./globals.css";
import { NIGHT, OFF_WHITE } from "@/lib/brand-palette";
import { getColourBlind, getThemePref } from "@/lib/get-workspace";
import { THEME_SCRIPT } from "@/lib/theme";

const SHARE_IMAGE = { url: "/og-image.png", width: 1200, height: 630, alt: "Owlhead" };

/**
 * Titles name the screen, never an agent, ticker, or amount (brief §5); the product is Owlhead (DEC-201).
 * The share card stays "Owlhead" on every route, so a sensitive page's link preview names nothing either.
 * Icons and the share image come from `npm run brand` (DEC-203).
 */
export const metadata: Metadata = {
  metadataBase: new URL("https://owlhead.ai"),
  applicationName: "Owlhead",
  title: { template: "%s · Owlhead", default: "Owlhead" },
  robots: { index: false, follow: false },
  referrer: "no-referrer",
  icons: {
    icon: [
      { url: "/favicon.ico", sizes: "16x16 32x32 48x48" },
      { url: "/favicon.svg", type: "image/svg+xml" },
      { url: "/favicon-16.png", sizes: "16x16", type: "image/png" },
      { url: "/favicon-32.png", sizes: "32x32", type: "image/png" },
      { url: "/favicon-48.png", sizes: "48x48", type: "image/png" },
    ],
    apple: [{ url: "/apple-touch-icon.png", sizes: "180x180", type: "image/png" }],
  },
  manifest: "/site.webmanifest",
  openGraph: { type: "website", siteName: "Owlhead", title: "Owlhead", url: "/", images: [SHARE_IMAGE] },
  twitter: { card: "summary_large_image", title: "Owlhead", images: [SHARE_IMAGE] },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: OFF_WHITE },
    { media: "(prefers-color-scheme: dark)", color: NIGHT },
  ],
  colorScheme: "light dark",
};

/**
 * The document alone. The app's frame is `(app)/layout.tsx`; the public pages' is `(site)/layout.tsx`.
 */
export default async function RootLayout({ children }: { children: ReactNode }) {
  const colourBlind = await getColourBlind();
  const themePref = await getThemePref();
  // "system" is resolved in the head script before paint, so the server's guess can differ.
  const mode = themePref === "dark" ? "dark" : "light";
  return (
    <html
      lang="en"
      data-mode={mode}
      data-theme-pref={themePref}
      className={mode === "dark" ? "dark" : undefined}
      style={{ colorScheme: mode }}
      data-theme="owlhead"
      data-cvd={colourBlind ? "on" : undefined}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body>
        <div className="isolate">{children}</div>
      </body>
    </html>
  );
}
