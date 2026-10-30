"use client";

import { ApiConnections } from "@/components/api-screens/connections";
import { ApiPasskeys } from "@/components/api-screens/passkeys";
import { BySource } from "@/components/api-screens/api-screens";
import { PasskeysSection } from "@/components/auth/passkeys-section";
import { PushSection } from "@/components/notifications/push-section";
import { WorkspaceGate } from "@/components/screens/common";
import { useSession } from "@/lib/session";
import { ChatSettingsSection } from "./chat-settings";

/** Settings, one page (DEC-2293): connections, each agent's chat, notifications, and your passkeys. Sign out is in the header's menu. */
export function SettingsScreen() {
  const session = useSession();
  return (
    <div className="mx-auto grid w-full max-w-2xl gap-(--section-gap)" data-slot="settings">
      <h1 className="text-h1">Settings</h1>
      <section aria-labelledby="settings-connections" className="grid gap-3">
        <h2 id="settings-connections" className="text-h2">
          Connections
        </h2>
        <BySource
          fixtures={<p className="text-muted-foreground">This is recorded demo data, so no broker account is connected.</p>}
          api={
            <WorkspaceGate api>
              <ApiConnections />
            </WorkspaceGate>
          }
        />
      </section>
      <ChatSettingsSection />
      <PushSection />
      <section aria-labelledby="settings-profile" className="grid gap-3">
        <h2 id="settings-profile" className="text-h2">
          Profile
        </h2>
        <BySource
          fixtures={session ? <PasskeysSection /> : <p className="text-muted-foreground">Sign in to manage your passkeys.</p>}
          api={
            <WorkspaceGate api>
              <ApiPasskeys />
            </WorkspaceGate>
          }
        />
      </section>
    </div>
  );
}
