"use client";

import { motion } from "motion/react";
import { cn } from "@/lib/utils";
import type { Agent } from "@/fixtures/types";
import { type Dec, ONE, ratio, sub } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { type Level, type Rail, agentLimits } from "@/lib/limits";

const EASE = [0.25, 1, 0.5, 1] as const;

/** A limit as a rail: usage runs along the track, the limit is the wall at its end. */
export function LimitRail({ rail, className }: { rail: Rail; className?: string }) {
  const share = ratio(rail.used, rail.cap);
  const over = rail.used > rail.cap;
  return (
    <div className={cn("grid gap-1.5", className)} data-slot="limit-rail">
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span>{rail.label}</span>
        <span className="text-right font-mono tabular text-muted-foreground">
          <span className="text-foreground">{usd(rail.used)}</span> of {usd(rail.cap)}
        </span>
      </div>
      <div
        role="img"
        aria-label={`${rail.label}: ${usd(rail.used)} of a ${usd(rail.cap)} limit`}
        className="relative h-2 overflow-hidden rounded-full bg-muted"
      >
        <motion.div
          className={cn("absolute inset-y-0 left-0 w-full origin-left rounded-full", over ? "bg-persimmon" : "bg-ultramarine")}
          initial={false}
          animate={{ scaleX: share }}
          transition={{ duration: 0.4, ease: EASE }}
        />
        <div className="absolute inset-y-0 right-0 w-[3px] bg-persimmon" aria-hidden />
      </div>
      <p className="text-caption text-muted-foreground">
        {usd(sub(rail.cap, rail.used) > 0n ? sub(rail.cap, rail.used) : 0n)} headroom. At the limit: {rail.atCap.toLowerCase()}.
      </p>
    </div>
  );
}

const TICK: Record<Level["kind"], string> = {
  floor: "bg-persimmon",
  rung: "bg-persimmon",
  daily: "bg-persimmon",
  high_water_mark: "bg-ultramarine",
  profit_stop: "bg-lagoon",
};

/**
 * Equity against the levels where the agent's behaviour changes. `profit_stop` is one level among
 * them, never a progress bar toward a goal (mandate spec §3.1).
 */
export function EquityLevels({ equity, levels }: { equity: Dec; levels: Level[] }) {
  const lo = levels.reduce((m, l) => (l.at < m ? l.at : m), equity);
  const hi = levels.reduce((m, l) => (l.at > m ? l.at : m), equity);
  const span = sub(hi, lo);
  const pos = (v: Dec) => `${(4 + ratio(sub(v, lo), span) * 92).toFixed(2)}%`;
  const rows = [...levels].reverse();
  const equityIndex = rows.findIndex((l) => l.at <= equity);
  const withEquity: Array<Level | "equity"> = [...rows];
  withEquity.splice(equityIndex === -1 ? rows.length : equityIndex, 0, "equity");

  return (
    <div className="grid gap-4">
      <div className="relative h-8" aria-hidden>
        <div className="absolute inset-x-0 top-1/2 h-px bg-border" />
        {levels.map((l) => (
          <span key={l.key} className={cn("absolute top-1/2 h-4 w-[2px] -translate-x-1/2 -translate-y-1/2 rounded-full", TICK[l.kind])} style={{ left: pos(l.at) }} />
        ))}
        <motion.span
          className="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-foreground ring-4 ring-card"
          initial={false}
          animate={{ left: pos(equity) }}
          transition={{ duration: 0.4, ease: EASE }}
        />
      </div>
      <ol className="grid text-sm" aria-label="Equity levels, highest first">
        {withEquity.map((row) =>
          row === "equity" ? (
            <li key="equity" className="my-1 grid grid-cols-[7.5rem_1fr] items-baseline gap-3 rounded-md bg-muted px-2 py-2 sm:grid-cols-[8.5rem_1fr_auto]">
              <span className="text-right font-mono tabular font-semibold">{usd(equity)}</span>
              <span className="font-medium">Equity now</span>
              <span className="hidden text-caption text-muted-foreground sm:block">Fixture value</span>
            </li>
          ) : (
            <li key={row.key} data-level={row.kind} className="grid grid-cols-[7.5rem_1fr] items-baseline gap-3 border-b border-border/60 px-2 py-2 last:border-b-0 sm:grid-cols-[8.5rem_1fr_auto]">
              <span className="text-right font-mono tabular">{usd(row.at)}</span>
              <span className="flex items-center gap-2">
                <span className={cn("h-3 w-[3px] shrink-0 rounded-full", TICK[row.kind])} aria-hidden />
                {row.label}
                {row.reached ? <span className="rounded-full bg-notice px-2 text-[0.6875rem] font-medium text-persimmon-text ring-1 ring-notice-border">Reached</span> : null}
              </span>
              <span className="col-start-2 text-caption text-muted-foreground sm:col-start-auto sm:text-right">{row.action}</span>
            </li>
          ),
        )}
      </ol>
    </div>
  );
}

/** The envelope: an agent's limits in dollars, inside the brand's gradient ring. */
export function Envelope({ agent, compact = false, className }: { agent: Agent; compact?: boolean; className?: string }) {
  const limits = agentLimits(agent);
  const scaled = limits.sizeFactor < ONE;
  return (
    <section aria-label="Limits in dollars" className={cn("mesh drift rounded-xl p-[1.5px]", className)}>
      <div className="grid gap-5 rounded-[calc(var(--radius-xl)-1.5px)] bg-card p-4 sm:p-5">
        {compact ? null : <EquityLevels equity={limits.equity} levels={limits.levels} />}
        <div className="grid gap-4">
          {limits.rails.map((rail) => (
            <LimitRail key={rail.key} rail={rail} />
          ))}
        </div>
        <dl className="grid grid-cols-2 gap-3 text-sm">
          <div>
            <dt className="text-caption text-muted-foreground">Largest order</dt>
            <dd className="font-mono tabular">
              {usd(limits.orderCap)}
              {scaled ? <span className="ml-1 font-sans text-caption text-persimmon-text">sizes scaled</span> : null}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-muted-foreground">Orders today</dt>
            <dd className="font-mono tabular">
              {limits.ordersToday} of {limits.ordersCap}
            </dd>
          </div>
        </dl>
        {agent.state.pending.map((p) => (
          <p key={p.limit} className="text-sm">
            Confirming a breach of {p.limit}: {p.seconds_in_breach} s of {p.confirm_after_s} s.
          </p>
        ))}
        <p className="text-caption text-muted-foreground">
          Each limit acts when it is reached. Gaps, halts, and outages can move prices past any level before an order fills.
        </p>
      </div>
    </section>
  );
}
