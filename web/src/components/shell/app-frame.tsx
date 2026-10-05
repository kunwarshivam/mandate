import type { ReactNode } from "react";
import { ViewTransition } from "react";
import { ScenarioSwitcher } from "@/components/dev/scenario-switcher";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { RoleProvider } from "@/lib/roles";
import { buildWorkspace } from "@/fixtures/workspace";
import { getColourBlind, getScenario } from "@/lib/get-workspace";
import { scenarioSwitcherShown } from "@/lib/scenario";
import { SessionProvider } from "@/lib/session";
import { signedInUser } from "@/lib/supabase/server";

/**
 * The app's frame over the fixture workspace: the shell with Stop and the dock. The `(app)`
 * layout wraps every screen in it, and the root not-found page wraps an unknown address in it, so
 * Stop stays at the end of the dock and the tab bar there too (brief P1).
 */
export async function AppFrame({ children }: { children: ReactNode }) {
  const scenario = await getScenario();
  const workspace = buildWorkspace(scenario);
  const colourBlind = await getColourBlind();
  const user = await signedInUser();
  return (
    <SessionProvider session={user}>
      <RoleProvider>
        <Providers key={scenario} workspace={workspace}>
          <AppShell>
            <ViewTransition>{children}</ViewTransition>
          </AppShell>
          {scenarioSwitcherShown ? <ScenarioSwitcher scenario={scenario} colourBlind={colourBlind} /> : null}
        </Providers>
      </RoleProvider>
    </SessionProvider>
  );
}
