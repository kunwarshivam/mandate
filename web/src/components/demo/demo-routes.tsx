import { type ReactNode, Suspense } from "react";
import { AgentScreen } from "@/components/agent-chat/agent-screen";
import { HomeScreen } from "@/components/agent-chat/home";
import { SettingsScreen } from "@/components/agent-chat/settings-screen";
import { SetupScreen } from "@/components/agent-chat/setup-screen";
import { ApprovalRequestScreen } from "@/components/screens/approval-request";
import { WorkspaceGate } from "@/components/screens/common";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { AGENT_ID, APPROVAL_ID, CONNECTION_ID } from "@/lib/ids";

/** The path alone, without its query or fragment, and without a trailing slash. */
export function demoPath(href: string): string {
  const path = href.split(/[?#]/)[0] || "/";
  return path.length > 1 ? path.replace(/\/+$/, "") : path;
}

/**
 * The app's screen for a path, as the `(app)` pages choose it, for the example workspace in the
 * landing page's browser (DEC-906); `null` for a path the app has no screen at. It mirrors those
 * pages' routing and validation, so the same address shows the same screen in both.
 */
export function demoScreen(href: string): ReactNode | null {
  const path = demoPath(href);
  const [first, second, ...rest] = path.split("/").slice(1);
  switch (first) {
    case "":
      return (
        <WorkspaceGate api>
          <HomeScreen />
        </WorkspaceGate>
      );
    case "new":
      return second === undefined ? (
        <Suspense>
          <WorkspaceGate api>
            <SetupScreen />
          </WorkspaceGate>
        </Suspense>
      ) : null;
    case "settings":
      return second === undefined ? <SettingsScreen /> : null;
    case "agents": {
      if (second === undefined || !AGENT_ID.test(second)) return null;
      if (rest.length === 0)
        return (
          <WorkspaceGate api>
            <AgentScreen agentId={second} />
          </WorkspaceGate>
        );
      if (rest.length === 1 && rest[0] === "kill-switch") return <StopRecordScreen kind="kill" targetId={second} />;
      if (rest.length === 1 && rest[0] === "release") return <StopRecordScreen kind="release" targetId={second} />;
      return null;
    }
    case "approvals":
      return second !== undefined && rest.length === 0 && APPROVAL_ID.test(second) ? <ApprovalRequestScreen approvalId={second} /> : null;
    case "connections": {
      if (second === undefined || !CONNECTION_ID.test(second) || rest.length !== 1) return null;
      if (rest[0] === "stop-all") return <StopRecordScreen kind="stop_all" targetId={second} />;
      return rest[0] === "close-all" ? <StopRecordScreen kind="close_all" targetId={second} /> : null;
    }
    default:
      return null;
  }
}
