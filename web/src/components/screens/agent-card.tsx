"use client";

import { type CSSProperties, useMemo } from "react";
import Link from "next/link";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { cn } from "@/lib/utils";
import { useMarket } from "@/components/charts/chart-parts";
import { Sparkline } from "@/components/charts/sparkline";
import { AsOf } from "@/components/domain/as-of";
import { AgentOwl } from "@/components/domain/owl";
import { ModeBadge } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import type { Agent } from "@/fixtures/types";
import { equityWindow, mandateLevels } from "@/lib/chart-data";
import { RECORD_ZONE, datedClock, quantity, usd } from "@/lib/format";
import { headroomLine } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";

function holdings(agent: Agent): string {
  if (agent.positions.length === 0) return "Flat";
  return agent.positions.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(", ");
}

/** Said once over a list of agent rows, not on every row: their P&L is on paper (DEC-467). */
export function PaperPnlNote({ className }: { className?: string }) {
  return (
    <p data-slot="paper-note" className={cn("text-caption text-muted-foreground", className)}>
      Paper P&amp;L, simulated.
    </p>
  );
}

/**
 * An agent as one calm row: name and mode, what it holds, today's line against the daily loss
 * limit, and its equity with today's change. The P&L since deployed, its disclosure and the headroom
 * to the next level sit right under it; restrictions follow, each saying what it blocks and how it
 * ends. The whole row opens the agent. Its list carries the one paper note, `PaperPnlNote`.
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
  const markAt = agent.positions[0]?.mark_as_of;
  const market = useMarket();
  const today = useMemo(() => equityWindow(market.equity[agent.agent_id] ?? [], "1D", market.end), [market, agent.agent_id]);
  const daily = mandateLevels(agent).find((l) => l.key === "daily");
  return (
    <article
      aria-labelledby={`agent-${agent.agent_id}`}
      data-mode={agent.mode}
      data-slot="agent-band"
      className="group reveal relative -mx-3 grid grid-cols-[2rem_minmax(0,1fr)] gap-x-3 gap-y-2.5 rounded-2xl px-3 py-4 transition-[background-color,scale] duration-(--duration-hover) ease-(--ease-out) hover:bg-background has-[a:active]:scale-[0.99] sm:grid-cols-[3rem_minmax(0,1fr)] sm:gap-x-4"
      style={{ "--i": index } as CSSProperties}
    >
      <AgentOwl agent={agent} className="row-span-3 size-8 sm:size-12" />
      <div className="grid grid-cols-[minmax(0,1fr)_4.5rem_auto] items-center gap-x-4 sm:grid-cols-[minmax(0,1fr)_12rem_10rem_1rem] sm:gap-x-6">
        <div className="grid min-w-0 gap-1">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <Heading id={`agent-${agent.agent_id}`} className="text-h3">
              <Link href={`/agents/${agent.agent_id}`} className={cn("underline-offset-4 group-hover:underline after:rounded-2xl", STRETCHED_LINK)}>
                {agent.label}
              </Link>
            </Heading>
            <ModeBadge mode={agent.mode} />
          </div>
          <p className="grid min-w-0 text-sm text-muted-foreground sm:block sm:truncate">
            <span className="truncate">
              {agent.mandate.name}
              <span className="hidden sm:inline"> · </span>
            </span>
            <span className="font-mono break-words tabular">{holdings(agent)}</span>
          </p>
        </div>
        <div className="min-w-0">
          {today.length > 1 ? (
            <Sparkline
              points={today}
              limit={daily?.price ?? null}
              label={`${agent.label} equity today${daily ? `, against the daily loss limit at ${usd(daily.price.toFixed(2))}` : ""}`}
              className="h-10 sm:h-12"
            />
          ) : null}
        </div>
        <div className="grid justify-items-end gap-0.5 text-right tabular">
          <p className="text-base font-medium sm:text-lg">
            <Money value={agent.state.equity} />
          </p>
          <p className="text-sm">
            <SignedMoney value={agent.pnl_today} showWord={false} /> <span className="text-muted-foreground">today</span>
          </p>
        </div>
        <ChevronRight aria-hidden className="hidden size-6 text-muted-foreground transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5 sm:block" />
      </div>

      <p className="col-start-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
        <SignedMoney value={agent.pnl_total} className="text-caption" />
        <span className="inline-flex items-center gap-x-2 whitespace-nowrap">
          since deployed
          <Placeholder name="performance" />
        </span>
        <span aria-hidden>·</span>
        <span data-slot="card-headroom" className="tabular">
          {headroomLine(agent)}
        </span>
        {marketStale && markAt ? <AsOf at={markAt} now={now} stale /> : null}
      </p>

      {agent.restrictions.length > 0 ? (
        <p aria-label="Restrictions" className="col-start-2 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-caption text-muted-foreground">
          {agent.restrictions.map((r, i) => {
            const d = describeRestriction(r);
            return (
              <span key={`${r.code}-${r.symbol ?? ""}`} data-source={d.source} className="inline-flex items-center gap-x-1.5">
                {i > 0 ? <span aria-hidden>·</span> : null}
                <span title={`Blocks ${d.blocks.toLowerCase()}. Ends when: ${d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.`}>
                  <span className="font-medium text-foreground">{d.title}</span> since {datedClock(r.since, now, RECORD_ZONE)}
                </span>
              </span>
            );
          })}
        </p>
      ) : null}
    </article>
  );
}
