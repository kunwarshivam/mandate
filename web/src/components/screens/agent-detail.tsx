"use client";

import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { Inbox } from "pixelarticons/react/Inbox.js";
import { Deadline } from "@/components/approvals/deadline";
import { AgentEquityChart } from "@/components/charts/equity-chart";
import { AsOf } from "@/components/domain/as-of";
import { Envelope, Headroom, MandateCard } from "@/components/domain/envelope";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { MandateEdit } from "@/components/mandate/mandate-edit";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { OrdersTable, PositionsTable } from "@/components/domain/positions";
import { Timeline } from "@/components/domain/timeline";
import { clock, price, quantity, zoneLabel } from "@/lib/format";
import type { Agent } from "@/fixtures/types";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { allOrders } from "@/lib/orders";
import { cn } from "@/lib/utils";
import { AGENT_SECTIONS, type AgentSectionKey, agentHref, decisionHref, orderHref, positionHref } from "@/lib/screens";
import { AgentFrame, AgentNotFound, useAgent } from "./agent-frame";
import { ComingSoon } from "./coming-soon";
import { NEEDS_CARD, NEEDS_ITEM, NEEDS_STRIP, PAGE_GRID, Section, SectionLink, WorkspaceGate } from "./common";
import { MandateSummary } from "./mandate-summary";
import { MandateVersions } from "./mandate-versions";
import { NewsSection } from "./news-section";
import { SideRail } from "./side-rail";

/** A list row that opens a record: the whole row is the target, on hairlines rather than boxes. */
const ROW_LINK =
  "press group -mx-3 grid gap-1 rounded-xl px-3 py-3.5 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";

/** The overview shows the latest few events; the Activity tab has them all. */
const RECENT_ACTIVITY = 5;

