import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { AgentSummaryView, DashboardState } from "@/api/dashboard";
import { AppShell } from "@/components/shell/app-shell";
import { buildWorkspace } from "@/fixtures/workspace";
import { signedUsd } from "@/lib/format";
import { RESTRICTIONS } from "@/lib/restrictions";
import { dockStop, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ApiDashboard } from "./dashboard-api";

const PERSUASIVE = /guarantee|safer|safe bet|recommend|profit (estimate|target)|price target|expected (return|profit)|don.t miss|act now|hurry|opportunity/i;
const HASH = `sha256:${"0f".repeat(32)}` as const;
const SERVED = "2026-09-28T18:05:20.000000000Z";

function summary(over: Partial<AgentSummaryView>): AgentSummaryView {
  return {
    agent_id: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC",
    label: "Agent 7",
    environment: "paper",
    pnl_label: "Simulated",
    mode: "normal",
    restrictions: [],
    startup: "ready",
    positions_count: 1,
    pnl_total: "123.45",
    pnl_today: "73.45",
    marks: { as_of: "2026-09-28T18:05:18.000000000Z", stale: false, age: "under 5 s" },
    ...over,
  };
}

function ready(agents: AgentSummaryView[]): DashboardState {
  const health = { state: "ok" as const, as_of: "2026-09-28T18:05:19.000000000Z" };
  return {
    status: "ready",
    served_at: SERVED,
    agents,
    open_approvals: 0,
    unread_alerts: 0,
    recent_decisions: [],
    health: { market_data: health, broker: health, deployment: health, relay: health },
    as_of: { dashboard: [{ stream_id: "ctl:ws_1", seq: 1, hash: HASH, recorded_at: SERVED }], health: [] },
  };
}

function show(state: DashboardState) {
  setPathname("/");
  return renderWithRuntime(
    <AppShell>
      <ApiDashboard state={state} />
    </AppShell>,
  );
}

const main = () => screen.getByRole("main");

beforeEach(() => setPathname("/"));

describe("D1 on the workspace API (E11-1, E11-9)", () => {
  it.skip("pending E11-1: shows each agent the summary names, with its mode and exact P&L", () => {
    show(ready([summary({}), summary({ agent_id: "agt_01JB3K9P2H6SD4F8G1E3W7XYZB", label: "Agent 8", mode: "paused", pnl_total: "-67.89" })]));
    const agents = within(main()).getByRole("region", { name: "Agents" });
    expect(agents).toHaveTextContent("Agent 7");
    expect(agents).toHaveTextContent("Agent 8");
    expect(agents.querySelector("[data-mode=paused]")).not.toBeNull();
    expect(agents).toHaveTextContent(signedUsd("123.45"));
    expect(agents).toHaveTextContent(signedUsd("-67.89"));
  });

  it.skip("pending E11-1: never shows the fixtures' agents in their place", () => {
    show(ready([summary({})]));
    for (const agent of buildWorkspace("normal").agents) expect(main()).not.toHaveTextContent(agent.label);
  });

  it.skip("pending E11-1: paper P&L carries the simulated note once; live carries none", () => {
    show(ready([summary({}), summary({ agent_id: "agt_2", label: "Agent 8" })]));
    expect(main().querySelectorAll("[data-slot=paper-note]")).toHaveLength(1);
  });

  it.skip("pending E11-1: an all-live dashboard has no simulated note", () => {
    show(ready([summary({ environment: "live", pnl_label: null })]));
    expect(main().querySelector("[data-slot=paper-note]")).toBeNull();
    expect(main()).not.toHaveTextContent(/simulated/i);
  });

  it.skip("pending E11-1: stale marks are shown stale with their age, never as current", () => {
    show(ready([summary({ marks: { as_of: "2026-09-28T18:02:11.000000000Z", stale: true, age: "3 min" } })]));
    const agents = within(main()).getByRole("region", { name: "Agents" });
    expect(agents.querySelector("[data-stale=true]")).not.toBeNull();
    expect(agents).toHaveTextContent("Stale");
    expect(agents).toHaveTextContent("3 min");
  });

  it.skip("pending E11-1: each restriction is named in the explainer's words", () => {
    show(ready([summary({ mode: "exits_only", restrictions: [{ code: "drawdown_exits_only", since: "2026-09-28T17:41:03.000000000Z" }] })]));
    expect(within(main()).getByRole("region", { name: "Agents" })).toHaveTextContent(RESTRICTIONS.drawdown_exits_only.label);
  });

  it.skip("pending E11-1: no agents says so plainly", () => {
    show(ready([]));
    expect(within(main()).getByRole("heading", { name: "No agents yet" })).toBeInTheDocument();
  });

  it.skip("pending E11-1: carries no persuasive or advisory wording", () => {
    show(ready([summary({}), summary({ agent_id: "agt_2", label: "Agent 8", pnl_total: "9000" })]));
    expect(main()).not.toHaveTextContent(PERSUASIVE);
  });
});

describe("G1 and G4 on the workspace API: unreachable shows nothing cached, and Stop stays", () => {
  const unreachable: DashboardState = { status: "unreachable", workspace: { ...buildWorkspace("unreachable") } };

  it.skip("pending E11-1: unreachable shows the notice and no agent data, numbers, or decisions", () => {
    show(unreachable);
    expect(main().querySelector("[data-slot=unreachable-notice]")).not.toBeNull();
    expect(main()).not.toHaveTextContent(/\$\d/);
    expect(main().querySelector("[data-slot=paper-note]")).toBeNull();
    for (const agent of buildWorkspace("normal").agents) expect(main()).not.toHaveTextContent(agent.label);
  });

  it.skip("pending E11-1: Stop stays reachable when the dashboard is unreachable", () => {
    show(unreachable);
    expect(isDisabled(dockStop())).toBe(false);
  });

  it.skip("pending E11-1: Stop stays reachable on a ready dashboard", () => {
    show(ready([summary({})]));
    expect(isDisabled(dockStop())).toBe(false);
    expect(screen.getAllByRole("banner")[0]).toHaveTextContent("PAPER");
  });
});
