"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Reload } from "pixelarticons/react/Reload.js";
import { cn } from "@/lib/utils";
import type { Agent } from "@/fixtures/types";
import { type Dec, ONE, ratio, sub } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { MODE_MEANING } from "@/lib/labels";
import { type Level, type Rail, agentLimits, headroomRows, nextLevel } from "@/lib/limits";
import { agentHref } from "@/lib/screens";
import { ModeBadge } from "./mode";
import { Placeholder } from "./placeholders";

const EASE = [0.23, 1, 0.32, 1] as const;

/** MotionConfig's reducedMotion covers named transform keys only, not a raw `transform` string. */
function useMove() {
  return { duration: useReducedMotion() ? 0 : 0.24, ease: EASE };
}

/**
 * A limit as a rail on the mandate field: usage fills in the marker colour toward the limit, a
 * fine post where the agent stops. It always sits on the mandate field, the colour of your mandate.
 */
export function LimitRail({ rail, caption, brief = false, className }: { rail: Rail; caption?: ReactNode; brief?: boolean; className?: string }) {
  const share = Math.min(ratio(rail.used, rail.cap), 1);
  const over = rail.used > rail.cap;
  const move = useMove();
  const headroom = `${usd(sub(rail.cap, rail.used) > 0n ? sub(rail.cap, rail.used) : 0n)} headroom.`;
  return (
    <div className={cn("grid gap-1.5", className)} data-slot="limit-rail" data-over={over ? "" : undefined}>
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span className="font-medium">{rail.label}</span>
        <span className="text-right font-mono tabular">
          <span className="font-medium">{usd(rail.used)}</span> <span className="text-mandate-muted">of {usd(rail.cap)}</span>
        </span>
      </div>
      <div role="img" aria-label={`${rail.label}: ${usd(rail.used)} of a ${usd(rail.cap)} limit`} className="relative h-2 rounded-xs bg-mandate-marker/15">
        <motion.div
          className="absolute inset-y-0 left-0 w-full origin-left rounded-xs bg-mandate-marker"
          initial={false}
          animate={{ transform: `scaleX(${share})` }}
          transition={move}
        />
        <div className="absolute -inset-y-1 right-0 w-0.5 rounded-xs bg-mandate-strong" aria-hidden />
      </div>
      <p className="text-caption text-mandate-muted">
        {over ? <span className="font-semibold text-mandate-strong">Over the limit. </span> : null}
        {caption ?? (brief ? headroom : `${headroom} At the limit: ${rail.atCap.toLowerCase()}.`)}
      </p>
    </div>
  );
}

const TICK: Record<Level["kind"], string> = {
  floor: "h-5 w-[3px]",
  rung: "h-4 w-[2px]",
  daily: "h-4 w-[2px]",
  high_water_mark: "h-3 w-px",
  profit_stop: "h-3 w-px",
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
      <div className="relative h-8 overflow-hidden" aria-hidden>
        <div className="absolute inset-x-0 top-1/2 h-px bg-mandate-marker/40" />
        {levels.map((l) => (
          <span key={l.key} className={cn("absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-xs bg-mandate-marker", TICK[l.kind])} style={{ left: pos(l.at) }} />
        ))}
        <motion.span
          className="absolute inset-0"
          initial={false}
          animate={{ transform: `translateX(${pos(equity)})` }}
          transition={move}
        >
          <span className="absolute top-1/2 left-0 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-lapis ring-3 ring-mandate" />
        </motion.span>
      </div>
      <ol className="grid text-sm" aria-label="Equity levels, highest first">
        {withEquity.map((row) =>
          row === "equity" ? (
            <li key="equity" data-level="equity" className="my-1 grid grid-cols-[6.5rem_1fr] items-baseline gap-3 rounded-lg bg-card px-2.5 py-2 ring-1 ring-mandate-edge ring-inset">
              <span className="text-right font-mono font-semibold tabular">{usd(equity)}</span>
              <span className="flex items-center gap-2 font-semibold">
                <span className="size-3 shrink-0 rounded-full bg-lapis ring-3 ring-mandate" aria-hidden />
                Equity now
              </span>
            </li>
          ) : (
            <li key={row.key} data-level={row.kind} className="grid grid-cols-[6.5rem_1fr] items-baseline gap-x-3 gap-y-0.5 border-b border-mandate-strong/15 px-2.5 py-2 last:border-b-0">
              <span className="text-right font-mono tabular">{usd(row.at)}</span>
              <span className="flex items-center gap-2">
                <span className={cn("shrink-0 bg-mandate-marker", TICK[row.kind])} aria-hidden />
                {row.label}
                {row.reached ? <span className="rounded-md bg-ink px-2 text-label text-ink-foreground">Reached</span> : null}
              </span>
              <span className="col-start-2 text-caption text-mandate-muted">{row.action}</span>
            </li>
          ),
        )}
      </ol>
    </div>
  );
}

