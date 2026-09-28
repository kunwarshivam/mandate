"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowRight, ArrowsClockwise, Octagon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { LinkButton } from "@cloudflare/kumo/components/button";
import { Deadline } from "@/components/approvals/deadline";
import { AsOf } from "@/components/domain/as-of";
import { Envelope } from "@/components/domain/envelope";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { MODE_FIELD, ModeBanner } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { OrdersTable, PositionsTable } from "@/components/domain/positions";
import { Timeline } from "@/components/domain/timeline";
import { price, quantity, usd } from "@/lib/format";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { OPEN_STOP_EVENT } from "@/components/shell/stop-control";
import type { Agent } from "@/fixtures/types";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { AGENT_SECTIONS, type AgentSectionKey, agentHref } from "@/lib/screens";
import { ComingSoon } from "./coming-soon";
import { Panel, Section, WorkspaceGate } from "./common";
import { MandateSummary } from "./mandate-summary";

function NotFound() {
  return (
    <section aria-labelledby="missing-title" className="reveal grid max-w-3xl gap-3 border-t-4 border-foreground bg-muted p-4 sm:p-6">
      <h1 id="missing-title" className="text-title sm:text-display">
        No agent with this ID
      </h1>
      <p className="max-w-prose">This workspace has no agent with that ID. It may belong to another workspace.</p>
      <LinkButton href="/agents" variant="outline" size="lg" className="h-11 w-fit">
        See all agents
      </LinkButton>
    </section>
  );
}

const TOP_SECTIONS = AGENT_SECTIONS.filter((s) => !s.parent);

/**
 * Every agent screen shares this frame: the owner's label for the agent as the title, the paper
 * badge beside it, route tabs, and a Stop scoped to this agent. The document title stays generic.
 */
function AgentFrame({ agent, children }: { agent: Agent; children: ReactNode }) {
  const canStop = useCan("stop.open");
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <PageHeader
        title={agent.label}
        environment={useRuntime().ws.environment}
        description={agent.mandate.name}
        tabs={TOP_SECTIONS.map((s) => ({ href: agentHref(agent.agent_id, s.key), label: s.label }))}
        tabsLabel="Agent sections"
        className="mb-0"
        actions={
          canStop ? (
            <button
              type="button"
              onClick={() => window.dispatchEvent(new Event(OPEN_STOP_EVENT))}
              aria-haspopup="dialog"
              className="press inline-flex h-11 items-center gap-2 border-2 border-ink bg-card px-3 font-bold text-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              <Octagon className="size-4" weight="bold" aria-hidden />
              Stop this agent…
            </button>
          ) : null
        }
      >
        <ModeBanner mode={agent.mode} restrictions={agent.restrictions} showMode={false} />
      </PageHeader>
      {children}
    </div>
  );
}

function useAgent(agentId: string): Agent | undefined {
  return useRuntime().ws.agents.find((a) => a.agent_id === agentId);
}

function AgentDetail({ agentId }: { agentId: string }) {
  const { ws, now } = useRuntime();
  const agent = useAgent(agentId);
  if (!agent) return <NotFound />;
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
      </div>

      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <div className="grid grid-cols-1 content-start gap-(--section-gap)">
          <Envelope agent={agent} />

          <Section title="Positions">
            <Panel>
              <PositionsTable positions={agent.positions} now={now} staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols} />
            </Panel>
          </Section>

          <Section title="Working orders">
            <OrdersTable orders={agent.orders} />
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
                    <GateDecisionRow key={d.event_id} decision={d} agent={agent} />
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
            <PositionsTable positions={agent.positions} now={now} staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols} />
          </Panel>
        </Section>
      );
    case "orders":
      return (
        <Section title="Orders">
          <OrdersTable orders={agent.orders} />
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
                  <GateDecisionRow key={d.event_id} decision={d} agent={agent} />
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
          <Section title="Mandate">
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
  if (!agent) return <NotFound />;
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
