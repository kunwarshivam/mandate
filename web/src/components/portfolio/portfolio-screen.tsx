"use client";

import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import type { CatalogueInstrument } from "@/api/catalogue";
import type { AgentPnl, Dashboard } from "@/api/portfolio";
import type { AgentDetail } from "@/api/read-models";
import { AccountEquityChart, ApiAccountChart } from "@/components/charts/equity-chart";
import { useMarket } from "@/components/charts/chart-parts";
import { AsOf } from "@/components/domain/as-of";
import { ModeBadge } from "@/components/domain/mode";
import { Money, SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import { STRETCHED_LINK } from "@/components/domain/positions";
import { KEY } from "@/components/kumo/key";
import { PAGE_GRID } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { dec, ratio } from "@/lib/decimal";
import { percent, price, quantity, usd } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import {
  type Breakdown,
  type Holding,
  type HoldingsRead,
  type Portfolio,
  type ServedRead,
  type SliceTone,
  agentName,
  apiPortfolio,
  assetClassWord,
  byAgent,
  byAssetClass,
  byInstrument,
  cashAndInvested,
  fixturePortfolio,
  instrumentTones,
} from "@/lib/portfolio";
import { agentHref } from "@/lib/screens";
import { cn } from "@/lib/utils";

/** How often the agents' positions are read again over the API while the page is open, as the positions screen did. */
const POLL_MS = 5000;

const NOT_REPORTED = "not reported";

/**
 * One read per agent over the API, made on open and every few seconds; an agent not yet answered is
 * absent. Each agent's reads are numbered, and an answer older than one already shown is dropped.
 * Restored from the positions screen #3003 removed.
 */
function usePerAgent<T>(fetchOne: ((agentId: string) => Promise<T>) | null, agents: readonly AgentDetail[]): Readonly<Record<string, T>> {
  const [answers, setAnswers] = useState<Readonly<Record<string, T>>>({});
  const sent = useRef(new Map<string, number>());
  const shown = useRef(new Map<string, number>());
  const ids = agents.map((a) => a.agent_id).join("\n");
  useEffect(() => {
    if (!fetchOne) return;
    let live = true;
    const load = () => {
      for (const id of ids ? ids.split("\n") : []) {
        const n = (sent.current.get(id) ?? 0) + 1;
        sent.current.set(id, n);
        void fetchOne(id).then((answer) => {
          if (!live || n < (shown.current.get(id) ?? 0)) return;
          shown.current.set(id, n);
          setAnswers((all) => ({ ...all, [id]: answer }));
        });
      }
    };
    load();
    const poll = window.setInterval(load, POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [fetchOne, ids]);
  return answers;
}

/** Every agent's own positions (`GET /agents/{id}/positions`), a failed read named as not reported. */
function useHoldingsReads(agents: readonly AgentDetail[]): Readonly<Record<string, HoldingsRead>> {
  const { holdings } = useRuntime();
  const fetchOne = useMemo(
    () => (holdings ? async (id: string): Promise<HoldingsRead> => {
      const answer = await holdings(id);
      return answer.ok ? { ok: true, items: answer.items } : { ok: false, reason: `Not reported: ${answer.reason}.` };
    } : null),
    [holdings],
  );
  return usePerAgent(fetchOne, agents);
}

/** Every agent's gross P&L (`GET /agents/{id}/pnl`, DEC-2413). */
function usePnlReads(agents: readonly AgentDetail[]): Readonly<Record<string, ServedRead<AgentPnl>>> {
  const { portfolio } = useRuntime();
  return usePerAgent(portfolio?.pnl ?? null, agents);
}

/** The accounts' latest snapshots (`GET /dashboard`, DEC-2413), read on open and every few seconds; absent until it answers. */
function useDashboard(): ServedRead<Dashboard> | undefined {
  const { portfolio } = useRuntime();
  const [answer, setAnswer] = useState<ServedRead<Dashboard> | undefined>(undefined);
  const sent = useRef(0);
  const shown = useRef(0);
  useEffect(() => {
    if (!portfolio) return;
    let live = true;
    const load = () => {
      sent.current += 1;
      const n = sent.current;
      void portfolio.dashboard().then((read) => {
        if (!live || n < shown.current) return;
        shown.current = n;
        setAnswer(read);
      });
    };
    load();
    const poll = window.setInterval(load, POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [portfolio]);
  return answer;
}

/** The registered instruments, read once, for each holding's symbol and asset class; `null` until read or when it cannot be. */
function useInstruments(): readonly CatalogueInstrument[] | null {
  const { catalogue } = useRuntime();
  const [instruments, setInstruments] = useState<readonly CatalogueInstrument[] | null>(null);
  useEffect(() => {
    if (!catalogue) return;
    let live = true;
    void catalogue().then((answer) => {
      if (live && answer.ok) setInstruments(answer.instruments);
    });
    return () => {
      live = false;
    };
  }, [catalogue]);
  return instruments;
}

const NO_AGENTS: readonly AgentDetail[] = [];

/** The page's figures, from the source the runtime reads (DEC-975, DEC-2413 item 3). */
function usePortfolio(): Portfolio {
  const { source, ws, view } = useRuntime();
  const agents = source === "api" ? (view?.agents ?? NO_AGENTS) : NO_AGENTS;
  const reads = useHoldingsReads(agents);
  const pnl = usePnlReads(agents);
  const dashboard = useDashboard();
  const instruments = useInstruments();
  return useMemo(
    () => (source === "fixtures" ? fixturePortfolio(ws) : apiPortfolio(ws.environment, agents, reads, instruments, { dashboard, pnl })),
    [source, ws, agents, reads, instruments, dashboard, pnl],
  );
}

function NotReported({ className }: { className?: string }) {
  return <span className={cn("text-muted-foreground", className)}>{NOT_REPORTED}</span>;
}

function MoneyOr({ value }: { value: string | null }) {
  return value === null ? <NotReported /> : <Money value={value} />;
}

function SignedOr({ value }: { value: string | null }) {
  return value === null ? <NotReported /> : <SignedMoney value={value} />;
}

/** The account's value with its history: the fixture market's line, or the served snapshots and history over the API. */
function AccountValue({ portfolio }: { portfolio: Portfolio }) {
  const market = useMarket();
  if (market) return <AccountEquityChart />;
  return <ApiAccountChart total={portfolio.total} observedAt={portfolio.observedAt ?? null} environment={portfolio.environment} agree={portfolio.accountsAgree ?? true} />;
}

function Figure({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid min-w-0 gap-1">
      <dt className="text-label text-muted-foreground">{label}</dt>
      <dd className="text-figure tabular">{children}</dd>
    </div>
  );
}

/** Cash and invested, and the agents' P&L, each a figure or "not reported". */
function Figures({ portfolio }: { portfolio: Portfolio }) {
  return (
    <section aria-label="Account figures" data-slot="account-figures" className="grid gap-3">
      <dl className="grid grid-cols-1 gap-x-6 gap-y-4 min-[22rem]:grid-cols-2 sm:grid-cols-3">
        {portfolio.accountCash !== undefined ? (
          <Figure label="Cash">
            <MoneyOr value={portfolio.accountCash} />
          </Figure>
        ) : null}
        <Figure label="Invested">
          <MoneyOr value={portfolio.invested} />
        </Figure>
        <Figure label="Agents' cash">
          <MoneyOr value={portfolio.cash} />
        </Figure>
        {portfolio.unmanaged !== null && dec(portfolio.unmanaged) !== 0n ? (
          <Figure label="Not managed by an agent">
            <Money value={portfolio.unmanaged} />
          </Figure>
        ) : null}
        <Figure label="Today">
          <SignedOr value={portfolio.today} />
        </Figure>
        <Figure label={portfolio.pnlGross ? "Total P&L, gross" : "Total P&L"}>
          <SignedOr value={portfolio.pnl} />
        </Figure>
        <Figure label="Unrealized">
          <SignedOr value={portfolio.unrealized} />
        </Figure>
      </dl>
      <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
        {portfolio.environment === "live" ? "Your agents' P&L." : "Your agents' paper P&L, simulated."}
        {portfolio.pnlGross ? <span data-slot="pnl-gross"> Gross: before fees, which aren&apos;t attributed to each agent.</span> : null} <Placeholder name="performance" />
      </p>
    </section>
  );
}

const TONE: Record<SliceTone, string> = {
  "series-1": "bg-series-1",
  "series-2": "bg-series-2",
  "series-3": "bg-series-3",
  "series-4": "bg-series-4",
  neutral: "bg-muted-foreground",
};

function Swatch({ tone }: { tone: SliceTone }) {
  return <span aria-hidden data-slot="swatch" className={cn("inline-block size-2.5 shrink-0 rounded-xs", TONE[tone])} />;
}

/**
 * One breakdown as allocation bars: each part's value and share in words, and a bar of its share
 * drawn beneath. With a part not reported no share is shown, since the whole is not known.
 */
function BreakdownList({ title, id, breakdown, empty, swatches = false }: { title: string; id: string; breakdown: Breakdown; empty: string; swatches?: boolean }) {
  return (
    <section aria-labelledby={id} data-slot="breakdown" data-breakdown={id} className="grid content-start gap-2">
      <h3 id={id} className="text-h3">
        {title}
      </h3>
      {breakdown.slices.length === 0 ? (
        <p className="text-sm text-muted-foreground">{empty}</p>
      ) : (
        <ul className="grid gap-3">
          {breakdown.slices.map((s) => (
            <li key={s.key} data-slot="slice" className={cn("grid gap-1.5", s.href && "group relative")}>
              <span className="flex items-baseline justify-between gap-3 text-sm">
                <span className="flex min-w-0 items-center gap-2">
                  {swatches ? <Swatch tone={s.tone} /> : null}
                  {s.href ? (
                    <Link href={s.href} className={cn("min-w-0 font-medium wrap-anywhere underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                      {s.label}
                    </Link>
                  ) : (
                    <span className="min-w-0 font-medium wrap-anywhere">{s.label}</span>
                  )}
                </span>
                <span className="flex shrink-0 items-baseline gap-2 tabular">
                  <span className="font-mono">{s.value === null ? <NotReported /> : usd(s.value)}</span>
                  <span data-slot="share" className="min-w-[4.5ch] text-right font-mono text-muted-foreground">
                    {s.share === null ? "—" : percent(s.share)}
                  </span>
                </span>
              </span>
              <span aria-hidden className="block h-1.5 overflow-hidden rounded-full bg-muted">
                {s.value !== null && breakdown.total !== null ? (
                  <span className={cn("block h-full rounded-full", TONE[s.tone])} style={{ width: `${ratio(dec(s.value), dec(breakdown.total)) * 100}%` }} />
                ) : null}
              </span>
            </li>
          ))}
        </ul>
      )}
      {breakdown.total === null && breakdown.slices.length > 0 ? <p className="text-caption text-muted-foreground">A value is not reported, so no share is shown.</p> : null}
    </section>
  );
}

function Breakdowns({ portfolio }: { portfolio: Portfolio }) {
  return (
    <section aria-labelledby="breakdowns-title" data-slot="breakdowns" className="grid content-start gap-(--section-gap) lg:gap-8">
      <h2 id="breakdowns-title" className="text-h2">
        Breakdowns
      </h2>
      <BreakdownList id="by-agent-title" title="By agent" breakdown={byAgent(portfolio, agentHref)} empty="No agents yet." />
      <BreakdownList id="by-instrument-title" title="By instrument" breakdown={byInstrument(portfolio)} empty="Nothing invested yet." swatches />
      <BreakdownList id="by-class-title" title="By asset class" breakdown={byAssetClass(portfolio)} empty="Nothing invested yet." />
      <BreakdownList id="cash-invested-title" title="Cash and invested" breakdown={cashAndInvested(portfolio)} empty="Nothing reported." />
    </section>
  );
}

const TH = "pt-1 pb-2 pr-3 text-label font-normal text-muted-foreground";

function Symbol({ holding, tone }: { holding: Holding; tone: SliceTone | undefined }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-2">
      {tone ? <Swatch tone={tone} /> : null}
      <span className="wrap-anywhere">{holding.symbol ?? holding.instrumentId}</span>
    </span>
  );
}

/**
 * Every holding: a table from 64rem, two-line rows below it so a phone never scrolls sideways (the
 * positions table's pattern). Each row opens the agent holding it.
 */
function HoldingsTable({ holdings, tones, now, stale }: { holdings: Holding[]; tones: Map<string, SliceTone>; now: string; stale: boolean }) {
  return (
    <div className="grid">
      <ul aria-label="Holdings" data-slot="holding-rows" className="grid lg:hidden">
        {holdings.map((h) => (
          <li key={`${h.agentId}-${h.instrumentId}`} data-slot="holding-row" className="group relative grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-1 border-b border-border/70 py-3 text-sm last:border-b-0">
            <p className="font-medium">
              <Symbol holding={h} tone={tones.get(h.instrumentId)} />
            </p>
            <p className="text-right font-mono font-medium tabular">
              <span className="sr-only">Market value </span>
              {h.marketValue === null ? <NotReported /> : usd(h.marketValue)}
            </p>
            <p className="flex min-w-0 flex-wrap items-baseline gap-x-1.5 text-caption text-muted-foreground">
              <span className="whitespace-nowrap">
                <span className="sr-only">Quantity </span>
                <span className="font-mono text-foreground tabular">{quantity(h.qty)}</span> at <span className="sr-only">a last price of </span>
                <span className="font-mono text-foreground tabular">{h.last === null ? NOT_REPORTED : price(h.last)}</span>
              </span>
              <span className="whitespace-nowrap">
                · avg cost <span className="font-mono text-foreground tabular">{h.avgCost === null ? NOT_REPORTED : price(h.avgCost)}</span>
              </span>
            </p>
            <p className="flex items-center justify-end gap-1 text-right">
              <span className="sr-only">Unrealized </span>
              {h.unrealized === null ? <NotReported /> : <SignedMoney value={h.unrealized} showWord={false} />}
              <Placeholder name="performance" />
            </p>
            <p className="col-span-2 text-caption">
              <Link href={agentHref(h.agentId)} className={cn("underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                {agentName(h.agentName)}
              </Link>
            </p>
          </li>
        ))}
      </ul>
      <table className="w-full text-sm max-lg:hidden">
        <caption className="sr-only">Holdings</caption>
        <thead>
          <tr className="border-b border-border text-left">
            <th scope="col" className={TH}>Instrument</th>
            <th scope="col" className={TH}>Agent</th>
            <th scope="col" className={cn(TH, "text-right")}>Quantity</th>
            <th scope="col" className={cn(TH, "text-right")}>Avg cost</th>
            <th scope="col" className={cn(TH, "text-right")}>Last</th>
            <th scope="col" className={cn(TH, "text-right")}>Value</th>
            <th scope="col" className={cn(TH, "pr-0 text-right")}>Unrealized</th>
          </tr>
        </thead>
        <tbody>
          {holdings.map((h) => (
            <tr key={`${h.agentId}-${h.instrumentId}`} data-slot="holding" className="group relative border-b border-border/70 align-top transition-colors duration-(--duration-hover) last:border-b-0 hover:bg-background">
              <th scope="row" className="py-3 pr-3 text-left font-medium">
                <Symbol holding={h} tone={tones.get(h.instrumentId)} />
                <span className="mt-1 block text-caption font-normal text-muted-foreground">{assetClassWord(h.assetClass)}</span>
              </th>
              <td className="py-3 pr-3">
                <Link href={agentHref(h.agentId)} className={cn("underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
                  {agentName(h.agentName)}
                </Link>
              </td>
              <td className="py-3 pr-3 text-right font-mono tabular">{quantity(h.qty)}</td>
              <td className="py-3 pr-3 text-right font-mono tabular">{h.avgCost === null ? <NotReported /> : price(h.avgCost)}</td>
              <td className="py-3 pr-3 text-right whitespace-nowrap">
                <span className="font-mono tabular">{h.last === null ? <NotReported /> : price(h.last)}</span>
                {h.last !== null ? (
                  <span className="block">
                    <AsOf at={h.lastAt} now={now} stale={stale} />
                  </span>
                ) : null}
              </td>
              <td className="py-3 pr-3 text-right font-mono tabular">{h.marketValue === null ? <NotReported /> : usd(h.marketValue)}</td>
              <td className="py-3 text-right">{h.unrealized === null ? <NotReported /> : <SignedMoney value={h.unrealized} showWord={false} />}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Holdings({ portfolio, now, stale }: { portfolio: Portfolio; now: string; stale: boolean }) {
  const unread = portfolio.agents.filter((a) => a.unread !== null);
  const flat = portfolio.agents.filter((a) => a.unread === null && !portfolio.holdings.some((h) => h.agentId === a.agentId));
  const tones = instrumentTones(portfolio);
  return (
    <section aria-labelledby="holdings-title" data-slot="holdings" className="grid content-start gap-(--block-gap)">
      <h2 id="holdings-title" className="text-h2">
        Holdings
      </h2>
      {portfolio.holdings.length > 0 ? <HoldingsTable holdings={portfolio.holdings} tones={tones} now={now} stale={stale} /> : null}
      {portfolio.holdings.length === 0 && unread.length === 0 ? <p className="text-sm text-muted-foreground">Your agents hold nothing yet.</p> : null}
      {unread.length > 0 ? (
        <ul aria-label="Holdings not read" data-slot="holdings-unread" className="grid gap-1 text-sm">
          {unread.map((a) => (
            <li key={a.agentId} role={a.unread === "Reading…" ? "status" : "alert"}>
              <Link href={agentHref(a.agentId)} className="font-medium underline-offset-4 hover:underline">
                {agentName(a.name)}
              </Link>
              : <span className="text-muted-foreground">{a.unread}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {flat.length > 0 && portfolio.holdings.length > 0 ? (
        <p className="text-sm text-muted-foreground">
          Holding nothing:{" "}
          {flat.map((a, i) => (
            <span key={a.agentId}>
              {i > 0 ? ", " : null}
              <Link href={agentHref(a.agentId)} className="text-foreground underline-offset-4 hover:underline">
                {agentName(a.name)}
              </Link>
              {a.mode && a.mode !== "normal" ? <ModeBadge mode={a.mode} className="ml-1.5" /> : null}
            </span>
          ))}
          .
        </p>
      ) : null}
      {portfolio.holdings.length > 0 ? (
        <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground">
          {portfolio.environment === "live" ? "Unrealized P&L." : "Unrealized paper P&L, simulated."} <Placeholder name="performance" />
        </p>
      ) : null}
    </section>
  );
}

function EmptyPortfolio() {
  return (
    <section aria-labelledby="portfolio-empty-title" data-slot="portfolio-empty" className="grid max-w-xl content-start gap-4">
      <h2 id="portfolio-empty-title" className="text-h2">
        No agents yet
      </h2>
      <p className="max-w-measure text-muted-foreground">Your portfolio fills in once an agent trades on your account. Describe one in your own words and it asks what it needs.</p>
      <Link href="/new" className={cn(KEY, "w-fit")}>
        Set up an agent
      </Link>
    </section>
  );
}

/**
 * The Portfolio page (DEC-2413): the account as a whole beside the agents' chat. Its value and
 * history, cash and invested, the agents' P&L, every holding with the agent holding it, and the
 * breakdowns, all read only, from the fixtures or the workspace API's served reads.
 */
export function PortfolioScreen() {
  const { ws, now } = useRuntime();
  const portfolio = usePortfolio();
  const stale = ws.health.market_data.state !== "ok";
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)" data-slot="portfolio">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <h1 className="text-h1">Portfolio</h1>
        <EnvironmentBadge environment={portfolio.environment} />
        <FixtureTag scope="workspace" />
      </header>
      {portfolio.agents.length === 0 ? (
        <EmptyPortfolio />
      ) : (
        <div className={PAGE_GRID}>
          <div data-layout="main" className="grid min-w-0 grid-cols-1 content-start gap-(--section-gap)">
            <AccountValue portfolio={portfolio} />
            <Figures portfolio={portfolio} />
            <Holdings portfolio={portfolio} now={now} stale={stale} />
          </div>
          <div data-layout="rail" className="grid min-w-0 content-start">
            <Breakdowns portfolio={portfolio} />
          </div>
        </div>
      )}
    </div>
  );
}
