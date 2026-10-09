import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as agentSection from "@/app/(app)/agents/[agentId]/[...section]/page";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { AppShell } from "@/components/shell/app-shell";
import type { Agent, ContentRef, GateDecision, Workspace } from "@/fixtures/types";
import { VERSION } from "@/fixtures/versions";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { interpret } from "@/lib/ask-record";
import { deskFor } from "@/lib/messages";
import { decisionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { DecisionTimeline } from "./decision-timeline";

/** Agent 1's order-size denial, decided under its first and only version, whose order limit is $1,000. */
const DENIAL = "01JBWPQ5E6EYCNDY0YP57RCYBV";

/** Agent 1's second version, built here: the owner raised the order limit from $1,000 to $2,500. */
const RAISED: ContentRef = "sha256:0000000000000000000000000000000000000000000000000000000000002500";
const RAISED_AT = "2026-09-28T13:50:00-04:00";

/** A version Agent 1 never had: no mandate document can be rebuilt for it. */
const UNKNOWN: ContentRef = "sha256:00000000000000000000000000000000000000000000000000000000000dead1";

/**
 * The text each version's rule reads, written out rather than built by the code under test, so the
 * test is its own oracle (DEC-511: outcomes, not looks).
 */
const OLD_LIMIT = "Orders are at most $1,000.00.";
const NEW_FIGURE = "$2,500.00";
const OLD_FIGURE = "$1,000.00";
const NO_FIGURE = "Orders are held to the order limit of the mandate version the gate decided under.";

/**
 * The normal workspace after the owner raised Agent 1's order limit, with the denial still bound to
 * the version it was decided under, or to `version` when given.
 */
function raised(version?: ContentRef): { ws: Workspace; btc: Agent; denial: GateDecision } {
  const ws = structuredClone(buildWorkspace("normal"));
  const btc = ws.agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
  expect(btc.mandate.risk.max_order_usd).toBe("1000");
  btc.mandate.risk.max_order_usd = "2500";
  btc.versions = [
    ...btc.versions,
    {
      mandate_version: RAISED,
      previous: VERSION.btc,
      confirmed_at: RAISED_AT,
      step_up: true,
      classification: "risk_increasing",
      changes: [{ path: "/risk/max_order_usd", from: "1000", to: "2500", classification: "risk_increasing" }],
      application: { result: "applied", at: RAISED_AT, approvals_canceled: 0 },
    },
  ];
  btc.mandate_version = RAISED;
  const denial = ws.decisions.find((d) => d.event_id === DENIAL)!;
  expect(denial).toMatchObject({ verdict: "deny", reason_code: "max_order_size", mandate_version: VERSION.btc });
  if (version) denial.mandate_version = version;
  return { ws, btc, denial };
}

/** What the owner reads on each surface that shows the recorded denial's rule. */
const SURFACES: Record<string, (ws: Workspace, btc: Agent, denial: GateDecision) => Promise<string>> = {
  "the agent's decisions list": async (ws, btc, denial) => {
    const view = renderWithRuntime(
      <ul>
        <GateDecisionRow decision={denial} agent={btc} />
      </ul>,
      "normal",
      { workspace: () => ws },
    );
    return view.container.querySelector("[data-slot=gate-rule]")?.textContent ?? "";
  },
  "Home's decision timeline": async (ws, _btc, denial) => {
    const view = renderWithRuntime(<DecisionTimeline ws={ws} decisions={[denial]} />, "normal", { workspace: () => ws });
    return view.container.querySelector("[data-slot=gate-rule]")?.textContent ?? "";
  },
  "the gate decision's order-size check": async (ws, btc) => {
    const path = decisionHref(btc.agent_id, DENIAL);
    setPathname(path);
    const page = await agentSection.default({ params: Promise.resolve({ agentId: btc.agent_id, section: ["decisions", DENIAL] }) });
    renderWithRuntime(<AppShell>{page}</AppShell>, "normal", { workspace: () => ws });
    return screen.getByRole("main").querySelector('[data-slot=gate-checks] [data-check="order-size"]')?.textContent ?? "";
  },
  "the answer to “why was it denied?”": async (ws) => {
    const reply = interpret("why was it denied?", { ws, now: ws.now, agentId: AGENT_IDS.btc });
    return reply.kind === "answer" ? reply.lines.join(" ") : "";
  },
  "the desk's risk gate step for a decision": async (ws) => {
    ws.approvals = [];
    return deskFor(ws, AGENT_IDS.btc, ws.now)?.steps.find((s) => s.role === "risk")?.lines.join(" ") ?? "";
  },
  "the desk's risk gate step for the request it sent": async (ws, _btc, denial) => {
    const request = ws.approvals.find((a) => a.agent_id === AGENT_IDS.btc)!;
    ws.approvals = [...ws.approvals.filter((a) => a.agent_id !== AGENT_IDS.btc), request];
    for (const d of ws.decisions) if (d.approval_id === request.approval_id) delete d.approval_id;
    denial.approval_id = request.approval_id;
    return deskFor(ws, AGENT_IDS.btc, ws.now)?.steps.find((s) => s.role === "risk")?.lines.join(" ") ?? "";
  },
};

beforeEach(() => setPathname("/"));

describe("a recorded decision's rule states the limit of the mandate version it was decided under", () => {
  it.each(Object.keys(SURFACES))("%s reads the old limit, not the agent's current one", async (surface) => {
    const { ws, btc, denial } = raised();
    const text = await SURFACES[surface](ws, btc, denial);
    expect(text).toContain(OLD_LIMIT);
    expect(text).not.toContain(NEW_FIGURE);
  });

  it.each(Object.keys(SURFACES))("%s states no figure when the decision's version cannot be rebuilt", async (surface) => {
    const { ws, btc, denial } = raised(UNKNOWN);
    const text = await SURFACES[surface](ws, btc, denial);
    expect(text).toContain(NO_FIGURE);
    expect(text).not.toContain(NEW_FIGURE);
    expect(text).not.toContain(OLD_FIGURE);
  });
});
