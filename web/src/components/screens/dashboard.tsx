"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { ArrowRight } from "@phosphor-icons/react";
import { LinkButton } from "@cloudflare/kumo/components/button";
import { Deadline } from "@/components/approvals/deadline";
import { AccountEquityChart } from "@/components/charts/equity-chart";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { ModeBadge } from "@/components/domain/mode";
import { SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import type { Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { price, quantity, usd } from "@/lib/format";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { RESTRICTIONS } from "@/lib/restrictions";
import { useCan } from "@/lib/roles";
import { decisionHref, positionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { AgentCard } from "./agent-card";
import { EmptyBoard, Panel, Section, WorkspaceGate } from "./common";

const HEALTH_WORD = { market_data: "Market data", broker: "Broker", deployment: "Deployment", relay: "Push relay" } as const;

function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

/** Everything the Alerts screen lists, in one line: degraded feeds first, then agent conditions. */
function alertLines(ws: Workspace): string[] {
  const feeds = (Object.keys(HEALTH_WORD) as Array<keyof typeof HEALTH_WORD>)
    .filter((k) => ws.health[k].state !== "ok")
    .map((k) => `${HEALTH_WORD[k]} ${ws.health[k].state === "down" ? "down" : "stale"}`);
  const agents = ws.agents.flatMap((a) => a.restrictions.map((r) => `${a.label}: ${lowerFirst(RESTRICTIONS[r.code].label)}${r.symbol ? ` (${r.symbol})` : ""}`));
  return [...feeds, ...agents];
}

function AlertsSummary({ ws }: { ws: Workspace }) {
  const lines = alertLines(ws);
  return (
    <Link
      href="/alerts"
      data-slot="alerts-summary"
      data-count={lines.length}
      className={cn(
        "press flex min-h-11 flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset sm:px-5",
        lines.length === 0 ? "bg-card hover:bg-muted" : "border-t border-foreground bg-muted hover:bg-card",
      )}
    >
      <span className="font-semibold">{lines.length === 0 ? "No alerts" : lines.length === 1 ? "1 alert" : `${lines.length} alerts`}</span>
      <span className="min-w-0 flex-1 text-sm text-muted-foreground">
        {lines.length === 0 ? "Market data, broker, deployment and push relay are current, and no agent is restricted." : `${lines.join("; ")}.`}
      </span>
      <ArrowRight className="size-4 shrink-0" aria-hidden />
    </Link>
  );
}

function Dashboard() {
  const { ws, now } = useRuntime();
  const canAudit = useCan("audit.view");
  if (ws.agents.length === 0) return <EmptyBoard />;
  const open = ws.approvals
    .map((a) => approvalAt(a, now))
    .filter((a) => a.status === "delivered")
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const marketStale = ws.health.market_data.state !== "ok";
  const running = ws.agents.filter((a) => a.mode !== "stopped").length;
  const positions = ws.agents.flatMap((a) => a.positions.map((p) => ({ agent: a, p })));

  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <div className="grid grid-cols-1 gap-(--seam) lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <header data-slot="account-board" className="reveal grid content-start gap-4 bg-lapis px-4 pt-4 pb-3 text-lapis-foreground sm:px-5 sm:pt-5">
          <div className="flex flex-wrap items-end justify-between gap-x-4 gap-y-1">
            <h1 className="text-h1">Dashboard</h1>
            <p className="text-lapis-muted">
              {ws.connection.broker}, <span className="font-mono tabular">{usd(ws.connection.account_equity)}</span> equity
            </p>
          </div>
          <table className="w-full border-collapse">
            <caption className="sr-only">Agents and their modes</caption>
            <thead className="sr-only">
              <tr>
                <th scope="col">Agent</th>
                <th scope="col">Mandate</th>
                <th scope="col">Mode</th>
              </tr>
            </thead>
            <tbody>
              {ws.agents.map((a) => (
                <tr key={a.agent_id} className="border-t border-lapis-muted/40">
                  <th scope="row" className="py-2 pr-3 text-left text-xl leading-none font-semibold">
                    {a.label}
                  </th>
                  <td className="py-2 pr-3 text-sm text-lapis-muted">{a.mandate.name}</td>
                  <td className="py-2 text-right">
                    <ModeBadge mode={a.mode} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-sm text-lapis-muted">
            {running} of {ws.agents.length} running
          </p>
        </header>

        <section aria-labelledby="waiting-title" data-slot="waiting" className="reveal grid content-start gap-3 bg-card px-4 py-4 sm:px-5 sm:py-5" style={{ "--i": 1 } as CSSProperties}>
          <h2 id="waiting-title" className="text-h2">
            {open.length === 0 ? "Nothing waiting" : open.length === 1 ? "1 request waiting" : `${open.length} requests waiting`}
          </h2>
          {open.length === 0 ? (
            <p className="text-muted-foreground">Requests for your approval appear here, with their deadline.</p>
          ) : (
            <ul className="grid gap-3">
              {open.map((a) => {
                const agent = findAgent(ws, a.agent_id);
                return (
                  <li key={a.approval_id} className="grid gap-2 border-t border-foreground pt-2.5">
                    <p className="font-semibold">
                      {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of{" "}
                      <span className="font-mono tabular">{price(a.bound.limit)}</span>
                    </p>
                    <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                    <LinkButton href={`/approvals/${a.approval_id}`} variant="primary" size="lg" className="h-11 w-fit">
                      Open request <ArrowRight aria-hidden className="size-4" />
                    </LinkButton>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>

      <div className="grid grid-cols-1 gap-(--seam)">
        <AlertsSummary ws={ws} />
        <AccountEquityChart />
      </div>

      <Section title="Agents">
        <ul className="grid gap-(--block-gap)">
          {ws.agents.map((agent, i) => (
            <li key={agent.agent_id} className="grid">
              <AgentCard agent={agent} now={now} marketStale={marketStale} index={i + 2} />
            </li>
          ))}
        </ul>
      </Section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <Section
          title="Recent gate decisions"
          action={
            canAudit ? (
              <Link href="/audit/decisions" className="text-sm font-semibold text-lapis underline underline-offset-4 hover:decoration-2">
                All decisions
              </Link>
            ) : undefined
          }
        >
          <Panel className="py-0.5 sm:py-0.5">
            {ws.decisions.length === 0 ? (
              <p className="py-2.5 text-sm text-muted-foreground">No decisions yet.</p>
            ) : (
              <ol>
                {ws.decisions.slice(0, 6).map((d) => (
                  <GateDecisionRow key={d.event_id} decision={d} agent={findAgent(ws, d.agent_id)} showAgent href={decisionHref(d.agent_id, d.event_id)} />
                ))}
              </ol>
            )}
          </Panel>
        </Section>

        <Section
          title="Positions"
          action={
            <Link href="/positions" className="text-sm font-semibold text-lapis underline underline-offset-4 hover:decoration-2">
              All positions
            </Link>
          }
        >
          <Panel>
            {positions.length === 0 ? (
              <p className="text-sm text-muted-foreground">No agent holds a position.</p>
            ) : (
              <table className="w-full text-sm">
                <caption className="sr-only">Positions across agents</caption>
                <thead>
                  <tr className="border-b border-foreground text-left">
                    <th scope="col" className="pb-1.5 field-label">Holding</th>
                    <th scope="col" className="pb-1.5 text-right field-label">Value</th>
                    <th scope="col" className="pb-1.5 text-right field-label">Unrealized</th>
                  </tr>
                </thead>
                <tbody>
                  {positions.map(({ agent, p }) => (
                    <tr key={`${agent.agent_id}-${p.instrument.asset_id}`} className="relative border-b last:border-b-0 hover:bg-muted">
                      <th scope="row" className="py-2 text-left font-normal">
                        <Link href={positionHref(agent.agent_id, p.instrument.asset_id)} className={cn("font-semibold underline decoration-2 underline-offset-4", STRETCHED_LINK)}>
                          <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                        </Link>
                        <span className="block text-caption text-muted-foreground">{agent.label}</span>
                      </th>
                      <td className="py-2 text-right align-top font-mono tabular">{usd(p.market_value)}</td>
                      <td className="py-2 text-right align-top">
                        <SignedMoney value={p.unrealized_pnl} showWord={false} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {positions.length > 0 ? <Placeholder name="performance" className="mt-2" /> : null}
            {ws.external_positions.length > 0 ? (
              <p className="mt-3 border-t pt-2.5 text-caption text-muted-foreground">
                Also on the account, not managed by any agent: {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
              </p>
            ) : null}
          </Panel>
        </Section>
      </div>
    </div>
  );
}

export function DashboardScreen() {
  return (
    <WorkspaceGate>
      <Dashboard />
    </WorkspaceGate>
  );
}
