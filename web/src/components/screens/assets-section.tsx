"use client";

import Link from "next/link";
import { AgentOwl } from "@/components/domain/owl";
import { SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import type { Agent, Workspace } from "@/fixtures/types";
import { add, dec, toFixed } from "@/lib/decimal";
import { percent, quantity, usd } from "@/lib/format";
import { type Slice, type SliceTone, accountSlices, agentSlices, assetTones, mandateUniverse } from "@/lib/holdings";
import { positionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { Section, SectionLink } from "./common";

const FILL: Record<SliceTone, string> = {
  "series-1": "bg-series-1",
  "series-2": "bg-series-2",
  "series-3": "bg-series-3",
  "series-4": "bg-series-4",
  "series-5": "bg-series-5",
  muted: "bg-muted",
};

/** Shares as one flat bar, a hairline of the page between parts. Decorative: the list beside it says the same in words. */
function ShareBar({ slices, className }: { slices: Slice[]; className?: string }) {
  return (
    <div aria-hidden data-slot="share-bar" className={cn("flex h-3 w-full gap-0.5 overflow-hidden rounded-xs", className)}>
      {slices.map((s) => (
        <span key={s.key} data-tone={s.tone} className={cn("h-full min-w-1 first:rounded-l-xs last:rounded-r-xs", FILL[s.tone])} style={{ flexGrow: Number(s.share), flexBasis: 0 }} />
      ))}
    </div>
  );
}

const CHIP = "inline-flex h-5 items-center rounded-sm px-1.5 font-mono text-label outline-none";

function Swatch({ tone }: { tone: SliceTone }) {
  return <span aria-hidden className={cn("size-2.5 shrink-0 rounded-xs", FILL[tone], tone === "muted" && "ring-1 ring-foreground/25 ring-inset")} />;
}

function AccountAssets({ ws }: { ws: Workspace }) {
  const { total, slices } = accountSlices(ws);
  return (
    <div data-slot="account-assets" className="grid content-start gap-4">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-h3">Across the account</h3>
        <p className="font-mono text-sm text-muted-foreground tabular">{usd(total)}</p>
      </div>
      <ShareBar slices={slices} />
      <table className="w-full text-sm">
        <caption className="sr-only">What the account holds, by share of its equity</caption>
        <thead className="sr-only">
          <tr>
            <th scope="col">Holding</th>
            <th scope="col">Share</th>
            <th scope="col">Value</th>
          </tr>
        </thead>
        <tbody>
          {slices.map((s) => (
            <tr key={s.key} data-slot="asset-row" data-kind={s.kind} className="border-b border-border/70 last:border-b-0">
              <th scope="row" className="py-2.5 text-left font-normal">
                <span className="flex items-center gap-2.5">
                  <Swatch tone={s.tone} />
                  <span className={cn(s.kind === "asset" ? "font-medium" : "text-muted-foreground")}>{s.label}</span>
                  {s.agents.length > 0 ? <span className="truncate text-caption text-muted-foreground">{s.agents.join(", ")}</span> : null}
                </span>
                {s.kind === "unmanaged" && ws.external_positions.length > 0 ? (
                  <span className="block pl-5 text-caption text-muted-foreground">
                    Cash and {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}
                  </span>
                ) : null}
              </th>
              <td className="w-16 py-2.5 text-right font-mono text-muted-foreground tabular">{percent(s.share)}</td>
              <td className="w-28 py-2.5 text-right font-mono tabular">{usd(s.value)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function AgentAssets({ agent, tones }: { agent: Agent; tones: Map<string, SliceTone> }) {
  const slices = agentSlices(agent, tones);
  const { instruments, classes } = mandateUniverse(agent);
  const held = new Map(agent.positions.map((p) => [p.instrument.symbol, p.instrument.asset_id]));
  return (
    <li data-slot="agent-assets" className="group relative grid grid-cols-[2rem_minmax(0,1fr)] gap-x-3 gap-y-2 border-b border-border/70 py-3.5 last:border-b-0">
      <AgentOwl agent={agent} className="row-span-3 size-8" />
      <div className="flex items-baseline justify-between gap-3">
        <Link href={`/agents/${agent.agent_id}`} className={cn("font-semibold underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
          {agent.label}
        </Link>
        <span className="font-mono text-sm tabular">{usd(agent.state.equity)}</span>
      </div>
      <ShareBar slices={slices} className="h-2" />
      <p className="flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground" data-slot="mandate-universe">
        <span>Its mandate:</span>
        {instruments.length > 0
          ? instruments.map((i) => {
              const assetId = held.get(i.symbol);
              return assetId ? (
                <Link key={i.symbol} href={positionHref(agent.agent_id, assetId)} data-held="" className={cn(CHIP, "relative bg-mandate text-mandate-strong underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring")}>
                  {i.symbol}
                  <span className="sr-only">, held: open the position</span>
                </Link>
              ) : (
                <span key={i.symbol} className={cn(CHIP, "text-mandate-strong ring-1 ring-mandate-edge ring-inset")}>
                  {i.symbol}
                  <span className="sr-only">, not held</span>
                </span>
              );
            })
          : classes.map((c) => (
              <span key={c} className={cn(CHIP, "bg-mandate font-sans text-mandate-strong")}>
                {c}
              </span>
            ))}
        {instruments.length === 0
          ? agent.positions.map((p) => (
              <Link key={p.instrument.asset_id} href={positionHref(agent.agent_id, p.instrument.asset_id)} data-held="" className={cn(CHIP, "relative bg-mandate text-mandate-strong underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring")}>
                {p.instrument.symbol}
                <span className="sr-only">, held: open the position</span>
              </Link>
            ))
          : null}
        <span className="sr-only">
          {slices.map((s) => `${s.label} ${percent(s.share)}`).join(", ")}
        </span>
      </p>
    </li>
  );
}

/**
 * What the account is made of: every instrument the agents hold, their cash and what no agent
 * manages, as one bar and a list; then each agent's split, with the instruments its mandate lets it
 * trade, held ones filled. Desktop only, like the positions it replaces (DEC-207 keeps a phone's Home short).
 */
export function AssetsSection({ ws, className }: { ws: Workspace; className?: string }) {
  const tones = assetTones(ws);
  const positions = ws.agents.flatMap((a) => a.positions);
  return (
    <Section title="Assets" action={<SectionLink href="/positions">All positions</SectionLink>} className={className}>
      <div className="grid grid-cols-1 gap-10 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-x-16">
        <AccountAssets ws={ws} />
        <div className="grid content-start gap-2">
          <h3 className="text-h3">By agent</h3>
          <ul className="grid">
            {ws.agents.map((a) => (
              <AgentAssets key={a.agent_id} agent={a} tones={tones} />
            ))}
          </ul>
        </div>
      </div>
      {positions.length > 0 ? (
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
          Unrealized paper P&amp;L across holdings, simulated:
          <SignedMoney value={toFixed(add(...positions.map((p) => dec(p.unrealized_pnl))), 2)} className="text-caption" />
          <Placeholder name="performance" />
        </p>
      ) : null}
    </Section>
  );
}
