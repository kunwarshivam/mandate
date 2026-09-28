"use client";

import { motion, useReducedMotion } from "motion/react";
import { cn } from "@/lib/utils";
import type { Agent } from "@/fixtures/types";
import { type Dec, ONE, ratio, sub } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { type Level, type Rail, agentLimits } from "@/lib/limits";

const EASE = [0.23, 1, 0.32, 1] as const;

/** MotionConfig's reducedMotion covers named transform keys only, not a raw `transform` string. */
function useMove() {
  return { duration: useReducedMotion() ? 0 : 0.24, ease: EASE };
}

/**
 * A limit as a rail on the marigold mandate field: usage fills in ink toward the limit, an ink
 * post where the agent stops. It always sits on marigold, the colour of your mandate.
 */
export function LimitRail({ rail, className }: { rail: Rail; className?: string }) {
  const share = Math.min(ratio(rail.used, rail.cap), 1);
  const over = rail.used > rail.cap;
  const move = useMove();
  return (
    <div className={cn("grid gap-1.5", className)} data-slot="limit-rail" data-over={over ? "" : undefined}>
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span className="font-bold">{rail.label}</span>
        <span className="text-right font-mono tabular">
          <span className="font-bold">{usd(rail.used)}</span> <span className="text-marigold-muted">of {usd(rail.cap)}</span>
        </span>
      </div>
      <div role="img" aria-label={`${rail.label}: ${usd(rail.used)} of a ${usd(rail.cap)} limit`} className="relative h-3 bg-marigold-foreground/15">
        <motion.div
          className="absolute inset-y-0 left-0 w-full origin-left bg-marigold-foreground"
          initial={false}
          animate={{ transform: `scaleX(${share})` }}
          transition={move}
        />
        <div className="absolute -inset-y-[3px] right-0 w-1 bg-marigold-foreground" aria-hidden />
      </div>
      <p className="text-caption text-marigold-muted">
        {over ? <span className="font-bold text-marigold-foreground">Over the limit. </span> : null}
        {usd(sub(rail.cap, rail.used) > 0n ? sub(rail.cap, rail.used) : 0n)} headroom. At the limit: {rail.atCap.toLowerCase()}.
      </p>
    </div>
  );
}

const TICK: Record<Level["kind"], string> = {
  floor: "h-5 w-1",
  rung: "h-4 w-[3px]",
  daily: "h-4 w-[3px]",
  high_water_mark: "h-3 w-[2px]",
  profit_stop: "h-3 w-[2px]",
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
  const move = useMove();
  withEquity.splice(equityIndex === -1 ? rows.length : equityIndex, 0, "equity");

  return (
    <div className="grid gap-4">
      <div className="relative h-8" aria-hidden>
        <div className="absolute inset-x-0 top-1/2 h-0.5 bg-marigold-foreground/30" />
        {levels.map((l) => (
          <span key={l.key} className={cn("absolute top-1/2 -translate-x-1/2 -translate-y-1/2 bg-marigold-foreground", TICK[l.kind])} style={{ left: pos(l.at) }} />
        ))}
        <motion.span
          className="absolute inset-0"
          initial={false}
          animate={{ transform: `translateX(${pos(equity)})` }}
          transition={move}
        >
          <span className="absolute top-1/2 left-0 size-3.5 -translate-x-1/2 -translate-y-1/2 bg-lapis ring-2 ring-marigold" />
        </motion.span>
      </div>
      <ol className="grid text-sm" aria-label="Equity levels, highest first">
        {withEquity.map((row) =>
          row === "equity" ? (
            <li key="equity" className="my-1 grid grid-cols-[7.5rem_1fr] items-baseline gap-3 bg-lapis px-2 py-2 text-lapis-foreground sm:grid-cols-[8.5rem_1fr_auto]">
              <span className="text-right font-mono font-bold tabular">{usd(equity)}</span>
              <span className="font-bold">Equity now</span>
              <span className="hidden text-caption text-lapis-muted sm:block">Fixture value</span>
            </li>
          ) : (
            <li key={row.key} data-level={row.kind} className="grid grid-cols-[7.5rem_1fr] items-baseline gap-3 border-b border-marigold-foreground/25 px-2 py-2 last:border-b-0 sm:grid-cols-[8.5rem_1fr_auto]">
              <span className="text-right font-mono tabular">{usd(row.at)}</span>
              <span className="flex items-center gap-2">
                <span className={cn("shrink-0 bg-marigold-foreground", TICK[row.kind])} aria-hidden />
                {row.label}
                {row.reached ? <span className="bg-ink px-1.5 label-caps text-ink-foreground">Reached</span> : null}
              </span>
              <span className="col-start-2 text-caption text-marigold-muted sm:col-start-auto sm:text-right">{row.action}</span>
            </li>
          ),
        )}
      </ol>
    </div>
  );
}

/**
 * The envelope: an agent's limits in dollars on one marigold field, the colour of your mandate.
 * Everything inside sits directly on the field; nothing is boxed inside it.
 */
export function Envelope({ agent, compact = false, className }: { agent: Agent; compact?: boolean; className?: string }) {
  const limits = agentLimits(agent);
  const scaled = limits.sizeFactor < ONE;
  return (
    <section aria-labelledby={`envelope-${agent.agent_id}`} data-slot="envelope" className={cn("grid gap-5 bg-marigold p-4 text-marigold-foreground", className)}>
      <h2 id={`envelope-${agent.agent_id}`} className="text-heading">
        Your mandate
      </h2>
      {compact ? null : <EquityLevels equity={limits.equity} levels={limits.levels} />}
      <div className="grid gap-4">
        {limits.rails.map((rail) => (
          <LimitRail key={rail.key} rail={rail} />
        ))}
      </div>
      <dl className="grid grid-cols-2 gap-3 text-sm">
        <div>
          <dt className="text-marigold-muted">Largest order</dt>
          <dd className="font-mono font-bold tabular">
            {usd(limits.orderCap)}
            {scaled ? <span className="ml-1.5 bg-ink px-1.5 font-sans label-caps text-ink-foreground">Sizes scaled</span> : null}
          </dd>
        </div>
        <div>
          <dt className="text-marigold-muted">Orders today</dt>
          <dd className="font-mono font-bold tabular">
            {limits.ordersToday} of {limits.ordersCap}
          </dd>
        </div>
      </dl>
      {agent.state.pending.map((p) => (
        <p key={p.limit} className="text-sm font-bold">
          Confirming a breach of {p.limit}: {p.seconds_in_breach} s of {p.confirm_after_s} s.
        </p>
      ))}
      <p className="text-caption text-marigold-muted">Each limit acts when it is reached. Gaps, halts, and outages can move prices past any level before an order fills.</p>
    </section>
  );
}
