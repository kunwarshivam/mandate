"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { ArrowRight, CaretRight, Check, CheckCircle, Tray, WarningCircle } from "@phosphor-icons/react";
import { Deadline } from "@/components/approvals/deadline";
import { AccountEquityChart } from "@/components/charts/equity-chart";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { ModeBadge } from "@/components/domain/mode";
import { SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import type { Agent, Approval, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { alertLines } from "@/lib/attention";
import { clock, price, quantity, usd, zoneLabel } from "@/lib/format";
import { headroomLine } from "@/lib/limits";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { decisionHref, positionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { AgentCard } from "./agent-card";
import { EmptyBoard, Section, SectionLink, WorkspaceGate } from "./common";

/** The rail beside the account chart shows this many requests; the rest are one link away. */
const WAITING_SHOWN = 3;

/** A phone's Home shows this much recent activity; the rest is one link away. */
const ACTIVITY_SHOWN_ON_PHONE = 3;

function AlertsSummary({ ws }: { ws: Workspace }) {
  const lines = alertLines(ws).map((a) => a.text);
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

function requestSentence(ws: Workspace, a: Approval) {
  return (
    <>
      {findAgent(ws, a.agent_id)?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of{" "}
      <span className="font-mono tabular">{price(a.bound.limit)}</span>
    </>
  );
}

const NEEDS_ROW =
  "press group -mx-2 grid min-h-11 grid-cols-[1.25rem_minmax(0,1fr)_1rem] items-start gap-x-3 rounded-xl px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset";

/**
 * The phone's first question, answered first (DEC-207): the requests waiting for you, soonest
 * deadline first, each with the static time it is skipped at, then the open alerts. Each row opens
 * where it is read in full. With nothing, it says so plainly. The only place Home shows a request.
 */
function NeedsYou({ ws, open, className }: { ws: Workspace; open: Approval[]; className?: string }) {
  const lines = alertLines(ws);
  const count = open.length + lines.length;
  return (
    <section aria-labelledby="needs-you-title" data-slot="needs-you" data-count={count} className={cn("grid content-start gap-2", className)}>
      <h2 id="needs-you-title" className="flex items-center gap-2.5 text-h2">
        Needs you
        {count > 0 ? (
          <span className="inline-flex h-6 min-w-6 items-center justify-center rounded-full bg-lapis px-2 font-mono text-label text-lapis-foreground tabular">
            {count}
            <span className="sr-only">{count === 1 ? " item" : " items"}</span>
          </span>
        ) : null}
      </h2>
      {count === 0 ? (
        <p data-slot="all-clear" className="flex min-h-11 items-center gap-2.5 text-sm text-muted-foreground">
          <Check aria-hidden className="size-5 shrink-0" />
          All clear. Nothing needs you.
        </p>
      ) : (
        <ul className="grid">
          {open.map((a) => (
            <li key={a.approval_id} data-kind="request" className="border-b border-border/70 last:border-b-0">
              <Link href={`/approvals/${a.approval_id}`} className={NEEDS_ROW}>
                <Tray aria-hidden weight="fill" className="mt-0.5 size-5 text-lapis" />
                <span className="grid gap-0.5">
                  <span className="font-medium text-pretty">{requestSentence(ws, a)}</span>
                  <span data-slot="deadline" className="text-sm text-muted-foreground">
                    Skipped at{" "}
                    <time dateTime={a.deadline} className="font-mono tabular">
                      {clock(a.deadline)} {zoneLabel(a.deadline)}
                    </time>{" "}
                    if you do nothing
                  </span>
                </span>
                <CaretRight aria-hidden className="mt-1 size-4 text-muted-foreground" />
              </Link>
            </li>
          ))}
          {lines.map((l) => (
            <li key={l.key} data-kind="alert" className="border-b border-border/70 last:border-b-0">
              <Link href={l.href} className={NEEDS_ROW}>
                <WarningCircle aria-hidden weight="fill" className="mt-0.5 size-5 text-foreground" />
                <span className="font-medium text-pretty">{l.text}</span>
                <CaretRight aria-hidden className="mt-1 size-4 text-muted-foreground" />
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * An agent on a phone: its name, its state in a word with its glyph, and its headroom to the next
 * level where its behaviour changes. No P&L, so no disclosure to repeat; the row opens the agent.
 */
function PhoneAgentRow({ agent }: { agent: Agent }) {
  return (
    <li data-slot="phone-agent" className="border-b border-border/70 last:border-b-0">
      <Link
        href={`/agents/${agent.agent_id}`}
        className="press -mx-2 grid min-h-11 grid-cols-[minmax(0,1fr)_1rem] items-center gap-x-3 rounded-xl px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset"
      >
        <span className="grid min-w-0 gap-1">
          <span className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <span className="font-semibold">{agent.label}</span>
            <ModeBadge mode={agent.mode} />
          </span>
          <span data-slot="headroom" className="text-sm text-muted-foreground tabular">
            {headroomLine(agent)}
          </span>
        </span>
        <CaretRight aria-hidden className="size-4 text-muted-foreground" />
      </Link>
    </li>
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
      <div className="grid grid-cols-1 gap-(--section-gap) max-lg:gap-8 lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-x-14">
        <NeedsYou ws={ws} open={open} className="lg:hidden" />
        <div data-slot="account-board" data-layout="main" className="reveal grid min-w-0 gap-5">
          <AccountEquityChart />
        </div>
        <div data-layout="rail" className="grid content-start gap-6 max-lg:hidden lg:pt-1">
          <Waiting ws={ws} open={open} now={now} />
          <AlertsSummary ws={ws} />
        </div>
      </div>

      <Section title="Agents" action={<SectionLink href="/agents">All agents</SectionLink>}>
        <ul className="grid max-lg:hidden">
          {ws.agents.map((agent, i) => (
            <li key={agent.agent_id} className="grid">
              <AgentCard agent={agent} now={now} marketStale={marketStale} index={i + 2} />
            </li>
          ))}
        </ul>
        <ul aria-label="Agents" data-slot="phone-agents" className="grid lg:hidden">
          {ws.agents.map((agent) => (
            <PhoneAgentRow key={agent.agent_id} agent={agent} />
          ))}
        </ul>
      </Section>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-x-14">
        <Section
          title="Recent activity"
          action={
            canAudit ? (
              <SectionLink href="/audit/decisions" className="max-lg:hidden">
                All decisions
              </SectionLink>
            ) : undefined
          }
        >
          {ws.decisions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No decisions yet.</p>
          ) : (
            <ol className="grid">
              {ws.decisions.slice(0, 6).map((d, i) => (
                <GateDecisionRow
                  key={d.event_id}
                  decision={d}
                  agent={findAgent(ws, d.agent_id)}
                  showAgent
                  href={decisionHref(d.agent_id, d.event_id)}
                  className={i >= ACTIVITY_SHOWN_ON_PHONE ? "max-lg:hidden" : "max-lg:[&:nth-child(3)]:border-b-0"}
                />
              ))}
            </ol>
          )}
          {canAudit && ws.decisions.length > 0 ? (
            <SectionLink href="/audit/decisions" className="w-fit lg:hidden">
              See all activity
            </SectionLink>
          ) : null}
        </Section>

        <Section title="Positions" action={<SectionLink href="/positions">All positions</SectionLink>} className="max-lg:hidden">
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
