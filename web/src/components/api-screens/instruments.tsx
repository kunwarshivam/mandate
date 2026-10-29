"use client";

import { type FormEvent, useEffect, useId, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import type { AgentDetail } from "@/api/read-models";
import type { CatalogueAnswer, CatalogueInstrument } from "@/api/catalogue";
import { KEY } from "@/components/kumo/key";
import { ModeBadge } from "@/components/domain/mode";
import { Section, WorkspaceGate } from "@/components/screens/common";
import { type AskOutcome, type PriceAnswer, useRuntime } from "@/lib/mock-runtime";
import { dateLabel, price } from "@/lib/format";
import { cn } from "@/lib/utils";

/**
 * Finding an instrument and asking an agent to buy it (DEC-2151), over the API only. Search lists
 * the instruments the deployment has registered (`GET /instruments`) in symbol order, filtered by
 * what the owner types: no ranking, no suggestion, no quote the API does not serve (PRD FR-8.6,
 * DEC-528). The instrument's page lets the owner ask one of their agents to buy a quantity they type.
 * It is a request, not an order: the agent's builder sizes it within the mandate, the gate and the
 * autonomy rules judge it, and an `ask` mandate puts it in Approvals first (AGENTS.md rule 11).
 */

const NOT_REPORTED = "text-muted-foreground";

const CLASS_LABEL: Readonly<Record<CatalogueInstrument["asset_class"], string>> = { us_equity: "US equity", crypto: "Crypto" };

/** The registered instruments, read once on open; `null` until the deployment answers. */
function useInstruments(): CatalogueAnswer | null {
  const { catalogue } = useRuntime();
  const [answer, setAnswer] = useState<CatalogueAnswer | null>(null);
  useEffect(() => {
    if (!catalogue) return;
    let live = true;
    void catalogue().then((next) => live && setAnswer(next));
    return () => {
      live = false;
    };
  }, [catalogue]);
  return answer;
}

/** Instruments whose symbol contains `query`, ignoring case, in symbol order: a filter, never a ranking. */
export function matching(instruments: readonly CatalogueInstrument[], query: string): CatalogueInstrument[] {
  const q = query.trim().toUpperCase();
  return instruments.filter((i) => i.symbol.toUpperCase().includes(q)).sort((a, b) => (a.symbol < b.symbol ? -1 : a.symbol > b.symbol ? 1 : 0));
}

/** Over the fixtures there is no deployment to search, so the screen says so rather than show examples. */
function OverFixtures() {
  return (
    <div className="grid max-w-2xl content-start gap-(--section-gap)">
      <h1 className="text-h1">Instruments</h1>
      <p className={NOT_REPORTED}>Search reads the instruments your deployment has registered. This app is showing examples, so there is nothing to search.</p>
    </div>
  );
}

export function InstrumentSearch() {
  const { source } = useRuntime();
  const answer = useInstruments();
  const [query, setQuery] = useState("");
  const inputId = useId();
  if (source !== "api") return <OverFixtures />;
  const found = answer?.ok ? matching(answer.instruments, query) : [];
  return (
    <WorkspaceGate api>
      <div className="grid max-w-2xl content-start gap-(--section-gap)">
        <h1 className="text-h1">Instruments</h1>
        <div className="grid gap-2">
          <label htmlFor={inputId} className="text-sm font-medium">
            Search by symbol
          </label>
          <input id={inputId} type="search" value={query} onChange={(e) => setQuery(e.target.value)} autoComplete="off" spellCheck={false} className="h-11 rounded-md border border-border bg-card px-3 font-mono uppercase" />
        </div>
        {answer === null ? (
          <p className={NOT_REPORTED}>Reading your deployment&rsquo;s instruments.</p>
        ) : !answer.ok ? (
          <p className={NOT_REPORTED}>The instruments are not reported.</p>
        ) : found.length === 0 ? (
          <p className={NOT_REPORTED}>{answer.instruments.length === 0 ? "Your deployment has registered no instruments." : "No registered instrument matches."}</p>
        ) : (
          <ul className="grid" aria-label="Instruments">
            {found.map((i) => (
              <li key={i.instrument_id} data-slot="instrument" className="flex flex-wrap items-baseline gap-x-4 gap-y-1 border-b border-border/70 py-3 last:border-b-0">
                <Link href={`/instruments/${i.instrument_id}`} className="font-mono text-h3 underline-offset-4 hover:underline">
                  {i.symbol}
                </Link>
                <span className="text-sm text-muted-foreground">{CLASS_LABEL[i.asset_class]}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </WorkspaceGate>
  );
}

/** How often the instrument's page reads its price again: a daily close moves once a session. */
export const PRICE_POLL_MS = 30_000;

/**
 * The last trusted daily close (DEC-2152), labelled as what it is, with the session it closed: never
 * "price now" or a bid. Anything the deployment does not report, or a read that fails, says so.
 */
function LastClose({ instrumentId }: { instrumentId: string }) {
  const { price: readPrice } = useRuntime();
  const [answer, setAnswer] = useState<PriceAnswer | null>(null);
  useEffect(() => {
    if (!readPrice) return;
    let live = true;
    const load = () => void readPrice(instrumentId).then((next) => live && setAnswer(next));
    load();
    const poll = window.setInterval(load, PRICE_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [readPrice, instrumentId]);
  if (answer?.ok && answer.value.reported)
    return (
      <p className="text-sm" data-slot="quote">
        Last daily close <span className="font-mono font-medium tabular">{price(answer.value.close)}</span>, the session of {dateLabel(answer.value.bar_start)}. Not a live price.
      </p>
    );
  return (
    <p className={cn("text-sm", NOT_REPORTED)} data-slot="quote">
      {answer === null ? "Reading the last close." : "Price not reported by your deployment."}
    </p>
  );
}

/** Why a request was not sent, in words; a server's code stays out of the sentence. */
function notSentWords(reason: string, symbol: string): string {
  switch (reason) {
    case "invalid_size":
      return "Enter a quantity above zero, such as 1 or 2.5.";
    case "instrument_not_pinned":
      return `This agent's mandate doesn't include ${symbol}, so it can't be asked for it.`;
    case "not_found":
      return "Your deployment reports no such agent, or it has stopped.";
    case "forbidden":
      return "Your role can't ask an agent for a trade.";
    case "reduction_only":
      return "This session can only reduce risk. Sign in again to ask an agent.";
    case "live_unavailable":
      return "Asking an agent is not available on real money.";
    default:
      return "Your deployment did not take the request. Nothing was recorded.";
  }
}

/** Why an agent in this mode takes no new position, so an ask it cannot act on is never a surprise (rule 13: exits still run). */
const HOLDS_OPENINGS: Readonly<Partial<Record<NonNullable<AgentDetail["mode"]>, string>>> = {
  paused: "This agent won't open new positions while paused. Resume it to act on a request.",
  exits_only: "This agent only exits positions now, so it won't open a new one.",
};

type Step = { step: "idle" } | { step: "sending"; recorded: boolean } | { step: "done"; outcome: AskOutcome };

function OutcomeLine({ outcome, symbol }: { outcome: AskOutcome; symbol: string }) {
  switch (outcome.phase) {
    case "taken":
      return (
        <p className="text-sm" data-slot="request-taken">
          Your agent took the request. If its mandate asks you first, it is waiting in{" "}
          <Link href="/approvals" className="underline underline-offset-4">
            Approvals
          </Link>
          .
        </p>
      );
    case "recorded":
      return <p className="text-sm">Your request is recorded. Your agent has not answered yet; check Approvals before asking again.</p>;
    case "refused":
      return (
        <p className="text-sm font-medium" data-reason={outcome.reason ?? undefined}>
          Your agent did not take the request. Its record is in the agent&rsquo;s activity.
        </p>
      );
    case "undelivered":
      return (
        <p className="text-sm font-medium" data-reason={outcome.reason}>
          Not sent. {notSentWords(outcome.reason, symbol)}
        </p>
      );
    case "unknown":
      return <p className="text-sm">Whether your request was recorded is not known yet. Check Approvals before asking again.</p>;
  }
}

/** One agent's form: a quantity, and the ask. Each press is one request under one idempotency key. */
function AskAgent({ agent, instrument }: { agent: AgentDetail; instrument: CatalogueInstrument }) {
  const { askAgent } = useRuntime();
  const [size, setSize] = useState("");
  const [step, setStep] = useState<Step>({ step: "idle" });
  const sizeId = useId();
  const busy = step.step === "sending";
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!askAgent || busy) return;
    setStep({ step: "sending", recorded: false });
    void askAgent(agent.agent_id, instrument.instrument_id, size.trim(), () => setStep({ step: "sending", recorded: true })).then((outcome) => setStep({ step: "done", outcome }));
  };
  return (
    <li data-slot="ask-agent" data-agent={agent.agent_id} className="grid gap-3 border-b border-border/70 py-4 last:border-b-0">
      <div className="flex flex-wrap items-center gap-3">
        <Link href={`/agents/${agent.agent_id}`} className="font-semibold underline-offset-4 hover:underline">
          {agent.label ?? <span className={NOT_REPORTED}>Name not reported</span>}
        </Link>
        {agent.mode ? <ModeBadge mode={agent.mode} /> : null}
      </div>
      {agent.mode && HOLDS_OPENINGS[agent.mode] ? (
        <p className="text-sm" data-slot="ask-held">
          {HOLDS_OPENINGS[agent.mode]}
        </p>
      ) : null}
      <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
        <div className="grid gap-1.5">
          <label htmlFor={sizeId} className="text-sm">
            Quantity of {instrument.symbol}
          </label>
          <input id={sizeId} inputMode="decimal" value={size} onChange={(e) => setSize(e.target.value)} disabled={busy} autoComplete="off" className="h-11 w-36 rounded-md border border-border bg-card px-3 font-mono tabular" />
        </div>
        <Button type="submit" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} disabled={busy || !askAgent}>
          Ask agent to buy
        </Button>
      </form>
      <div role="status" aria-live="polite">
        {step.step === "sending" ? <p className="text-sm">{step.recorded ? "Your request is recorded. Waiting for your agent." : "Asking your agent."}</p> : null}
        {step.step === "done" ? <OutcomeLine outcome={step.outcome} symbol={instrument.symbol} /> : null}
      </div>
    </li>
  );
}

export function InstrumentScreen({ instrumentId }: { instrumentId: string }) {
  const { source, view } = useRuntime();
  const answer = useInstruments();
  if (source !== "api") return <OverFixtures />;
  const instrument = answer?.ok ? answer.instruments.find((i) => i.instrument_id === instrumentId) : undefined;
  const agents = (view?.agents ?? []).filter((a) => a.mode !== "stopped");
  return (
    <WorkspaceGate api>
      <article className="grid max-w-2xl content-start gap-(--section-gap)">
        <Link href="/instruments" className="text-sm underline-offset-4 hover:underline">
          Back to search
        </Link>
        {answer === null ? (
          <p className={NOT_REPORTED}>Reading the instrument.</p>
        ) : !answer.ok ? (
          <p className={NOT_REPORTED}>The instrument is not reported.</p>
        ) : !instrument ? (
          <h1 className="text-h1">Instrument not registered</h1>
        ) : (
          <>
            <header className="grid gap-1">
              <h1 className="font-mono text-h1">{instrument.symbol}</h1>
              <p className="text-sm text-muted-foreground">{CLASS_LABEL[instrument.asset_class]}</p>
              <LastClose instrumentId={instrument.instrument_id} />
            </header>
            <Section title="Ask your agent">
              <p className="max-w-measure text-sm text-muted-foreground">
                This asks your agent; it places no order. The agent sizes the order within its mandate&rsquo;s limits, and its rules decide whether it asks you first. Only an agent whose mandate includes {instrument.symbol} can take it.
              </p>
              {agents.length === 0 ? (
                <p className={NOT_REPORTED}>You have no running agent to ask.</p>
              ) : (
                <ul className="grid" aria-label="Your agents">
                  {agents.map((agent) => (
                    <AskAgent key={agent.agent_id} agent={agent} instrument={instrument} />
                  ))}
                </ul>
              )}
            </Section>
          </>
        )}
      </article>
    </WorkspaceGate>
  );
}
