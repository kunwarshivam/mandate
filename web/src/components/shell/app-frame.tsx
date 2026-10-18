import type { ReactNode } from "react";
import { ViewTransition } from "react";
import { ScenarioSwitcher } from "@/components/dev/scenario-switcher";
import { ApiProviders, Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { RoleProvider } from "@/lib/roles";
import { buildWorkspace } from "@/fixtures/workspace";
import { getDataSource } from "@/lib/data-source";
import { getColourBlind, getScenario } from "@/lib/get-workspace";
import { scenarioSwitcherShown } from "@/lib/scenario";
import { SessionProvider } from "@/lib/session";
import { readStubSignIn } from "@/lib/stub-sign-in";
import { signedInUser } from "@/lib/supabase/server";

/**
 * The app's frame over the workspace: the shell with Stop and the dock. The `(app)` layout wraps every
 * screen in it, and the root not-found page wraps an unknown address in it, so Stop stays at the end of
 * the dock and the tab bar there too (brief P1). The server's configuration picks what the workspace is
 * (DEC-975): the fixtures, with the scenario switch where it is on, or a deployment's workspace API, read
 * by the browser with its own session. A misconfigured API is shown unreachable, never as fixtures.
 */
export async function AppFrame({ children }: { children: ReactNode }) {
  const source = await getDataSource();
  const colourBlind = await getColourBlind();
  const user = await signedInUser();
  if (source.kind !== "fixtures") {
    return (
      <SessionProvider session={user}>
        <RoleProvider>
          <ApiProviders
            source={source.kind === "api" ? { origin: source.origin, workspaceId: source.workspaceId } : null}
            stubSignIn={readStubSignIn(process.env, process.env.NODE_ENV) !== null}
          >
            <AppShell>
              <ViewTransition>{children}</ViewTransition>
            </AppShell>
          </ApiProviders>
        </RoleProvider>
      </SessionProvider>
    );
  }
  const scenario = await getScenario();
  const workspace = buildWorkspace(scenario);
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
