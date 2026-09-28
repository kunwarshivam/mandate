"use client";

import Link from "next/link";
import { ArrowLeft, RefreshCw } from "lucide-react";
import { Deadline } from "@/components/approvals/deadline";
import { AsOf } from "@/components/domain/as-of";
import { Envelope } from "@/components/domain/envelope";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { ModeBadge, ModeBanner } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { OrdersTable, PositionsTable } from "@/components/domain/positions";
import { Timeline } from "@/components/domain/timeline";
import { price, quantity } from "@/lib/format";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { Panel, Section, WorkspaceGate } from "./common";
import { MandateSummary } from "./mandate-summary";

function AgentDetail({ agentId }: { agentId: string }) {
  const { ws, now } = useRuntime();
  const agent = ws.agents.find((a) => a.agent_id === agentId);
  if (!agent) {
    return (
      <Panel className="max-w-xl">
        <h1 className="text-heading">No agent with this ID</h1>
        <p className="mt-2 text-muted-foreground">This workspace has no agent with that ID. It may belong to another workspace.</p>
        <Link href="/agents" className="mt-4 inline-block text-primary underline-offset-4 hover:underline">
          See all agents
        </Link>
      </Panel>
    );
  }
  const staleSymbols = new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
  const marketStale = ws.health.market_data.state !== "ok";
  const open = ws.approvals.filter((a) => a.agent_id === agent.agent_id).map((a) => approvalAt(a, now)).filter((a) => a.status === "delivered");
  const decisions = ws.decisions.filter((d) => d.agent_id === agent.agent_id);

  return (
    <div className="grid gap-8">
      <div className="grid gap-4">
        <Link href="/agents" className="inline-flex w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
          <ArrowLeft className="size-4" aria-hidden />
          Agents
        </Link>
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div className="grid gap-1">
            <h1 className="text-title sm:text-display">{agent.label}</h1>
            <p className="text-muted-foreground">{agent.mandate.name}</p>
          </div>
          <ModeBadge mode={agent.mode} className="h-7 px-3 text-sm" />
        </header>
        {agent.startup === "reconciling" ? (
          <p role="status" className="flex items-center gap-2 rounded-lg bg-muted px-4 py-3 font-medium" data-slot="reconciling">
            <RefreshCw className="size-4" aria-hidden />
            Checking with the broker. Nothing is needed from you.
          </p>
        ) : null}
        <ModeBanner mode={agent.mode} restrictions={agent.restrictions} />
      </div>

      <div className="grid gap-8 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <div className="grid content-start gap-8">
          <Panel>
            <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3">
              <div>
                <dt className="text-caption text-muted-foreground">Equity</dt>
                <dd>
                  <Money value={agent.state.equity} className="text-xl font-semibold" />
                </dd>
              </div>
              <div>
                <dt className="text-caption text-muted-foreground">Paper P&amp;L, simulated</dt>
                <dd>
                  <SignedMoney value={agent.pnl_total} className="text-xl font-semibold" />
                </dd>
              </div>
              <div>
                <dt className="text-caption text-muted-foreground">Today</dt>
                <dd>
                  <SignedMoney value={agent.pnl_today} className="text-xl font-semibold" />
                </dd>
              </div>
            </dl>
            <p className="mt-3 flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
              <Placeholder name="performance" />
              {marketStale && agent.positions[0] ? <AsOf at={agent.positions[0].mark_as_of} now={now} stale /> : null}
            </p>
          </Panel>

          <Section title="Limits in dollars">
            <Envelope agent={agent} />
          </Section>

          <Section title="Positions">
            <Panel>
              <PositionsTable positions={agent.positions} now={now} staleSymbols={marketStale ? new Set(agent.positions.map((p) => p.instrument.symbol)) : staleSymbols} />
            </Panel>
          </Section>

          <Section title="Working orders">
            <Panel className="py-1 sm:py-1">
              <OrdersTable orders={agent.orders} />
            </Panel>
          </Section>
        </div>

        <div className="grid content-start gap-8">
          {open.length > 0 ? (
            <Section title="Waiting for you">
              <ul className="grid gap-2">
                {open.map((a) => (
                  <li key={a.approval_id}>
                    <Link href={`/approvals/${a.approval_id}`} className="press grid gap-1 rounded-xl border bg-card p-4 shadow-whisper hover:border-primary/50">
                      <span className="font-medium">
                        Buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of <span className="font-mono tabular">{price(a.bound.limit)}</span>
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
            <Panel className="py-1 sm:py-1">
              {decisions.length === 0 ? (
                <p className="py-3 text-sm text-muted-foreground">No decisions yet.</p>
              ) : (
                <ul className="divide-y divide-border/60">
                  {decisions.map((d) => (
                    <GateDecisionRow key={d.event_id} decision={d} agent={agent} />
                  ))}
                </ul>
              )}
            </Panel>
          </Section>

          <Section title="Timeline">
            <Panel>
              <Timeline events={ws.timeline[agent.agent_id] ?? []} today={ws.now.slice(0, 10)} />
            </Panel>
          </Section>
        </div>
      </div>
    </div>
  );
}

export function AgentDetailScreen({ agentId }: { agentId: string }) {
  return (
    <WorkspaceGate>
      <AgentDetail agentId={agentId} />
    </WorkspaceGate>
  );
}
