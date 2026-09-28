"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { AsOf } from "@/components/domain/as-of";
import { LimitRail } from "@/components/domain/envelope";
import { MODE_FIELD, SOURCE_FIELD, SourceTag } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import type { Agent } from "@/fixtures/types";
import { quantity, usd } from "@/lib/format";
import { MODE_LABEL } from "@/lib/labels";
import { agentLimits } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";

/**
 * An agent as a sign band: its mode field (read from across the room), its identity and money on
 * a card field, and your mandate on marigold. Restrictions follow on their own fields, each in the
 * colour of whoever imposed it. Fields sit on seams, never inside one another.
 */
export function AgentCard({
  agent,
  now,
  marketStale,
  index = 0,
  heading: Heading = "h3",
}: {
  agent: Agent;
  now: string;
  marketStale: boolean;
  index?: number;
  heading?: "h2" | "h3";
}) {
  const limits = agentLimits(agent);
  const rails = limits.rails.filter((r) => r.key === "gross" || r.key === "daily");
  const markAt = agent.positions[0]?.mark_as_of;
  return (
    <article
      aria-labelledby={`agent-${agent.agent_id}`}
      data-mode={agent.mode}
      className="reveal grid grid-cols-1 gap-(--seam) md:grid-cols-[8.5rem_minmax(0,1fr)_minmax(0,1.1fr)]"
      style={{ "--i": Math.min(index, 6) } as CSSProperties}
    >
      <div data-slot="mode-field" className={cn("flex items-center px-3 py-1.5 transition-colors duration-(--duration-hover) sm:px-4 md:items-end md:py-3", MODE_FIELD[agent.mode])}>
        <p className="font-display text-lg leading-[0.9] font-extrabold uppercase md:text-[1.625rem]">{MODE_LABEL[agent.mode]}</p>
      </div>

      <div className="grid content-start gap-2 bg-card px-3 py-3 sm:px-4">
        <header className="flex flex-wrap items-baseline justify-between gap-x-3">
          <Heading id={`agent-${agent.agent_id}`} className="text-heading">
            <Link href={`/agents/${agent.agent_id}`} className="underline-offset-4 hover:underline">
              {agent.label}
            </Link>
          </Heading>
          <span className="text-sm text-muted-foreground">{agent.mandate.name}</span>
        </header>
        <p className="font-display text-[2.5rem] leading-none font-bold">
          <Money value={agent.state.equity} className="font-display" />
        </p>
        <p className="text-sm text-muted-foreground">
          Equity, of <span className="font-mono tabular">{usd(agent.mandate.capital.allocation_usd, 0)}</span> capital
        </p>
        <div className="grid gap-1 border-t pt-2">
          <p className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 text-sm">
            <span>Paper P&amp;L, simulated</span>
            <Placeholder name="performance" />
          </p>
          <p className="flex flex-wrap items-baseline justify-between gap-x-3">
            <SignedMoney value={agent.pnl_total} className="font-bold" />
            <span className="text-caption text-muted-foreground">
              today <SignedMoney value={agent.pnl_today} showWord={false} className="text-caption" />
            </span>
          </p>
          {marketStale && markAt ? <AsOf at={markAt} now={now} stale /> : null}
        </div>
        <p className="text-sm">
          {agent.positions.length === 0 ? (
            "Flat, no positions."
          ) : (
            <>
              Holds{" "}
              {agent.positions.map((p, i) => (
                <span key={p.instrument.asset_id}>
                  {i > 0 ? ", " : null}
                  <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                </span>
              ))}
              .
            </>
          )}
        </p>
      </div>

      <div className="grid content-start gap-3 bg-marigold px-3 py-3 text-marigold-foreground sm:px-4">
        <p className="font-display text-base leading-none font-extrabold uppercase">Your mandate</p>
        {rails.map((rail) => (
          <LimitRail key={rail.key} rail={rail} />
        ))}
      </div>

      {agent.restrictions.length > 0 ? (
        <ul aria-label="Restrictions" className="grid gap-(--seam) md:col-span-3">
          {agent.restrictions.map((r) => {
            const d = describeRestriction(r);
            return (
              <li key={`${r.code}-${r.symbol ?? ""}`} data-source={d.source} className={cn("flex flex-wrap items-baseline gap-x-2 gap-y-1 px-3 py-2 text-sm sm:px-4", SOURCE_FIELD[d.source])}>
                <SourceTag source={d.source} className="self-center" />
                <span>
                  <span className="font-bold">{d.title}.</span> Blocks {d.blocks.toLowerCase()}. Ends when: {d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.
                </span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </article>
  );
}
