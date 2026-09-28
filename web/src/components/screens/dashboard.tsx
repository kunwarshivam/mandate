"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { ArrowRight, CheckCircle, WarningCircle } from "@phosphor-icons/react";
import { Deadline } from "@/components/approvals/deadline";
import { AccountEquityChart } from "@/components/charts/equity-chart";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import type { Approval, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { price, quantity, usd } from "@/lib/format";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { RESTRICTIONS } from "@/lib/restrictions";
import { useCan } from "@/lib/roles";
import { decisionHref, positionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { AgentCard } from "./agent-card";
import { EmptyBoard, Section, SectionLink, WorkspaceGate } from "./common";

/** The rail beside the account chart shows this many requests; the rest are one link away. */
const WAITING_SHOWN = 3;

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
  const Icon = lines.length === 0 ? CheckCircle : WarningCircle;
  return (
    <Link
      href="/alerts"
      data-slot="alerts-summary"
      data-count={lines.length}
      className={cn(
        "press group -mx-3 grid min-h-11 grid-cols-[1.25rem_minmax(0,1fr)_1rem] items-start gap-x-3 rounded-xl px-3 py-3 outline-none hover:bg-background focus-visible:ring-2 focus-visible:ring-ring",
        lines.length > 0 && "bg-background hover:bg-muted",
      )}
    >
      <Icon aria-hidden weight={lines.length === 0 ? "regular" : "fill"} className={cn("mt-0.5 size-5", lines.length === 0 ? "text-muted-foreground" : "text-foreground")} />
      <span className="grid gap-0.5">
        <span className="font-medium">{lines.length === 0 ? "No alerts" : lines.length === 1 ? "1 alert" : `${lines.length} alerts`}</span>
        <span className="text-sm text-muted-foreground">
          {lines.length === 0 ? "Market data, broker, deployment and push relay are current, and no agent is restricted." : `${lines.join("; ")}.`}
        </span>
      </span>
      <ArrowRight aria-hidden className="mt-1 size-4 text-muted-foreground transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" />
    </Link>
  );
}

/**
 * Requests for your approval, soonest deadline first. Prominent by place and tint, calm in voice:
 * what the agent asks, and when it is skipped if you do nothing. No urgency colour, no countdown.
 */
function Waiting({ ws, open, now }: { ws: Workspace; open: Approval[]; now: string }) {
  const more = open.length - WAITING_SHOWN;
  return (
    <section aria-labelledby="waiting-title" data-slot="waiting" className="grid content-start gap-(--block-gap)">
      <h2 id="waiting-title" className="flex items-center gap-2.5 text-h2">
        {open.length === 0 ? "Nothing waiting" : "Waiting for you"}
        {open.length > 0 ? (
          <span className="inline-flex h-6 min-w-6 items-center justify-center rounded-full bg-lapis px-2 font-mono text-label text-lapis-foreground tabular">
            {open.length}
            <span className="sr-only">{open.length === 1 ? " request" : " requests"}</span>
          </span>
        ) : null}
      </h2>
      {open.length === 0 ? (
        <p className="text-sm text-muted-foreground">Requests for your approval appear here, with their deadline.</p>
      ) : (
        <ul className="grid gap-2">
          {open.slice(0, WAITING_SHOWN).map((a, i) => {
            const agent = findAgent(ws, a.agent_id);
            return (
              <li key={a.approval_id} className="reveal grid gap-3 rounded-2xl bg-lapis-soft px-4 py-4" style={{ "--i": i + 1 } as CSSProperties}>
                <p className="font-medium text-pretty">
                  {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of{" "}
                  <span className="font-mono tabular">{price(a.bound.limit)}</span>
                </p>
                <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                <Link
                  href={`/approvals/${a.approval_id}`}
                  className="press inline-flex h-11 w-fit items-center gap-2 rounded-full bg-lapis pr-4 pl-5 text-sm font-semibold text-lapis-foreground outline-none hover:bg-lapis-strong focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-lapis-soft"
                >
                  Open request <ArrowRight aria-hidden className="size-4" />
                </Link>
              </li>
            );
          })}
        </ul>
      )}
      {more > 0 ? (
        <SectionLink href="/approvals">
          {more === 1 ? "1 more request" : `${more} more requests`}
        </SectionLink>
      ) : null}
    </section>
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
  const positions = ws.agents.flatMap((a) => a.positions.map((p) => ({ agent: a, p })));

  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <h1 className="sr-only">Dashboard</h1>
      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-x-14">
        <div data-slot="account-board" data-layout="main" className="reveal grid min-w-0 gap-5">
          {open.length > 0 ? (
            <Link
              href={open.length === 1 ? `/approvals/${open[0].approval_id}` : "/approvals"}
              data-slot="waiting-notice"
              className="press -mb-1 flex min-h-11 w-fit max-w-full items-center gap-2.5 rounded-full bg-lapis-soft py-1.5 pr-4 pl-1.5 text-sm font-medium text-lapis outline-none hover:bg-lapis-muted focus-visible:ring-3 focus-visible:ring-ring lg:hidden"
            >
              <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-lapis font-mono text-label text-lapis-foreground tabular">{open.length}</span>
              <span className="truncate">{open.length === 1 ? "Request waiting for you" : "Requests waiting for you"}</span>
              <ArrowRight aria-hidden className="size-4 shrink-0" />
            </Link>
          ) : null}
          <AccountEquityChart />
        </div>
        <div data-layout="rail" className="grid content-start gap-6 lg:pt-1">
          <Waiting ws={ws} open={open} now={now} />
          <AlertsSummary ws={ws} />
        </div>
      </div>

      <Section title="Agents" action={<SectionLink href="/agents">All agents</SectionLink>}>
        <ul className="grid">
          {ws.agents.map((agent, i) => (
            <li key={agent.agent_id} className="grid">
              <AgentCard agent={agent} now={now} marketStale={marketStale} index={i + 2} />
            </li>
          ))}
        </ul>
      </Section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-x-14">
        <Section title="Recent activity" action={canAudit ? <SectionLink href="/audit/decisions">All decisions</SectionLink> : undefined}>
          {ws.decisions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No decisions yet.</p>
          ) : (
            <ol className="grid">
              {ws.decisions.slice(0, 6).map((d) => (
                <GateDecisionRow key={d.event_id} decision={d} agent={findAgent(ws, d.agent_id)} showAgent href={decisionHref(d.agent_id, d.event_id)} />
              ))}
            </ol>
          )}
        </Section>

        <Section title="Positions" action={<SectionLink href="/positions">All positions</SectionLink>}>
          {positions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No agent holds a position.</p>
          ) : (
            <table className="w-full text-sm">
              <caption className="sr-only">Positions across agents</caption>
              <thead className="sr-only">
                <tr>
                  <th scope="col">Holding</th>
                  <th scope="col">Value and unrealized P&amp;L</th>
                </tr>
              </thead>
              <tbody>
                {positions.map(({ agent, p }) => (
                  <tr key={`${agent.agent_id}-${p.instrument.asset_id}`} className="group relative border-b border-border/70 last:border-b-0">
                    <th scope="row" className="py-3 text-left font-normal">
                      <Link href={positionHref(agent.agent_id, p.instrument.asset_id)} className={cn("font-medium underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                        <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                      </Link>
                      <span className="block text-caption text-muted-foreground">{agent.label}</span>
                    </th>
                    <td className="py-3 text-right align-top">
                      <span className="block font-mono tabular">{usd(p.market_value)}</span>
                      <SignedMoney value={p.unrealized_pnl} showWord={false} className="text-caption" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {positions.length > 0 ? (
            <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
              Unrealized paper P&amp;L, simulated. <Placeholder name="performance" />
            </p>
          ) : null}
          {ws.external_positions.length > 0 ? (
            <p className="text-caption text-muted-foreground">
              Also on the account, not managed by any agent: {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
            </p>
          ) : null}
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