function AgentDetail({ agentId }: { agentId: string }) {
  const { ws, now } = useRuntime();
  const agent = useAgent(agentId);
  if (!agent) return <AgentNotFound />;
  const staleSymbols = new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
  const marketStale = ws.health.market_data.state !== "ok";
  const open = ws.approvals.filter((a) => a.agent_id === agent.agent_id).map((a) => approvalAt(a, now)).filter((a) => a.status === "delivered");
  const decisions = ws.decisions.filter((d) => d.agent_id === agent.agent_id);
  const activity = ws.timeline[agent.agent_id] ?? [];
  const markAt = agent.positions[0]?.mark_as_of;

  /*
   * One story in the wide column, and only the mandate at a glance beside it, spanning both rows of the
   * story and staying in view. A phone keeps what needs you, the equity and the headroom; the rest is a
   * section link away (DEC-207).
   */
  return (
    <AgentFrame agent={agent}>
      {open.length > 0 ? (
        <section aria-labelledby="phone-waiting-title" data-slot="phone-waiting" className="grid gap-2 lg:hidden">
          <h2 id="phone-waiting-title" className="text-h3">
            Waiting for you
          </h2>
          <ul className={NEEDS_STRIP}>
            {open.map((a) => (
              <li key={a.approval_id} className={NEEDS_ITEM}>
                <Link
                  href={`/approvals/${a.approval_id}`}
                  className={cn(
                    "press -mx-2 grid min-h-11 grid-cols-[1.25rem_minmax(0,1fr)_1rem] items-start gap-x-3 rounded-xl bg-lapis-soft px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset",
                    NEEDS_CARD,
                  )}
                >
                  <Inbox aria-hidden className="size-6 text-lapis" />
                  <span className="grid min-w-0 gap-0">
                    <span className="text-sm leading-5 font-medium text-pretty">
                      Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at<span className="sr-only"> a limit of</span>{" "}
                      <span className="font-mono tabular">{price(a.bound.limit)}</span>
                    </span>
                    <span data-slot="deadline" className="text-caption text-muted-foreground">
                      Skipped at{" "}
                      <time dateTime={a.deadline} className="font-mono tabular">
                        {clock(a.deadline)} {zoneLabel(a.deadline)}
                      </time>
                      <span className="sr-only"> if you do nothing</span>
                    </span>
                  </span>
                  <ChevronRight aria-hidden className="size-6 text-muted-foreground" />
                </Link>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      <div className={cn(PAGE_GRID, "max-lg:gap-8 lg:grid-rows-[auto_1fr]")}>
        <div data-layout="main" className="reveal grid min-w-0 content-start gap-6 lg:col-start-1 lg:row-start-1">
          <AgentEquityChart agent={agent} />
        </div>

        <Headroom agent={agent} className="lg:hidden" />

        <SideRail className="max-lg:hidden lg:col-start-2 lg:row-span-2 lg:row-start-1">
          <MandateCard agent={agent} />
        </SideRail>

        <div data-layout="main" className="grid min-w-0 grid-cols-1 content-start gap-(--section-gap) max-lg:hidden lg:col-start-1 lg:row-start-2">
          <section aria-labelledby="figures-title" data-slot="key-figures" className="grid gap-4">
            <h2 id="figures-title" className="text-h2">
              Key figures
            </h2>
            <dl className="grid grid-cols-2 gap-x-6 gap-y-5 sm:grid-cols-4">
              <div className="grid gap-1">
                <dt className="text-sm text-muted-foreground">Capital</dt>
                <dd className="text-figure">
                  <Money value={agent.mandate.capital.allocation_usd} places={0} />
                </dd>
              </div>
              <div className="grid gap-1">
                <dt className="text-sm text-muted-foreground">Equity</dt>
                <dd className="text-figure">
                  <Money value={agent.state.equity} />
                </dd>
              </div>
              <div className="grid gap-1">
                <dt className="text-sm text-muted-foreground">Paper P&amp;L, simulated</dt>
                <dd>
                  <SignedMoney value={agent.pnl_total} className="text-figure" />
                </dd>
              </div>
              <div className="grid gap-1">
                <dt className="text-sm text-muted-foreground">Today</dt>
                <dd>
                  <SignedMoney value={agent.pnl_today} className="text-figure" />
                </dd>
              </div>
            </dl>
            <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
              <Placeholder name="performance" />
              {marketStale && markAt ? <AsOf at={markAt} now={now} stale /> : null}
            </p>
          </section>

          {open.length > 0 ? (
            <Section title="Waiting for you">
              <ul className="grid gap-2">
                {open.map((a) => (
                  <li key={a.approval_id}>
                    <Link
                      href={`/approvals/${a.approval_id}`}
                      className="press group grid gap-1.5 rounded-2xl bg-lapis-soft px-4 py-3.5 outline-none hover:bg-lapis-soft/70 focus-visible:ring-3 focus-visible:ring-ring"
                    >
                      <span className="flex items-baseline justify-between gap-3 font-medium">
                        <span>
                          Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of <span className="font-mono tabular">{price(a.bound.limit)}</span>
                        </span>
                        <ArrowRight className="size-6 shrink-0 self-center text-lapis transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" aria-hidden />
                      </span>
                      <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                    </Link>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}

          <Section title="Positions" action={<SectionLink href={agentHref(agent.agent_id, "positions")}>All positions</SectionLink>}>
            <PositionsTable
              positions={agent.positions}
              now={now}
              staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols}
              hrefFor={(p) => positionHref(agent.agent_id, p.instrument.asset_id)}
            />
          </Section>

          <Section title="Working orders" action={<SectionLink href={agentHref(agent.agent_id, "orders")}>All orders</SectionLink>}>
            <OrdersTable orders={agent.orders} hrefFor={(o) => orderHref(agent.agent_id, o.client_order_id)} />
          </Section>

          <Section title="Recent decisions" action={<SectionLink href={agentHref(agent.agent_id, "decisions")}>All decisions</SectionLink>}>
            {decisions.length === 0 ? (
              <p className="text-sm text-muted-foreground">No decisions yet.</p>
            ) : (
              <ul className="grid">
                {decisions.map((d) => (
                  <GateDecisionRow key={d.event_id} decision={d} agent={agent} href={decisionHref(agent.agent_id, d.event_id)} />
                ))}
              </ul>
            )}
          </Section>

          <Section title="Activity" action={<SectionLink href={agentHref(agent.agent_id, "activity")}>View all activity</SectionLink>}>
            <Timeline events={activity.slice(0, RECENT_ACTIVITY)} now={now} />
          </Section>

          <NewsSection ws={ws} agent={agent} now={now} />
        </div>
      </div>
    </AgentFrame>
  );
}

export function AgentDetailScreen({ agentId }: { agentId: string }) {
  return (
    <WorkspaceGate>
      <AgentDetail agentId={agentId} />
    </WorkspaceGate>
  );
}

function AgentApprovals({ agent }: { agent: Agent }) {
  const { ws, now } = useRuntime();
  const all = ws.approvals.filter((a) => a.agent_id === agent.agent_id).map((a) => approvalAt(a, now));
  if (all.length === 0) return <p className="text-muted-foreground">No requests from this agent.</p>;
  return (
    <ul className="grid divide-y divide-border/70">
      {all.map((a) => (
        <li key={a.approval_id}>
          <Link href={`/approvals/${a.approval_id}`} className={ROW_LINK}>
            <span className="flex items-baseline justify-between gap-3 font-medium">
              <span>
                Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of <span className="font-mono tabular">{price(a.bound.limit)}</span>
              </span>
              <ArrowRight className="size-6 shrink-0 self-center" aria-hidden />
            </span>
            {a.status === "delivered" ? (
              <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
            ) : (
              <span className="text-sm text-muted-foreground">{a.resolution?.text}</span>
            )}
          </Link>
        </li>
      ))}
    </ul>
  );
}

function SectionBody({ agent, section }: { agent: Agent; section: AgentSectionKey }) {
  const { ws, now } = useRuntime();
  const marketStale = ws.health.market_data.state !== "ok";
  const staleSymbols = new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
  const meta = AGENT_SECTIONS.find((s) => s.key === section);
  if (!meta) return null;
  if (!meta.built) {
    const parent = meta.parent ? AGENT_SECTIONS.find((s) => s.key === meta.parent) : undefined;
    return (
      <Section title={meta.label}>
        <ComingSoon
          purpose={meta.purpose}
          back={parent ? { href: agentHref(agent.agent_id, parent.key), label: `Back to ${parent.label.toLowerCase()}` } : undefined}
        />
      </Section>
    );
  }
  switch (section) {
    case "overview":
      return null;
    case "positions":
      return (
        <Section title="Positions">
            <PositionsTable
                positions={agent.positions}
                now={now}
                staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols}
                hrefFor={(p) => positionHref(agent.agent_id, p.instrument.asset_id)}
              />
        </Section>
      );
    case "orders":
      return (
        <Section title="Orders">
          <OrdersTable orders={allOrders(agent)} hrefFor={(o) => orderHref(agent.agent_id, o.client_order_id)} empty="No orders yet." />
        </Section>
      );
    case "decisions": {
      const decisions = ws.decisions.filter((d) => d.agent_id === agent.agent_id);
      return (
        <Section title="Gate decisions">
            {decisions.length === 0 ? (
              <p className="text-sm text-muted-foreground">No decisions yet.</p>
            ) : (
              <ul className="grid">
                {decisions.map((d) => (
                  <GateDecisionRow key={d.event_id} decision={d} agent={agent} href={decisionHref(agent.agent_id, d.event_id)} />
                ))}
              </ul>
            )}
        </Section>
      );
    }
    case "approvals":
      return (
        <Section title="Approvals">
          <AgentApprovals agent={agent} />
        </Section>
      );
    case "mandate":
      return (
        <div className="grid grid-cols-1 gap-(--section-gap)">
          <Envelope agent={agent} />
          <Section
            title="Mandate details"
            action={
              <span className="flex items-center gap-4">
                <SectionLink href={agentHref(agent.agent_id, "mandate/edit")}>Edit</SectionLink>
                <SectionLink href={agentHref(agent.agent_id, "mandate/versions")}>Versions</SectionLink>
              </span>
            }
          >
            <MandateSummary agent={agent} />
          </Section>
        </div>
      );
    case "prove":
      return (
        <Section title="Prove">
          <ul className="grid divide-y divide-border/70">
            {AGENT_SECTIONS.filter((s) => s.parent === "prove").map((s) => (
              <li key={s.key}>
                <Link href={agentHref(agent.agent_id, s.key)} className={ROW_LINK}>
                  <span className="flex items-center justify-between gap-3 font-medium">
                    {s.label}
                    <ArrowRight className="size-6 shrink-0" aria-hidden />
                  </span>
                  <span className="text-sm text-muted-foreground">{s.purpose}</span>
                </Link>
              </li>
            ))}
          </ul>
        </Section>
      );
    case "activity":
      return (
        <Section title="Activity">
            <Timeline events={ws.timeline[agent.agent_id] ?? []} now={now} />
        </Section>
      );
    case "mandate/versions":
      return (
        <Section title="Versions">
          <MandateVersions agent={agent} />
        </Section>
      );
    case "mandate/edit":
      return (
        <Section title="Edit mandate" action={<SectionLink href={agentHref(agent.agent_id, "mandate/versions")}>Versions</SectionLink>}>
          <MandateEdit agent={agent} />
        </Section>
      );
    case "prove/backtests":
    case "prove/paper":
    case "prove/live":
      return null;
    default: {
      const unhandled: never = section;
      throw new Error(`unhandled section ${String(unhandled)}`);
    }
  }
}

function AgentSection({ agentId, section }: { agentId: string; section: AgentSectionKey }) {
  const agent = useAgent(agentId);
  if (!agent) return <AgentNotFound />;
  return (
    <AgentFrame agent={agent}>
      <SectionBody agent={agent} section={section} />
    </AgentFrame>
  );
}

export function AgentSectionScreen({ agentId, section }: { agentId: string; section: AgentSectionKey }) {
  return (
    <WorkspaceGate>
      <AgentSection agentId={agentId} section={section} />
    </WorkspaceGate>
  );
}
