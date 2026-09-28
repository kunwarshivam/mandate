import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { ViewTransition } from "react";
import "@fontsource-variable/bricolage-grotesque";
import "@fontsource-variable/hanken-grotesk";
import "@fontsource-variable/jetbrains-mono";
import "./globals.css";
import { ScenarioSwitcher } from "@/components/dev/scenario-switcher";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { themeScript } from "@/components/shell/theme-toggle";
import { buildWorkspace } from "@/fixtures/workspace";
import { getScenario } from "@/lib/get-workspace";
import { scenariosEnabled } from "@/lib/scenario";

export const metadata: Metadata = {
  title: { template: "%s · Mandate", default: "Mandate" },
  description: "Autonomous trading agents under a mandate you set. Paper trading only.",
  robots: { index: false, follow: false },
  referrer: "no-referrer",
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f8f9fc" },
    { media: "(prefers-color-scheme: dark)", color: "#0e1222" },
  ],
};

export default async function RootLayout({ children }: { children: ReactNode }) {
  const scenario = await getScenario();
  const workspace = buildWorkspace(scenario);
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>
        <Providers key={scenario} workspace={workspace}>
          <AppShell>
            <ViewTransition>{children}</ViewTransition>
          </AppShell>
          {scenariosEnabled ? <ScenarioSwitcher scenario={scenario} /> : null}
        </Providers>
      </body>
    </html>
  );
}
