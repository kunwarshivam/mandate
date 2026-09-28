"use client";

import Link from "next/link";
import { motion } from "motion/react";
import { AsOf } from "@/components/domain/as-of";
import { LimitRail } from "@/components/domain/envelope";
import { ModeBadge } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import type { Agent } from "@/fixtures/types";
import { quantity, usd } from "@/lib/format";
import { agentLimits } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";

export function AgentCard({ agent, now, marketStale }: { agent: Agent; now: string; marketStale: boolean }) {
  const limits = agentLimits(agent);
  const markAt = agent.positions[0]?.mark_as_of;
  return (
    <motion.article
      layout
      aria-labelledby={`agent-${agent.agent_id}`}
      className="grid content-start gap-4 rounded-xl border bg-card p-4 shadow-whisper sm:p-5"
    >
      <header className="flex items-start justify-between gap-3">
        <div className="grid">
          <h3 id={`agent-${agent.agent_id}`} className="text-heading">
            <Link href={`/agents/${agent.agent_id}`} className="rounded-sm hover:underline hover:underline-offset-4">
              {agent.label}
            </Link>
          </h3>
          <span className="text-caption text-muted-foreground">{agent.mandate.name}</span>
        </div>
        <ModeBadge mode={agent.mode} />
      </header>

      <dl className="grid grid-cols-2 gap-x-4 gap-y-3">
        <div>
          <dt className="text-caption text-muted-foreground">Capital</dt>
          <dd>
            <Money value={agent.mandate.capital.allocation_usd} />
          </dd>
        </div>
        <div className="text-right">
          <dt className="text-caption text-muted-foreground">Equity</dt>
          <dd>
            <Money value={agent.state.equity} className="text-lg font-semibold" />
          </dd>
        </div>
        <div className="col-span-2 grid gap-1 rounded-lg bg-muted/60 px-3 py-2">
          <dt className="flex items-center justify-between gap-2 text-caption text-muted-foreground">
            Paper P&amp;L, simulated <Placeholder name="performance" />
          </dt>
          <dd className="flex flex-wrap items-baseline justify-between gap-x-3">
            <SignedMoney value={agent.pnl_total} className="text-lg font-semibold" />
            <span className="text-caption text-muted-foreground">
              today <SignedMoney value={agent.pnl_today} showWord={false} className="text-caption" />
            </span>
          </dd>
          {marketStale && markAt ? (
            <dd>
              <AsOf at={markAt} now={now} stale />
            </dd>
          ) : null}
        </div>
      </dl>

      {agent.restrictions.length > 0 ? (
        <ul className="grid gap-1.5 rounded-lg border border-notice-border bg-notice px-3 py-2 text-sm" aria-label="Restrictions">
          {agent.restrictions.map((r) => {
            const d = describeRestriction(r);
            return (
              <li key={`${r.code}-${r.symbol ?? ""}`}>
                <span className="font-medium">{d.title}.</span> Blocks {d.blocks.toLowerCase()}. Ends when: {d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.
              </li>
            );
          })}
        </ul>
      ) : null}

      <div className="grid gap-1 text-sm">
        <p className="text-caption text-muted-foreground">Positions</p>
        {agent.positions.length === 0 ? (
          <p>Flat.</p>
        ) : (
          <ul className="grid gap-0.5">
            {agent.positions.map((p) => (
              <li key={p.instrument.asset_id} className="flex justify-between gap-3">
                <span>
                  <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                </span>
                <span className="font-mono tabular">{usd(p.market_value)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="grid gap-3 border-t pt-4">
        {limits.rails
          .filter((r) => r.key === "gross" || r.key === "daily")
          .map((rail) => (
            <LimitRail key={rail.key} rail={rail} />
          ))}
      </div>
    </motion.article>
  );
}