const CAVEAT = "Each limit acts when it is reached. Gaps, halts, and outages can move prices past any level before an order fills.";

function PendingBreaches({ agent }: { agent: Agent }) {
  return agent.state.pending.map((p) => (
    <p key={p.limit} className="text-sm font-medium">
      Confirming a breach of {p.limit}: {p.seconds_in_breach} s of {p.confirm_after_s} s.
    </p>
  ));
}

/**
 * The envelope: an agent's limits in dollars on one pale azure field, the colour of your mandate.
 * Everything inside sits directly on the field; nothing is boxed inside it. On a wide screen the
 * levels and the rails sit side by side, so the field reads across rather than down.
 */
export function Envelope({ agent, className }: { agent: Agent; className?: string }) {
  const limits = agentLimits(agent);
  const scaled = limits.sizeFactor < ONE;
  return (
    <section aria-labelledby={`envelope-${agent.agent_id}`} data-slot="envelope" className={cn("@container grid gap-5 rounded-2xl bg-mandate px-5 py-5 text-mandate-foreground sm:px-6", className)}>
      <div className="grid gap-1">
        <h2 id={`envelope-${agent.agent_id}`} className="text-h2 text-mandate-strong">
          Your mandate
        </h2>
        <p className="text-sm text-mandate-muted">Limits in dollars, and the levels where the agent&apos;s behaviour changes.</p>
      </div>
      <div className="grid gap-x-10 gap-y-6 @3xl:grid-cols-2">
        <EquityLevels equity={limits.equity} levels={limits.levels} />
        <div className="grid content-start gap-5">
          <div className="grid gap-4">
            {limits.rails.map((rail) => (
              <LimitRail key={rail.key} rail={rail} />
            ))}
            <Placeholder name="performance" className="w-fit" />
          </div>
          <dl className="grid grid-cols-2 gap-3 text-sm">
            <div>
              <dt className="text-mandate-muted">Largest order</dt>
              <dd className="font-mono font-medium tabular">
                {usd(limits.orderCap)}
                {scaled ? <span className="ml-1.5 rounded-md bg-ink px-2 font-sans text-label text-ink-foreground">Sizes scaled</span> : null}
              </dd>
            </div>
            <div>
              <dt className="text-mandate-muted">Orders today</dt>
              <dd className="font-mono font-medium tabular">
                {limits.ordersToday} of {limits.ordersCap}
              </dd>
            </div>
          </dl>
          <PendingBreaches agent={agent} />
        </div>
      </div>
      <p className="text-caption text-mandate-muted">{CAVEAT}</p>
    </section>
  );
}

function HeadroomMeter({ label, share }: { label: string; share: number }) {
  return (
    <div role="img" aria-label={label} data-slot="headroom-meter" className="relative h-1 rounded-xs bg-muted">
      <div
        className="absolute inset-y-0 left-0 w-full origin-left rounded-xs bg-foreground transition-transform duration-(--duration-hover) motion-reduce:transition-none"
        style={{ transform: `scaleX(${share})` }}
      />
      <div className="absolute -inset-y-1 right-0 w-0.5 rounded-xs bg-mandate-strong" aria-hidden />
    </div>
  );
}

/**
 * An agent's limits on a phone, one hairline row each: the room left, the limit, and a thin meter in
 * ink with the mandate's post where the agent stops. Distances to limits, never results, so no gain
 * or loss colour and no disclosure to repeat (DEC-207).
 */
