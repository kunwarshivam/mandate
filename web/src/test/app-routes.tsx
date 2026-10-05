import type { ReactNode } from "react";
import * as agentSection from "@/app/(app)/agents/[agentId]/[...section]/page";
import * as killSwitch from "@/app/(app)/agents/[agentId]/kill-switch/page";
import * as agent from "@/app/(app)/agents/[agentId]/page";
import * as release from "@/app/(app)/agents/[agentId]/release/page";
import * as agentsNew from "@/app/(app)/agents/new/page";
import * as agents from "@/app/(app)/agents/page";
import * as alerts from "@/app/(app)/alerts/page";
import * as approval from "@/app/(app)/approvals/[approvalId]/page";
import * as approvals from "@/app/(app)/approvals/page";
import * as auditScreen from "@/app/(app)/audit/[screen]/page";
import * as audit from "@/app/(app)/audit/page";
import * as closeAll from "@/app/(app)/connections/[connectionId]/close-all/page";
import * as stopAll from "@/app/(app)/connections/[connectionId]/stop-all/page";
import * as connections from "@/app/(app)/connections/page";
import * as designNewAgent from "@/app/(app)/design/new-agent/page";
import * as design from "@/app/(app)/design/page";
import * as dashboard from "@/app/(app)/page";
import * as positions from "@/app/(app)/positions/page";
import * as settingsScreen from "@/app/(app)/settings/[screen]/page";
import * as settings from "@/app/(app)/settings/page";
import { recordHref } from "@/components/stop/commands";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { allOrders } from "@/lib/orders";
import { AGENT_SECTIONS, SCREENS, SECTION_INDEX, agentHref, decisionHref, orderHref, positionHref } from "@/lib/screens";

/** What Next would render for a path, resolved the same way the app directory does. */
export async function pageFor(path: string): Promise<ReactNode> {
  const parts = path.split("/").filter(Boolean);
  const [head, second, ...rest] = parts;
  const params = <T,>(p: T) => ({ params: Promise.resolve(p) });
  switch (head) {
    case undefined:
      return <dashboard.default />;
    case "agents":
      if (!second) return <agents.default />;
      if (second === "new" && rest.length === 0) return <agentsNew.default />;
      if (rest.length === 0) return agent.default(params({ agentId: second }));
      if (rest.length === 1 && rest[0] === "kill-switch") return killSwitch.default(params({ agentId: second }));
      if (rest.length === 1 && rest[0] === "release") return release.default(params({ agentId: second }));
      return agentSection.default(params({ agentId: second, section: rest }));
    case "approvals":
      if (!second) return <approvals.default />;
      return approval.default(params({ approvalId: second }));
    case "alerts":
      return <alerts.default />;
    case "positions":
      if (second) throw new Error(`no page for ${path}`);
      return <positions.default />;
    case "connections":
      if (!second) return <connections.default />;
      if (rest.length === 1 && rest[0] === "stop-all") return stopAll.default(params({ connectionId: second }));
      if (rest.length === 1 && rest[0] === "close-all") return closeAll.default(params({ connectionId: second }));
      throw new Error(`no page for ${path}`);
    case "audit":
      if (!second) return <audit.default />;
      return auditScreen.default(params({ screen: second }));
    case "settings":
      if (!second) return <settings.default />;
      return settingsScreen.default(params({ screen: second }));
    case "design":
      if (!second) return <design.default />;
      if (second === "new-agent" && rest.length === 0) return <designNewAgent.default />;
      throw new Error(`no page for ${path}`);
    default:
      throw new Error(`no page for ${path}`);
  }
}

/**
 * Every route a scenario can reach: each screen, and for every agent each section, record screen,
 * position (and its close page), order and gate decision, and every approval request.
 */
export function pathsFor(scenario: Scenario): string[] {
  const ws = buildWorkspace(scenario);
  const perAgent = ws.agents.flatMap((a) => [
    ...AGENT_SECTIONS.map((s) => agentHref(a.agent_id, s.key)),
    recordHref("kill", a.agent_id),
    recordHref("release", a.agent_id),
    ...a.positions.flatMap((p) => [positionHref(a.agent_id, p.instrument.asset_id), `${positionHref(a.agent_id, p.instrument.asset_id)}/close`]),
    ...allOrders(a).map((o) => orderHref(a.agent_id, o.client_order_id)),
    ...ws.decisions.filter((d) => d.agent_id === a.agent_id).map((d) => decisionHref(a.agent_id, d.event_id)),
  ]);
  const connection = ws.connection.connection_id;
  return [
    ...new Set([
      ...SCREENS.map((s) => s.href),
      SECTION_INDEX.audit.href,
      SECTION_INDEX.workspace.href,
      "/design",
      "/design/new-agent",
      recordHref("stop_all", connection),
      recordHref("close_all", connection),
      ...ws.approvals.map((a) => `/approvals/${a.approval_id}`),
      ...perAgent,
    ]),
  ];
}
