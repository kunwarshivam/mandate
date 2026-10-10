import type { ReactNode } from "react";
import { MessagesScreen } from "@/components/messages/messages-screen";
import { NewAgentFlow } from "@/components/new-agent/new-agent-flow";
import { AlertsScreen, RegistryScreen, SectionIndexScreen } from "@/components/screens/account-screens";
import { AgentDetailScreen, AgentSectionScreen } from "@/components/screens/agent-detail";
import { AgentRecordScreen } from "@/components/screens/agent-records";
import { AgentsListScreen } from "@/components/screens/agents-list";
import { ApprovalRequestScreen } from "@/components/screens/approval-request";
import { ApprovalsInboxScreen } from "@/components/screens/approvals-inbox";
import { WorkspaceGate } from "@/components/screens/common";
import { DashboardScreen } from "@/components/screens/dashboard";
import { PositionsScreen } from "@/components/screens/positions-screen";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { AGENT_ID, APPROVAL_ID, ASSET_ID, CONNECTION_ID, EVENT_ID, ORDER_ID } from "@/lib/ids";
import { type AgentRecord, SECTION_INDEX, findAgentRecord, findAgentSection, findScreen } from "@/lib/screens";

const RECORD_ID: Record<AgentRecord["kind"], RegExp> = {
  position: ASSET_ID,
  "close-position": ASSET_ID,
  order: ORDER_ID,
  decision: EVENT_ID,
};

/** The path alone, without its query or fragment, and without a trailing slash. */
export function demoPath(href: string): string {
  const path = href.split(/[?#]/)[0] || "/";
  return path.length > 1 ? path.replace(/\/+$/, "") : path;
}

/** Inside Messages, where `(app)/messages/layout.tsx` holds what the owner asked while they stay (DEC-479). */
export function inMessages(href: string): boolean {
  const path = demoPath(href);
  return path === "/messages" || path.startsWith("/messages/");
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
      return <DashboardScreen />;
    case "agents": {
      if (second === undefined) return <AgentsListScreen />;
      if (second === "new" && rest.length === 0)
        return (
          <WorkspaceGate>
            <NewAgentFlow />
          </WorkspaceGate>
        );
      if (!AGENT_ID.test(second)) return null;
      if (rest.length === 0) return <AgentDetailScreen agentId={second} />;
      if (rest.length === 1 && rest[0] === "kill-switch") return <StopRecordScreen kind="kill" targetId={second} />;
      if (rest.length === 1 && rest[0] === "release") return <StopRecordScreen kind="release" targetId={second} />;
      const record = findAgentRecord(rest);
      if (record && RECORD_ID[record.kind].test(record.id)) return <AgentRecordScreen agentId={second} record={record} />;
      const section = findAgentSection(rest);
      return section ? <AgentSectionScreen agentId={second} section={section.key} /> : null;
    }
    case "approvals":
      if (second === undefined) return <ApprovalsInboxScreen />;
      return rest.length === 0 && APPROVAL_ID.test(second) ? <ApprovalRequestScreen approvalId={second} /> : null;
    case "positions":
      return second === undefined ? <PositionsScreen /> : null;
    case "alerts":
      return second === undefined ? <AlertsScreen /> : null;
    case "messages":
      if (second === undefined) return <MessagesScreen />;
      if (!AGENT_ID.test(second)) return null;
      if (rest.length === 0) return <MessagesScreen agentId={second} view="chat" />;
      return rest.length === 1 && rest[0] === "desk" ? <MessagesScreen agentId={second} view="desk" /> : null;
    case "audit":
      if (second === undefined) return <SectionIndexScreen title={SECTION_INDEX.audit.label} purpose={SECTION_INDEX.audit.purpose} prefix="/audit" />;
      return registry(path, rest);
    case "settings":
      if (second === undefined) return <SectionIndexScreen title={SECTION_INDEX.workspace.label} purpose={SECTION_INDEX.workspace.purpose} prefix="/settings" />;
      return registry(path, rest);
    case "connections": {
      if (second === undefined) return registry(path, []);
      if (!CONNECTION_ID.test(second) || rest.length !== 1) return null;
      if (rest[0] === "stop-all") return <StopRecordScreen kind="stop_all" targetId={second} />;
      return rest[0] === "close-all" ? <StopRecordScreen kind="close_all" targetId={second} /> : null;
    }
    default:
      return null;
  }
}

function registry(path: string, rest: string[]): ReactNode | null {
  const screen = rest.length === 0 ? findScreen(path) : undefined;
  return screen ? <RegistryScreen screen={screen} /> : null;
}