export function Headroom({ agent, className }: { agent: Agent; className?: string }) {
  const limits = agentLimits(agent);
  const scaled = limits.sizeFactor < ONE;
  const titleId = `headroom-${agent.agent_id}`;
  const ordersLeft = Math.max(0, limits.ordersCap - limits.ordersToday);
  return (
    <section aria-labelledby={titleId} data-slot="headroom" className={cn("grid content-start gap-2", className)}>
      <h2 id={titleId} className="text-h2">
        Headroom
      </h2>
      {agent.mode !== "normal" ? <p className="text-sm text-muted-foreground">{MODE_MEANING[agent.mode]}</p> : null}
      {agent.startup === "reconciling" ? (
        <p role="status" data-slot="reconciling" className="flex items-center gap-2 text-sm font-medium">
          <Reload className="size-6 shrink-0 motion-safe:animate-spin motion-safe:[animation-duration:2.4s]" aria-hidden />
          Checking with the broker. Nothing is needed from you.
        </p>
      ) : null}
      <ul className="grid">
        {headroomRows(limits).map((row) => (
          <li key={row.key} data-slot="headroom-row" data-over={row.over ? "" : undefined} className="grid gap-2 border-b border-border/70 py-3">
            <p className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
              <span className="font-medium">{row.label}</span>
              <span className="font-mono text-sm font-medium tabular">{row.headroom} headroom</span>
            </p>
            <HeadroomMeter label={`${row.label}: ${row.headroom} headroom under a limit of ${row.limit}`} share={row.share} />
            <p className="text-caption text-muted-foreground">
              {row.over ? <span className="font-semibold text-foreground">Over the limit. </span> : null}
              Limit <span className="font-mono tabular">{row.limit}</span>. At the limit: {row.atCap.toLowerCase()}.
            </p>
          </li>
        ))}
        <li data-slot="headroom-row" className="grid gap-2 border-b border-border/70 py-3">
          <p className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
            <span className="font-medium">Orders today</span>
            <span className="font-mono text-sm font-medium tabular">{ordersLeft} left</span>
          </p>
          <HeadroomMeter label={`Orders today: ${limits.ordersToday} of ${limits.ordersCap}`} share={Math.min(limits.ordersToday / Math.max(limits.ordersCap, 1), 1)} />
          <p className="text-caption text-muted-foreground">
            Limit <span className="font-mono tabular">{limits.ordersCap}</span> a day. Largest order <span className="font-mono tabular">{usd(limits.orderCap)}</span>.
            {scaled ? <span className="ml-1.5 rounded-md bg-ink px-2 font-sans text-label text-ink-foreground">Sizes scaled</span> : null}
          </p>
        </li>
      </ul>
      <PendingBreaches agent={agent} />
      <p className="text-caption text-muted-foreground">{CAVEAT}</p>
      <Link
        href={agentHref(agent.agent_id, "mandate")}
        className="-mx-2 inline-flex min-h-11 w-fit items-center gap-1 rounded-md px-2 text-sm font-semibold underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
      >
        View full mandate
        <ArrowRight aria-hidden className="size-6" />
      </Link>
    </section>
  );
}

/**
 * The mandate at a glance, for the rail beside an agent's story: its mode, the limit rails in dollars,
 * and the next level where its behaviour changes. The levels ladder and every field live on the
 * Mandate tab. Kept short enough to stay in view on a laptop screen as the page scrolls.
 */
export function MandateCard({ agent, className }: { agent: Agent; className?: string }) {
  const limits = agentLimits(agent);
  const next = nextLevel(limits);
  const scaled = limits.sizeFactor < ONE;
  const titleId = `mandate-card-${agent.agent_id}`;
  return (
    <section aria-labelledby={titleId} data-slot="mandate-card" className={cn("grid gap-4 rounded-2xl bg-mandate px-5 py-5 text-mandate-foreground", className)}>
      <div className="grid gap-2">
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1.5">
          <h2 id={titleId} className="text-h2 text-mandate-strong">
            Your mandate
          </h2>
          <ModeBadge mode={agent.mode} />
        </div>
        <div role="group" aria-label="Mode" data-mode={agent.mode} data-slot="mode-field" className="grid gap-1.5">
          <p className="text-sm text-mandate-muted">{MODE_MEANING[agent.mode]}</p>
          {agent.startup === "reconciling" ? (
            <p role="status" data-slot="reconciling" className="flex items-center gap-2 text-sm font-medium">
              <Reload className="size-6 shrink-0 motion-safe:animate-spin motion-safe:[animation-duration:2.4s]" aria-hidden />
              Checking with the broker. Nothing is needed from you.
            </p>
          ) : null}
        </div>
      </div>

      {next ? (
        <div data-slot="next-level" data-level={next.level.kind} className="grid gap-0.5 border-y border-mandate-strong/15 py-3 text-sm">
          <p className="text-mandate-muted">Next level</p>
          <p className="flex items-baseline justify-between gap-3 font-medium">
            <span>{next.level.label}</span>
            <span className="font-mono tabular">{usd(next.level.at)}</span>
          </p>
          <p className="text-caption text-mandate-muted">
            <span className="font-mono tabular">{usd(next.distance)}</span> {next.side} equity now. At the limit: {next.level.action.toLowerCase()}.
          </p>
        </div>
      ) : null}

      <div className="grid gap-3.5">
        {limits.rails.map((rail) => (
          <LimitRail key={rail.key} rail={rail} brief />
        ))}
      </div>
      {scaled ? (
        <p className="text-sm">
          <span className="rounded-md bg-ink px-2 text-label text-ink-foreground">Sizes scaled</span> Largest order now{" "}
          <span className="font-mono font-medium tabular">{usd(limits.orderCap)}</span>.
        </p>
      ) : null}
      <PendingBreaches agent={agent} />

      <div className="grid gap-2">
        <Placeholder name="performance" className="w-fit" />
        <p className="text-caption text-mandate-muted">{CAVEAT}</p>
      </div>
      <Link
        href={agentHref(agent.agent_id, "mandate")}
        className="-mx-2 inline-flex min-h-9 w-fit items-center gap-1 rounded-md px-2 text-sm font-semibold text-mandate-strong underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
      >
        View full mandate
        <ArrowRight aria-hidden className="size-6" />
      </Link>
    </section>
  );
}
