import type { ReactNode } from "react";
import * as killSwitch from "@/app/(app)/agents/[agentId]/kill-switch/page";
import * as agent from "@/app/(app)/agents/[agentId]/page";
import * as release from "@/app/(app)/agents/[agentId]/release/page";
import * as approval from "@/app/(app)/approvals/[approvalId]/page";
import * as closeAll from "@/app/(app)/connections/[connectionId]/close-all/page";
import * as connection from "@/app/(app)/connections/[connectionId]/page";
import * as stopAll from "@/app/(app)/connections/[connectionId]/stop-all/page";
import * as design from "@/app/(app)/design/page.dev";
import * as setup from "@/app/(app)/new/page";
import * as home from "@/app/(app)/page";
import * as portfolio from "@/app/(app)/portfolio/page";
import * as settings from "@/app/(app)/settings/page";
import { recordHref } from "@/components/stop/commands";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { SCREENS, agentHref } from "@/lib/screens";

/** What Next would render for a path, resolved the same way the app directory does. */
export async function pageFor(path: string): Promise<ReactNode> {
  const parts = path.split("/").filter(Boolean);
  const [head, second, ...rest] = parts;
  const params = <T,>(p: T) => ({ params: Promise.resolve(p) });
  switch (head) {
    case undefined:
      return <home.default />;
    case "new":
      if (second) throw new Error(`no page for ${path}`);
      return <setup.default />;
    case "portfolio":
      if (second) throw new Error(`no page for ${path}`);
      return <portfolio.default />;
    case "agents":
      if (!second) throw new Error(`no page for ${path}`);
      if (rest.length === 0) return agent.default(params({ agentId: second }));
      if (rest.length === 1 && rest[0] === "kill-switch") return killSwitch.default(params({ agentId: second }));
      if (rest.length === 1 && rest[0] === "release") return release.default(params({ agentId: second }));
      throw new Error(`no page for ${path}`);
    case "approvals":
      if (!second || rest.length > 0) throw new Error(`no page for ${path}`);
      return approval.default(params({ approvalId: second }));
    case "connections":
      if (second && rest.length === 0) return connection.default(params({ connectionId: second }));
      if (second && rest.length === 1 && rest[0] === "stop-all") return stopAll.default(params({ connectionId: second }));
      if (second && rest.length === 1 && rest[0] === "close-all") return closeAll.default(params({ connectionId: second }));
      throw new Error(`no page for ${path}`);
    case "settings":
      if (second) throw new Error(`no page for ${path}`);
      return <settings.default />;
    case "design":
      if (!second) return <design.default />;
      throw new Error(`no page for ${path}`);
    default:
      throw new Error(`no page for ${path}`);
  }
}

/** Every route a scenario can reach: each screen, every agent's thread, kill switch and release, the connection's record screens, and every approval request. */
export function pathsFor(scenario: Scenario): string[] {
  const ws = buildWorkspace(scenario);
  const perAgent = ws.agents.flatMap((a) => [agentHref(a.agent_id), recordHref("kill", a.agent_id), recordHref("release", a.agent_id)]);
  const connection = ws.connection.connection_id;
  return [
    ...new Set([
      ...SCREENS.map((s) => s.href),
      "/design",
      recordHref("stop_all", connection),
      recordHref("close_all", connection),
      ...ws.approvals.map((a) => `/approvals/${a.approval_id}`),
      ...perAgent,
    ]),
  ];
}
