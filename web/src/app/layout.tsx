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

/** Titles name the screen, never an agent, ticker, or amount (brief §5); the product is Owlhead (DEC-201). */
export const metadata: Metadata = {
  metadataBase: new URL("https://owlhead.ai"),
  applicationName: "Owlhead",
  title: { template: "%s · Owlhead", default: "Owlhead" },
  description: "Autonomous trading agents under a mandate you set. Paper trading only.",
  robots: { index: false, follow: false },
  referrer: "no-referrer",
};

export const viewport: Viewport = {
  themeColor: "#f4f7fa",
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
