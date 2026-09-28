import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { ViewTransition } from "react";
import "@fontsource-variable/big-shoulders-display";
import "@fontsource-variable/atkinson-hyperlegible-next";
import "./globals.css";
import { ScenarioSwitcher } from "@/components/dev/scenario-switcher";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { RoleProvider } from "@/lib/roles";
import { buildWorkspace } from "@/fixtures/workspace";
import { getScenario } from "@/lib/get-workspace";
import { scenariosEnabled } from "@/lib/scenario";

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
  themeColor: "#F7FAFE",
  colorScheme: "light",
};

export default async function RootLayout({ children }: { children: ReactNode }) {
  const scenario = await getScenario();
  const workspace = buildWorkspace(scenario);
  return (
    <html lang="en" data-mode="light" data-theme="placard">
      <body>
        <div className="isolate">
          <RoleProvider>
            <Providers key={scenario} workspace={workspace}>
              <AppShell>
                <ViewTransition>{children}</ViewTransition>
              </AppShell>
              {scenariosEnabled ? <ScenarioSwitcher scenario={scenario} /> : null}
            </Providers>
          </RoleProvider>
        </div>
      </body>
    </html>
  );
}
