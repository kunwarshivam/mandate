"use client";

import Link from "next/link";
import { ArrowLeft, ArrowRight, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
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
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { Panel, Section, WorkspaceGate } from "./common";
import { MandateSummary } from "./mandate-summary";

const BACK = "inline-flex h-11 w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground lg:h-8";

function NotFound() {
  return (
    <section aria-labelledby="missing-title" className="reveal grid max-w-3xl gap-3 border-t-4 border-foreground bg-muted p-4 sm:p-6">
      <h1 id="missing-title" className="text-title sm:text-display">
        No agent with this ID
      </h1>
      <p className="max-w-prose">This workspace has no agent with that ID. It may belong to another workspace.</p>
      <Button asChild variant="outline" size="lg" className="w-fit">
        <Link href="/agents">See all agents</Link>
      </Button>
    </section>
  );
}

function AgentDetail({ agentId }: { agentId: string }) {
  const { ws, now } = useRuntime();
  const agent = ws.agents.find((a) => a.agent_id === agentId);
  if (!agent) return <NotFound />;
  const staleSymbols = new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
  const marketStale = ws.health.market_data.state !== "ok";
  const open = ws.approvals.filter((a) => a.agent_id === agent.agent_id).map((a) => approvalAt(a, now)).filter((a) => a.status === "delivered");
  const decisions = ws.decisions.filter((d) => d.agent_id === agent.agent_id);
  const markAt = agent.positions[0]?.mark_as_of;

  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <div className="grid grid-cols-1 gap-(--seam)">
        <Link href="/agents" className={BACK}>
          <ArrowLeft className="size-4" aria-hidden />
          Agents
        </Link>
        <header data-mode={agent.mode} className="reveal grid grid-cols-1 gap-(--seam) md:grid-cols-[11rem_minmax(0,1fr)]">
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
            <div className="grid gap-1">
              <h1 className="text-title sm:text-display">{agent.label}</h1>
              <p className="text-muted-foreground">{agent.mandate.name}</p>
            </div>
            {agent.startup === "reconciling" ? (
              <p role="status" data-slot="reconciling" className="flex items-center gap-2 font-bold">
                <RefreshCw className="size-4 shrink-0" aria-hidden />
                Checking with the broker. Nothing is needed from you.
              </p>
            ) : null}
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 border-t pt-3 sm:grid-cols-[auto_auto_auto] sm:justify-start sm:gap-x-8">
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
        </header>
        <ModeBanner mode={agent.mode} restrictions={agent.restrictions} showMode={false} />
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
