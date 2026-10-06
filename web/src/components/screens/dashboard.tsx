"use client";

import Link from "next/link";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { Inbox } from "pixelarticons/react/Inbox.js";
import { SquareAlert } from "pixelarticons/react/SquareAlert.js";
import { AccountEquityChart } from "@/components/charts/equity-chart";
import { ModeBadge } from "@/components/domain/mode";
import { BrandOwl } from "@/components/brand/brand-owl";
import { AgentOwl } from "@/components/domain/owl";
import type { Agent, Approval, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { alertLines } from "@/lib/attention";
import { clock, price, quantity, zoneLabel } from "@/lib/format";
import { headroomLine } from "@/lib/limits";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { AgentCard, PaperPnlNote } from "./agent-card";
import { AssetsSection } from "./assets-section";
import { DecisionTimeline, tally } from "./decision-timeline";
import { EmptyBoard, PAGE_GRID, Section, SectionLink, WorkspaceGate } from "./common";
import { SideRail } from "./side-rail";

/** Home's rail shows the latest decisions beside the money; the audit has them all. */
const DECISIONS_SHOWN = 4;

/** A phone's Home ends on this many decisions; the rest is one link away. */
const DECISIONS_SHOWN_ON_PHONE = 3;

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
 * Home's first question, answered first at every width (DEC-207, DEC-467): the requests waiting for
 * you, soonest deadline first, each with the static time it is skipped at, then the open alerts. Each
 * row opens where it is read in full. With nothing, it says so plainly. The only place Home shows a
 * request; the dock carries the count.
 */
function NeedsYou({ ws, open }: { ws: Workspace; open: Approval[] }) {
  const lines = alertLines(ws);
  const count = open.length + lines.length;
  return (
    <section aria-labelledby="needs-you-title" data-slot="needs-you" data-count={count} className="grid content-start gap-2">
      <h2 id="needs-you-title" className="flex items-center gap-2.5 text-h2">
        Needs you
        {count > 0 ? (
          <span className="inline-flex h-6 min-w-6 items-center justify-center rounded-sm bg-lapis px-2 font-mono text-label text-lapis-foreground tabular">
            {count}
            <span className="sr-only">{count === 1 ? " item" : " items"}</span>
          </span>
        ) : null}
      </h2>
      {count === 0 ? (
        <p data-slot="all-clear" className="pixel-face flex min-h-11 items-center gap-3 text-sm text-muted-foreground">
          <BrandOwl className="size-8" />
          All clear. Nothing needs you.
        </p>
      ) : (
        <ul className="grid">
          {open.map((a) => (
            <li key={a.approval_id} data-kind="request" className="border-b border-border/70 last:border-b-0">
              <Link href={`/approvals/${a.approval_id}`} className={NEEDS_ROW}>
                <Inbox aria-hidden className="size-6 text-lapis" />
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
                <ChevronRight aria-hidden className="size-6 text-muted-foreground transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" />
              </Link>
            </li>
          ))}
          {lines.map((l) => (
            <li key={l.key} data-kind="alert" className="border-b border-border/70 last:border-b-0">
              <Link href={l.href} className={NEEDS_ROW}>
                <SquareAlert aria-hidden className="size-6 text-foreground" />
                <span className="font-medium text-pretty">{l.text}</span>
                <ChevronRight aria-hidden className="size-6 text-muted-foreground transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" />
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
        className="press -mx-2 grid min-h-11 grid-cols-[2rem_minmax(0,1fr)_1rem] items-center gap-x-3 rounded-xl px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset"
      >
        <AgentOwl agent={agent} className="size-8" />
        <span className="grid min-w-0 gap-1">
          <span className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <span className="font-semibold">{agent.label}</span>
            <ModeBadge mode={agent.mode} />
          </span>
          <span data-slot="headroom" className="text-sm text-muted-foreground tabular">
            {headroomLine(agent)}
          </span>
        </span>
        <ChevronRight aria-hidden className="size-6 text-muted-foreground" />
      </Link>
    </li>
  );
}

/**
 * The latest decisions, after the fact: what each agent set out to do and what its mandate said. Home
 * keeps them beside the money, not ahead of it, with the audit one link away.
 */
function RecentDecisions({ ws, count, id, className }: { ws: Workspace; count: number; id: string; className?: string }) {
  const canAudit = useCan("audit.view");
  const decisions = ws.decisions.slice(0, count);
  return (
    <Section title="Decisions" id={id} className={className} action={canAudit && decisions.length > 0 ? <SectionLink href="/audit/decisions">All decisions</SectionLink> : undefined}>
      {decisions.length === 0 ? (
        <p className="text-sm text-muted-foreground">No decisions yet. Each action an agent wants appears here with what its mandate said.</p>
      ) : (
        <>
          <p data-slot="decision-tally" className="-mt-1 text-sm text-muted-foreground">
            The latest {tally(decisions)}.
          </p>
          <DecisionTimeline ws={ws} decisions={decisions} />
        </>
      )}
    </Section>
  );
}

/**
 * Home answers what an owner opens it for, in that order (DEC-468): does anything need me, and how is
 * my money doing, in the account and in each agent. Needs you comes first in the source and the rail;
 * the account and the agents lead the main column; the latest decisions sit in the rail beside them,
 * and at the foot of a phone's Home.
 */
function Dashboard() {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) return <EmptyBoard />;
  const open = ws.approvals
    .map((a) => approvalAt(a, now))
    .filter((a) => a.status === "delivered")
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const marketStale = ws.health.market_data.state !== "ok";

  return (
    <div className={PAGE_GRID}>
      <h1 className="sr-only">Dashboard</h1>
      <SideRail className="max-lg:gap-8 lg:col-start-2 lg:row-start-1">
        <NeedsYou ws={ws} open={open} />
        <RecentDecisions ws={ws} count={DECISIONS_SHOWN} id="rail-decisions-title" className="max-lg:hidden" />
      </SideRail>

      <div data-layout="main" className="grid min-w-0 grid-cols-1 content-start gap-(--section-gap) lg:col-start-1 lg:row-start-1">
        <div data-slot="account-board" className="reveal grid min-w-0">
          <AccountEquityChart />
        </div>

        <Section title="Agents" action={<SectionLink href="/agents">All agents</SectionLink>}>
          <PaperPnlNote className="-mt-1 max-lg:hidden" />
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

        <AssetsSection ws={ws} className="max-lg:hidden" />

        <RecentDecisions ws={ws} count={DECISIONS_SHOWN_ON_PHONE} id="phone-decisions-title" className="lg:hidden" />
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
