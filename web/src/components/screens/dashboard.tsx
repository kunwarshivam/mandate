"use client";

import Link from "next/link";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { Inbox } from "pixelarticons/react/Inbox.js";
import { SquareAlert } from "pixelarticons/react/SquareAlert.js";
import { AccountEquityChart } from "@/components/charts/equity-chart";
import { ModeBadge } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import type { Agent, Approval, GateDecision, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { needsYouLines, nothingNeedsYou } from "@/lib/attention";
import { clock, price, quantity, zoneLabel } from "@/lib/format";
import { verdictBadge } from "@/lib/gate-reasons";
import { headroomLine } from "@/lib/limits";
import { openRequests, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { AgentCard, PaperPnlNote } from "./agent-card";
import { AssetsSection } from "./assets-section";
import { DecisionTimeline, tally } from "./decision-timeline";
import { AllClear, EmptyBoard, NEEDS_CARD, NEEDS_ITEM, NEEDS_STRIP, PAGE_GRID, Section, SectionLink, WorkspaceGate } from "./common";
import { SideRail } from "./side-rail";

/** Home's rail shows the latest decisions beside the money; the audit has them all. */
const DECISIONS_SHOWN = 4;

/** A phone's Home ends on this many decisions; the rest is one link away. */
const DECISIONS_SHOWN_ON_PHONE = 3;

/** On a phone the sentence is drawn short ("Agent 2: buy 2 XYZ at $141.30") and read in full. */
function requestSentence(ws: Workspace, a: Approval) {
  return (
    <>
      {findAgent(ws, a.agent_id)?.label ?? "An agent"}
      <span aria-hidden className="lg:hidden">
        :
      </span>
      <span className="max-lg:sr-only"> asks to</span> buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at
      <span className="max-lg:sr-only"> a limit of</span> <span className="font-mono tabular">{price(a.bound.limit)}</span>
    </>
  );
}

const NEEDS_ROW = cn(
  "press group -mx-2 grid min-h-11 grid-cols-[1.25rem_minmax(0,1fr)_1rem] items-start gap-x-3 rounded-xl px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset",
  NEEDS_CARD,
);

/**
 * Home's first question, answered first at every width (DEC-207, DEC-467): the requests waiting for
 * you, soonest deadline first, each with the static time it is skipped at, then the agents' open
 * conditions, one line per agent and condition; a degraded feed is the strip's to say (DEC-513). A
 * request is a pale volt card at every width, the one thing on the screen asking for the owner
 * (DEC-515); a condition is a hairline row. Each row opens where it is read in full. With nothing, it says so plainly. The only place Home shows a
 * request; the dock carries the count. On a phone they are one sideways row of cards, never a stack
 * above the money (DEC-482).
 */
function NeedsYou({ ws, open }: { ws: Workspace; open: Approval[] }) {
  const lines = needsYouLines(ws);
  const count = open.length + lines.length;
  return (
    <section aria-labelledby="needs-you-title" data-slot="needs-you" data-count={count} className="grid content-start gap-2">
      <h2 id="needs-you-title" className="flex items-center gap-2.5 text-h2 max-lg:text-h3">
        Needs you
        {count > 0 ? (
          <span className="inline-flex h-6 min-w-6 items-center justify-center rounded-sm bg-lapis px-2 font-mono text-label text-lapis-foreground tabular">
            {count}
            <span className="sr-only">{count === 1 ? " item" : " items"}</span>
          </span>
        ) : null}
      </h2>
      {nothingNeedsYou(ws, open) ? (
        <AllClear />
      ) : (
        <ul className={NEEDS_STRIP}>
          {open.map((a) => (
            <li key={a.approval_id} data-kind="request" className={NEEDS_ITEM}>
              <Link href={`/approvals/${a.approval_id}`} className={cn(NEEDS_ROW, "bg-lapis-soft max-lg:bg-lapis-soft lg:mx-0 lg:rounded-xl lg:px-3")}>
                <Inbox aria-hidden className="size-6 text-lapis" />
                <span className="grid min-w-0 gap-0.5 max-lg:gap-0">
                  <span className="font-medium text-pretty max-lg:text-sm max-lg:leading-5">{requestSentence(ws, a)}</span>
                  <span data-slot="deadline" className="text-sm text-muted-foreground max-lg:text-caption">
                    Skipped at{" "}
                    <time dateTime={a.deadline} className="font-mono tabular">
                      {clock(a.deadline)} {zoneLabel(a.deadline)}
                    </time>
                    <span className="max-lg:sr-only"> if you do nothing</span>
                  </span>
                </span>
                <ChevronRight aria-hidden className="size-6 text-muted-foreground transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" />
              </Link>
            </li>
          ))}
          {lines.map((l) => (
            <li key={l.key} data-kind="alert" className={NEEDS_ITEM}>
              <Link href={l.href} className={cn(NEEDS_ROW, "max-lg:bg-background")}>
                <SquareAlert aria-hidden className="size-6 text-foreground" />
                <span className="font-medium text-pretty max-lg:text-sm max-lg:leading-5">{l.text}</span>
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
 * Home's decisions leave out the one that asked for a request still open under Needs you, so a
 * request appears once on Home (DESIGN.md, Needs you; critique C-5). Once the request is answered or
 * skipped its row comes back; the audit and the agent's Decisions keep it throughout.
 */
function decisionsBesideNeedsYou(decisions: GateDecision[], open: Approval[]): GateDecision[] {
  const openIds = new Set(open.map((a) => a.approval_id));
  return decisions.filter((d) => !(verdictBadge(d) === "Asked you" && d.approval_id && openIds.has(d.approval_id)));
}

/**
 * The latest decisions, after the fact: what each agent set out to do and what its mandate said. Home
 * keeps them beside the money, not ahead of it, with the audit one link away.
 */
function RecentDecisions({ ws, open, count, id, className }: { ws: Workspace; open: Approval[]; count: number; id: string; className?: string }) {
  const canAudit = useCan("audit.view");
  const decisions = decisionsBesideNeedsYou(ws.decisions, open).slice(0, count);
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
  const open = openRequests(ws, now);
  const marketStale = ws.health.market_data.state !== "ok";

  return (
    <div className={cn(PAGE_GRID, "max-lg:gap-8")}>
      <h1 className="sr-only">Dashboard</h1>
      <SideRail className="max-lg:gap-8 lg:col-start-2 lg:row-start-1">
        <NeedsYou ws={ws} open={open} />
        <RecentDecisions ws={ws} open={open} count={DECISIONS_SHOWN} id="rail-decisions-title" className="max-lg:hidden" />
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

        <RecentDecisions ws={ws} open={open} count={DECISIONS_SHOWN_ON_PHONE} id="phone-decisions-title" className="lg:hidden" />
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
