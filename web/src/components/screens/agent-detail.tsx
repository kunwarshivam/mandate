"use client";

import Link from "next/link";
import { ArrowRight, ArrowsClockwise } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { Deadline } from "@/components/approvals/deadline";
import { AgentEquityChart } from "@/components/charts/equity-chart";
import { AsOf } from "@/components/domain/as-of";
import { Envelope } from "@/components/domain/envelope";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { MODE_FIELD } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { OrdersTable, PositionsTable } from "@/components/domain/positions";
import { Timeline } from "@/components/domain/timeline";
import { price, quantity, usd } from "@/lib/format";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import type { Agent } from "@/fixtures/types";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { allOrders } from "@/lib/orders";
import { AGENT_SECTIONS, type AgentSectionKey, agentHref, decisionHref, orderHref, positionHref } from "@/lib/screens";
import { AgentFrame, AgentNotFound, useAgent } from "./agent-frame";
import { ComingSoon } from "./coming-soon";
import { Panel, Section, WorkspaceGate } from "./common";
import { MandateSummary } from "./mandate-summary";

function AgentDetail({ agentId }: { agentId: string }) {
  const { ws, now } = useRuntime();
  const agent = useAgent(agentId);
  if (!agent) return <AgentNotFound />;
  const staleSymbols = new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
  const marketStale = ws.health.market_data.state !== "ok";
  const open = ws.approvals.filter((a) => a.agent_id === agent.agent_id).map((a) => approvalAt(a, now)).filter((a) => a.status === "delivered");
  const decisions = ws.decisions.filter((d) => d.agent_id === agent.agent_id);
  const markAt = agent.positions[0]?.mark_as_of;

  return (
    <AgentFrame agent={agent}>
      <div className="grid grid-cols-1 gap-(--seam)">
        <section aria-label="Mode and figures" data-mode={agent.mode} className="reveal grid grid-cols-1 gap-(--seam) md:grid-cols-[11rem_minmax(0,1fr)]">
          <div
            data-slot="mode-field"
            className={cn(
              "flex flex-wrap items-baseline gap-x-3 gap-y-1 px-3 py-2.5 transition-colors duration-(--duration-hover) sm:px-4 md:flex-col md:flex-nowrap md:items-start md:justify-end md:py-4",
              MODE_FIELD[agent.mode],
            )}
          >
            <p className="font-display text-[1.625rem] leading-[0.9] font-extrabold uppercase md:text-[2rem]">{MODE_LABEL[agent.mode]}</p>
            <p className="text-sm font-medium">{MODE_MEANING[agent.mode]}</p>
          </div>
          <div className="grid gap-3 bg-card px-3 py-3 sm:px-4 sm:py-4">
            {agent.startup === "reconciling" ? (
              <p role="status" data-slot="reconciling" className="flex items-center gap-2 font-bold">
                <ArrowsClockwise className="size-4 shrink-0" aria-hidden />
                Checking with the broker. Nothing is needed from you.
              </p>
            ) : null}
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 sm:grid-cols-[auto_auto_auto] sm:justify-start sm:gap-x-8">
              <div className="col-span-2 sm:col-span-1">
                <dt className="label-caps text-muted-foreground">Equity</dt>
                <dd className="font-display text-[2.25rem] leading-none font-bold">
                  <Money value={agent.state.equity} className="font-display" />
                </dd>
                <dd className="text-caption text-muted-foreground">
                  of <span className="font-mono tabular">{usd(agent.mandate.capital.allocation_usd, 0)}</span> capital
                </dd>
              </div>
              <div>
                <dt className="label-caps text-muted-foreground">Paper P&amp;L, simulated</dt>
                <dd>
                  <SignedMoney value={agent.pnl_total} className="font-bold" />
                </dd>
              </div>
              <div>
                <dt className="label-caps text-muted-foreground">Today</dt>
                <dd>
                  <SignedMoney value={agent.pnl_today} className="font-bold" />
                </dd>
              </div>
            </dl>
            <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
              <Placeholder name="performance" />
              {marketStale && markAt ? <AsOf at={markAt} now={now} stale /> : null}
            </p>
          </div>
        </section>
        <AgentEquityChart agent={agent} />
      </div>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <div className="grid grid-cols-1 content-start gap-(--section-gap)">
          <Envelope agent={agent} />

          <Section title="Positions">
            <Panel>
              <PositionsTable
                positions={agent.positions}
                now={now}
                staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols}
                hrefFor={(p) => positionHref(agent.agent_id, p.instrument.asset_id)}
              />
            </Panel>
          </Section>

          <Section
            title="Working orders"
            action={
              <Link href={agentHref(agent.agent_id, "orders")} className="text-sm font-bold text-lapis underline underline-offset-4 hover:decoration-2">
                All orders
              </Link>
            }
          >
            <OrdersTable orders={agent.orders} hrefFor={(o) => orderHref(agent.agent_id, o.client_order_id)} />
          </Section>
        </div>

        <div className="grid grid-cols-1 content-start gap-(--section-gap)">
          {open.length > 0 ? (
            <Section title="Waiting for you">
              <ul className="grid gap-(--seam)">
                {open.map((a) => (
                  <li key={a.approval_id}>
                    <Link href={`/approvals/${a.approval_id}`} className="press group grid gap-1 bg-card px-3 py-3 hover:bg-muted sm:px-4">
                      <span className="flex items-baseline justify-between gap-3 font-bold">
                        <span>
                          Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of <span className="font-mono tabular">{price(a.bound.limit)}</span>
                        </span>
                        <ArrowRight className="size-4 shrink-0 self-center" aria-hidden />
                      </span>
                      <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                    </Link>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}

          <Section title="Mandate">
            <Panel>
              <MandateSummary agent={agent} />
            </Panel>
          </Section>

          <Section title="Gate decisions">
            <Panel className="py-0.5 sm:py-0.5">
              {decisions.length === 0 ? (
                <p className="py-2.5 text-sm text-muted-foreground">No decisions yet.</p>
              ) : (
                <ul>
                  {decisions.map((d) => (
                    <GateDecisionRow key={d.event_id} decision={d} agent={agent} href={decisionHref(agent.agent_id, d.event_id)} />
                  ))}
                </ul>
              )}
            </Panel>
          </Section>

          <Section title="Activity">
            <Panel>
              <Timeline events={ws.timeline[agent.agent_id] ?? []} today={ws.now.slice(0, 10)} />
            </Panel>
          </Section>
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
  if (all.length === 0) return <p className="bg-muted px-3 py-3 text-muted-foreground sm:px-4">No requests from this agent.</p>;
  return (
    <ul className="grid gap-(--seam)">
      {all.map((a) => (
        <li key={a.approval_id}>
          <Link href={`/approvals/${a.approval_id}`} className="press grid gap-1 bg-card px-3 py-3 hover:bg-muted sm:px-4">
            <span className="flex items-baseline justify-between gap-3 font-bold">
              <span>
                Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of <span className="font-mono tabular">{price(a.bound.limit)}</span>
              </span>
              <ArrowRight className="size-4 shrink-0 self-center" aria-hidden />
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
          <Panel>
            <PositionsTable
                positions={agent.positions}
                now={now}
                staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols}
                hrefFor={(p) => positionHref(agent.agent_id, p.instrument.asset_id)}
              />
          </Panel>
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
          <Panel className="py-0.5 sm:py-0.5">
            {decisions.length === 0 ? (
              <p className="py-2.5 text-sm text-muted-foreground">No decisions yet.</p>
            ) : (
              <ul>
                {decisions.map((d) => (
                  <GateDecisionRow key={d.event_id} decision={d} agent={agent} href={decisionHref(agent.agent_id, d.event_id)} />
                ))}
              </ul>
            )}
          </Panel>
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
            title="Mandate"
            action={
              <Link href={agentHref(agent.agent_id, "mandate/versions")} className="text-sm font-bold text-lapis underline underline-offset-4 hover:decoration-2">
                Versions
              </Link>
            }
          >
            <Panel>
              <MandateSummary agent={agent} />
            </Panel>
          </Section>
        </div>
      );
    case "prove":
      return (
        <Section title="Prove">
          <ul className="grid gap-(--seam)">
            {AGENT_SECTIONS.filter((s) => s.parent === "prove").map((s) => (
              <li key={s.key}>
                <Link href={agentHref(agent.agent_id, s.key)} className="press grid gap-1 bg-card px-3 py-3 hover:bg-muted sm:px-4">
                  <span className="flex items-center justify-between gap-3 font-bold">
                    {s.label}
                    <ArrowRight className="size-4 shrink-0" aria-hidden />
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
          <Panel>
            <Timeline events={ws.timeline[agent.agent_id] ?? []} today={ws.now.slice(0, 10)} />
          </Panel>
        </Section>
      );
    case "mandate/versions":
    case "mandate/edit":
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
