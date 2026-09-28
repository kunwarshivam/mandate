"use client";

import Link from "next/link";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { Deadline } from "@/components/approvals/deadline";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { SignedMoney } from "@/components/domain/money";
import { findAgent } from "@/fixtures/workspace";
import { price, quantity, usd } from "@/lib/format";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { AgentCard } from "./agent-card";
import { Panel, Section, WorkspaceGate } from "./common";

const stagger = {
  hidden: {},
  shown: { transition: { staggerChildren: 0.04 } },
};
const rise = {
  hidden: { opacity: 0, y: 6 },
  shown: { opacity: 1, y: 0, transition: { duration: 0.24, ease: [0.25, 1, 0.5, 1] as const } },
};

function EmptyWorkspace() {
  return (
    <section
      className="envelope grid min-h-[22rem] place-items-end rounded-2xl p-3 pr-[calc(var(--envelope-wall)+0.75rem)] sm:p-4 sm:pr-[calc(var(--envelope-wall)+1rem)]"
      aria-labelledby="empty-title"
    >
      <div className="grid w-full max-w-xl gap-4 rounded-xl bg-card p-5 sm:p-6">
        <h1 id="empty-title" className="text-title">
          No agents yet
        </h1>
        <p className="text-muted-foreground">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
        <Button asChild size="lg" className="press h-11 w-fit px-5 text-base">
          <Link href="/agents/new">Describe your first agent</Link>
        </Button>
      </div>
    </section>
  );
}

function Dashboard() {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) return <EmptyWorkspace />;
  const open = ws.approvals.map((a) => approvalAt(a, now)).filter((a) => a.status === "delivered").sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const marketStale = ws.health.market_data.state !== "ok";
  const active = ws.agents.filter((a) => a.mode !== "stopped").length;
  const positions = ws.agents.flatMap((a) => a.positions.map((p) => ({ agent: a, p })));

  return (
    <div className="grid gap-10">
      <section
        className="envelope rounded-2xl p-2.5 pr-[calc(var(--envelope-wall)+0.625rem)] sm:p-3 sm:pr-[calc(var(--envelope-wall)+0.75rem)]"
        aria-labelledby="dashboard-title"
      >
        <div className="grid gap-4 rounded-xl bg-card p-5 sm:grid-cols-[1fr_auto] sm:items-end sm:p-6">
          <div className="grid gap-2">
            <h1 id="dashboard-title" className="text-title sm:text-display">
              Dashboard
            </h1>
            <p className="text-muted-foreground">
              {ws.agents.length} {ws.agents.length === 1 ? "agent" : "agents"} on {ws.connection.broker}, {active} running.{" "}
              {open.length === 0 ? "Nothing is waiting for you." : `${open.length} ${open.length === 1 ? "request is" : "requests are"} waiting for you.`}
            </p>
          </div>
          {open.length > 0 ? (
            <Button asChild variant="outline" size="lg" className="press h-10 w-fit px-4">
              <Link href="/approvals">Open approvals</Link>
            </Button>
          ) : null}
        </div>
      </section>

      {open.length > 0 ? (
        <Section title="Waiting for you">
          <ul className="grid gap-2">
            {open.map((a) => {
              const agent = findAgent(ws, a.agent_id);
              return (
                <li key={a.approval_id}>
                  <Link
                    href={`/approvals/${a.approval_id}`}
                    className="press grid gap-1 rounded-xl border bg-card p-4 shadow-whisper hover:border-primary/50 sm:grid-cols-[1fr_auto] sm:items-center sm:gap-4"
                  >
                    <span className="font-medium">
                      {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at a limit of{" "}
                      <span className="font-mono tabular">{price(a.bound.limit)}</span>
                    </span>
                    <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                  </Link>
                </li>
              );
            })}
          </ul>
        </Section>
      ) : null}

      <Section title="Agents">
        <motion.div variants={stagger} initial="hidden" animate="shown" className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {ws.agents.map((agent) => (
            <motion.div key={agent.agent_id} variants={rise} className="grid">
              <AgentCard agent={agent} now={now} marketStale={marketStale} />
            </motion.div>
          ))}
        </motion.div>
      </Section>

      <div className="grid gap-10 lg:grid-cols-[3fr_2fr]">
        <Section title="Recent gate decisions">
          <Panel className="py-1 sm:py-1">
            {ws.decisions.length === 0 ? (
              <p className="py-3 text-sm text-muted-foreground">No decisions yet.</p>
            ) : (
              <ul className="divide-y divide-border/60">
                {ws.decisions.slice(0, 6).map((d) => (
                  <GateDecisionRow key={d.event_id} decision={d} agent={findAgent(ws, d.agent_id)} showAgent />
                ))}
              </ul>
            )}
          </Panel>
        </Section>

        <Section title="Positions">
          <Panel>
            {positions.length === 0 ? (
              <p className="text-sm text-muted-foreground">No agent holds a position.</p>
            ) : (
              <table className="w-full text-sm">
                <caption className="sr-only">Positions across agents</caption>
                <thead>
                  <tr className="border-b text-left text-caption text-muted-foreground">
                    <th scope="col" className="pb-2 font-medium">Holding</th>
                    <th scope="col" className="pb-2 text-right font-medium">Value</th>
                    <th scope="col" className="pb-2 text-right font-medium">Unrealized</th>
                  </tr>
                </thead>
                <tbody>
                  {positions.map(({ agent, p }) => (
                    <tr key={`${agent.agent_id}-${p.instrument.asset_id}`} className="border-b border-border/60 last:border-b-0">
                      <th scope="row" className="py-2.5 text-left font-normal">
                        <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                        <span className="block text-caption text-muted-foreground">{agent.label}</span>
                      </th>
                      <td className="py-2.5 text-right font-mono tabular">{usd(p.market_value)}</td>
                      <td className="py-2.5 text-right">
                        <SignedMoney value={p.unrealized_pnl} showWord={false} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {ws.external_positions.length > 0 ? (
              <p className="mt-4 border-t pt-3 text-caption text-muted-foreground">
                Also on the account, not managed by any agent:{" "}
                {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
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
