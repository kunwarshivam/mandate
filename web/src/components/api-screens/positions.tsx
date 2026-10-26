"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import type { AgentHolding, Protection, RiskMark } from "@/api/activity";
import { isAssetId } from "@/api/commands";
import type { AgentDetail, ReadPosition } from "@/api/read-models";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { Section, WorkspaceGate } from "@/components/screens/common";
import { clock, price, quantity, signedUsd, usd } from "@/lib/format";
import { type CommandRecord, type ExitOutcome, type PositionsAnswer, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

/**
 * Positions over the workspace API (§4.7 D3, DEC-2111, DEC-1096): each agent's own positions, the net
 * quantity its fills hold per instrument, grouped by the account its document names, and beside them
 * what that account held at its last reconciliation, whichever agent bought it, from the same reads. The cost basis, risk
 * mark, market value, unrealized P&L and protection are DEC-2138's, shown as served: each the read
 * serves `null` says "not reported", a mark that is not sane shows no price, and nothing is computed
 * here. A list that cannot be read is never shown as empty.
 */

/** How often the positions are read again while the screen is open. */
const POLL_MS = 5000;
const NOT_REPORTED = "text-muted-foreground";

/** An agent's latest positions answer, with when it arrived among all the screen's reads. */
type Holdings = PositionsAnswer & { received: number };

/**
 * Every agent's own positions, read on open and every few seconds; an agent not yet answered is
 * absent. Each agent's reads are numbered, and an answer older than one already shown is dropped.
 */
function useHoldings(
  agents: readonly AgentDetail[],
): Readonly<Record<string, Holdings>> {
  const { holdings } = useRuntime();
  const [answers, setAnswers] = useState<Readonly<Record<string, Holdings>>>(
    {},
  );
  const sent = useRef(new Map<string, number>());
  const shown = useRef(new Map<string, number>());
  const arrivals = useRef(0);
  const ids = agents.map((a) => a.agent_id).join("\n");
  useEffect(() => {
    if (!holdings) return;
    let live = true;
    const load = () => {
      for (const id of ids ? ids.split("\n") : []) {
        const n = (sent.current.get(id) ?? 0) + 1;
        sent.current.set(id, n);
        void holdings(id).then((answer) => {
          if (!live || n < (shown.current.get(id) ?? 0)) return;
          shown.current.set(id, n);
          arrivals.current += 1;
          const received = arrivals.current;
          setAnswers((all) => ({ ...all, [id]: { ...answer, received } }));
        });
      }
    };
    load();
    const poll = window.setInterval(load, POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [holdings, ids]);
  return answers;
}

function AgentLink({ agent }: { agent: AgentDetail }) {
  return (
    <Link
      href={`/agents/${agent.agent_id}`}
      className="font-semibold underline-offset-4 hover:underline"
    >
      {agent.label ?? <span className={NOT_REPORTED}>Name not reported</span>}
    </Link>
  );
}

/** Where the owner's exit from one instrument stands on this screen. */
type ExitStep = { step: "confirming" } | { step: "sending" } | { step: "recorded" } | { step: "done"; outcome: ExitOutcome };

/** One exit the owner started here: the agent, its name, the asset, and where it stands. */
interface Exit {
  agentId: string;
  name: string;
  instrument: string;
  state: ExitStep;
}

const exitKey = (agentId: string, instrument: string) => `${agentId}\n${instrument}`;

/** An exit's outcome in words, never by a code: what the journal says, and that nothing was sold where nothing was recorded. */
function exitWords(state: Exclude<ExitStep, { step: "confirming" }>): string {
  if (state.step === "sending") return "Sending your exit…";
  if (state.step === "recorded") return "Recorded in the journal: your exit. Waiting for the agent's runtime to take it.";
  switch (state.outcome.phase) {
    case "applied":
      return "Taken by the agent's runtime. Look at the agent's orders for its sell.";
    case "refused":
      return "Refused by the agent's runtime: nothing was sold for it. Its records say why.";
    case "ended":
      return "Ended without a sell for it. The agent's records say why.";
    case "recorded":
      return "Recorded in the journal: your exit. Its outcome is not reported yet; look at the agent's orders.";
    case "undelivered":
      return "Not recorded: your deployment refused your exit. Nothing was sold.";
    case "unknown":
      return "The result is unknown; we are checking. Look at the agent's orders before you exit again.";
  }
}

/** Whether the owner may ask again here: only where nothing was recorded, or the runtime refused it. */
const retryable = (state: ExitStep) => state.step === "done" && (state.outcome.phase === "undelivered" || state.outcome.phase === "refused");

/** The exits the owner started on this screen, kept here so a row that sold to nothing, or a read that failed, never hides one. */
interface Exits {
  of: (agentId: string, instrument: string) => Exit | undefined;
  ask: (agentId: string, name: string, instrument: string) => void;
  cancel: (agentId: string, instrument: string) => void;
  sell: (agent: AgentDetail, name: string, instrument: string, lines: string[]) => void;
  all: Exit[];
}

function useExits(): Exits {
  const { ownerExit, ws } = useRuntime();
  const [exits, setExits] = useState<Readonly<Record<string, Exit>>>({});
  const sending = useRef(new Set<string>());
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const put = (key: string, exit: Exit) => setExits((all) => ({ ...all, [key]: exit }));
  return {
    of: (agentId, instrument) => exits[exitKey(agentId, instrument)],
    ask: (agentId, name, instrument) => put(exitKey(agentId, instrument), { agentId, name, instrument, state: { step: "confirming" } }),
    cancel: (agentId, instrument) =>
      setExits((all) => {
        const rest = { ...all };
        delete rest[exitKey(agentId, instrument)];
        return rest;
      }),
    sell: (agent, name, instrument, lines) => {
      const key = exitKey(agent.agent_id, instrument);
      if (!ownerExit || sending.current.has(key)) return;
      sending.current.add(key);
      const at = (state: ExitStep) => {
        if (live.current) put(key, { agentId: agent.agent_id, name, instrument, state });
      };
      const record: CommandRecord = { screen: "D10", environment: ws.environment, title: `Exit ${instrument}`, shown: lines, modes: [{ agent: agent.agent_id, badge: agent.mode ?? "not reported" }] };
      at({ step: "sending" });
      void ownerExit(agent.agent_id, instrument, record, () => at({ step: "recorded" })).then((outcome) => {
        at({ step: "done", outcome });
        if (outcome.phase === "undelivered" || outcome.phase === "refused") sending.current.delete(key);
      });
    },
    all: Object.values(exits),
  };
}

/**
 * The owner's exit from one instrument (§5.4, rule 13): asked first on the screen, then sent once.
 * The bid confirmation is never sent, since this screen shows no quote with its size and time, so
 * the confirmation says an equity sell outside the regular session waits for it.
 */
function ExitControl({ agent, holding, name, exits }: { agent: AgentDetail; holding: AgentHolding; name: string; exits: Exits }) {
  const { ownerExit } = useRuntime();
  const allowed = useCan("positions.exit");
  const instrument = holding.instrument_id;
  if (!ownerExit || !allowed || holding.filled_qty.startsWith("-") || /^0(\.0*)?$/.test(holding.filled_qty)) return null;
  if (!isAssetId(instrument)) return <span className={NOT_REPORTED}>{"No exit here: this position names no asset id. The agent's kill switch still sells it."}</span>;
  const exit = exits.of(agent.agent_id, instrument);
  if (exit && exit.state.step !== "confirming" && !retryable(exit.state)) return <span className="text-muted-foreground">Asked below.</span>;
  if (!exit || exit.state.step !== "confirming")
    return (
      <Button type="button" variant="secondary" aria-label={`Exit ${instrument}`} onClick={() => exits.ask(agent.agent_id, name, instrument)}>
        {exit ? "Exit again" : "Exit"}
      </Button>
    );
  const lines = [
    `Sell ${name}'s whole position in ${instrument}: ${quantity(holding.filled_qty)} held by its fills.`,
    "Your exit is never denied for the agent's limits or conduct controls; only participation caps pace it.",
    "In the regular session it is sent at once. Outside the regular session, an equity sell waits for it: this screen has no bid you could confirm.",
    "The journal records it with what this screen showed.",
  ];
  return (
    <div role="group" aria-label={`Exit ${instrument}`} className="grid gap-2">
      <ul className="grid gap-1">
        {lines.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      <div className="flex gap-2">
        <Button type="button" variant="primary" onClick={() => exits.sell(agent, name, instrument, lines)}>
          Sell it
        </Button>
        <Button type="button" variant="secondary" onClick={() => exits.cancel(agent.agent_id, instrument)}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

/** Every exit sent from this screen, with where it stands, whatever the latest positions read shows. */
function ExitStatuses({ exits }: { exits: Exits }) {
  const shown = exits.all.filter((e) => e.state.step !== "confirming");
  if (shown.length === 0) return null;
  return (
    <Section id="positions-exits" title="Your exits">
      <div className="grid gap-2">
        {shown.map((e) => (
          <p key={exitKey(e.agentId, e.instrument)} role="status" aria-label={`Your exit from ${e.instrument}`}>
            <span className="font-medium">
              {e.name}, {e.instrument}:
            </span>{" "}
            {e.state.step === "confirming" ? null : exitWords(e.state)}
          </p>
        ))}
      </div>
    </Section>
  );
}

/** A figure the read served, or "Not reported" where it served `null`: never a zero or a figure computed here. */
function Figure({ text }: { text: string | null }) {
  return text === null ? <td className={`py-2.5 ${NOT_REPORTED}`}>Not reported</td> : <td className="py-2.5 font-mono tabular">{text}</td>;
}

/** The latest counted risk mark; one that is not sane shows no price at all (DEC-2138 item 2). */
function MarkCell({ mark }: { mark: RiskMark | null }) {
  if (mark === null) return <Figure text={null} />;
  if (!mark.sane || mark.bid === null) return <td className={`py-2.5 ${NOT_REPORTED}`}>No sane mark</td>;
  return (
    <td className="py-2.5">
      <span className="font-mono tabular">{price(mark.bid)}</span> bid, {clock(mark.marked_at)} UTC
    </td>
  );
}

/** The account's protection in the instrument as the executor folds it (trading spec §5.4, DEC-2138 item 4). */
function ProtectionCell({ protection }: { protection: Protection | null }) {
  if (protection === null) return <Figure text={null} />;
  if (protection.orders.length > 0) {
    const parts = [protection.stop === null ? null : `stop ${price(protection.stop)}`, protection.take_profit === null ? null : `take-profit ${price(protection.take_profit)}`].filter((p): p is string => p !== null);
    return (
      <td className="py-2.5">
        {parts.length > 0 ? `${parts.join(", ")}; ` : ""}covers {quantity(protection.covered_qty)}
      </td>
    );
  }
  if (protection.unprotected_since !== null) return <td className="py-2.5 font-medium">Unprotected since {clock(protection.unprotected_since)} UTC</td>;
  return <td className="py-2.5 text-muted-foreground">No protective order recorded</td>;
}

/** One agent's own positions, with what its deployment reports of each and "not reported" where it does not. */
function OwnPositions({
  agent,
  answer,
  name,
  exits,
}: {
  agent: AgentDetail;
  answer: Holdings;
  name: string;
  exits: Exits;
}) {
  if (!answer.ok || answer.account_ref === null) return null;
  return (
    <div
      className="grid gap-2"
      data-slot="agent-positions"
      data-agent={agent.agent_id}
    >
      <AgentLink agent={agent} />
      {answer.items.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          Its own fills net to nothing.
        </p>
      ) : (
        <table
          className="w-full text-left text-sm"
          aria-label={`${name}'s positions`}
        >
          <thead className="text-muted-foreground">
            <tr>
              <th scope="col" className="py-2 font-normal">
                Instrument
              </th>
              <th scope="col" className="py-2 font-normal">
                Held by its fills
              </th>
              <th scope="col" className="py-2 font-normal">
                Cost basis
              </th>
              <th scope="col" className="py-2 font-normal">
                Risk mark
              </th>
              <th scope="col" className="py-2 font-normal">
                Market value
              </th>
              <th scope="col" className="py-2 font-normal">
                Unrealized P&amp;L
              </th>
              <th scope="col" className="py-2 font-normal">
                Protection
              </th>
              <th scope="col" className="py-2 font-normal">
                Your exit
              </th>
            </tr>
          </thead>
          <tbody>
            {answer.items.map((h) => (
              <tr key={h.instrument_id} className="border-t border-border/70">
                <td className="py-2.5 font-medium">{h.instrument_id}</td>
                <td className="py-2.5 font-mono tabular">
                  {quantity(h.filled_qty)}
                </td>
                <Figure text={h.cost_basis ? `${usd(h.cost_basis.basis)} for ${quantity(h.cost_basis.qty)}` : null} />
                <MarkCell mark={h.risk_mark} />
                <Figure text={h.market_value === null ? null : usd(h.market_value)} />
                <Figure text={h.unrealized_pnl === null ? null : signedUsd(h.unrealized_pnl)} />
                <ProtectionCell protection={h.protection} />
                <td className="py-2.5">
                  <ExitControl agent={agent} holding={h} name={name} exits={exits} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/**
 * What the account held at its last reconciliation, from the latest of its agents' positions reads
 * to arrive: each serves the same account stream's fold, so the latest is the freshest, an empty
 * list after a full sale included.
 */
function AccountHoldings({ answers }: { answers: readonly Holdings[] }) {
  const latest = answers
    .filter((a): a is Extract<Holdings, { ok: true }> => a.ok && a.account_positions !== null)
    .reduce<Extract<Holdings, { ok: true }> | null>((best, a) => (best === null || a.received > best.received ? a : best), null);
  const served: readonly ReadPosition[] | null = latest?.account_positions ?? null;
  if (served === null)
    return (
      <p className={NOT_REPORTED}>
        The account&rsquo;s holdings are not reported.
      </p>
    );
  if (served.length === 0)
    return (
      <p className="text-muted-foreground">
        The account held nothing at its last reconciliation.
      </p>
    );
  return (
    <table
      className="w-full text-left text-sm"
      aria-label="The account's holdings"
    >
      <thead className="text-muted-foreground">
        <tr>
          <th scope="col" className="py-2 font-normal">
            Instrument
          </th>
          <th scope="col" className="py-2 font-normal">
            Held
          </th>
          <th scope="col" className="py-2 font-normal">
            At the broker
          </th>
          <th scope="col" className="py-2 font-normal">
            As of
          </th>
        </tr>
      </thead>
      <tbody>
        {served.map((p) => (
          <tr key={p.instrument} className="border-t border-border/70">
            <td className="py-2.5 font-medium">{p.instrument}</td>
            <td className="py-2.5 font-mono tabular">{quantity(p.qty)}</td>
            <td className="py-2.5 font-mono tabular">
              {quantity(p.broker_qty)}
              {p.mismatch ? (
                <span className="ms-2 font-sans text-foreground">
                  Differs from the broker
                </span>
              ) : null}
            </td>
            <td className="py-2.5 text-muted-foreground">
              {clock(p.observed_at)} UTC
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Positions() {
  const view = useRuntime().view;
  const agents = view?.agents ?? [];
  const answers = useHoldings(agents);
  const exits = useExits();
  if (!view) return null;
  const header = (
    <PageHeader
      title="Positions"
      description="What each agent's own fills hold, and what its account held at its last reconciliation."
      className="mb-0"
    />
  );
  if (agents.length === 0)
    return (
      <div className="grid gap-(--section-gap)">
        {header}
        <p className="text-muted-foreground">
          Your deployment reports no agents.
        </p>
      </div>
    );
  const accounts = new Map<string, AgentDetail[]>();
  const counts = new Map<string, number>();
  for (const a of agents)
    counts.set(
      a.label ?? a.agent_id,
      (counts.get(a.label ?? a.agent_id) ?? 0) + 1,
    );
  const nameOf = (a: AgentDetail) =>
    counts.get(a.label ?? a.agent_id)! > 1
      ? `${a.label ?? a.agent_id} (${a.agent_id})`
      : (a.label ?? a.agent_id);
  const reading: AgentDetail[] = [];
  const unreported: AgentDetail[] = [];
  const noAccount: AgentDetail[] = [];
  for (const agent of agents) {
    const answer = answers[agent.agent_id];
    if (!answer) reading.push(agent);
    else if (!answer.ok) unreported.push(agent);
    else if (answer.account_ref === null) noAccount.push(agent);
    else
      accounts.set(answer.account_ref, [
        ...(accounts.get(answer.account_ref) ?? []),
        agent,
      ]);
  }
  const names = (list: AgentDetail[]) => list.map(nameOf).join(", ");
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      {header}
      <ExitStatuses exits={exits} />
      {[...accounts.entries()].map(([ref, members], i) => (
        <Section
          key={ref}
          id={`positions-account-${i}`}
          title={accounts.size > 1 ? `Account ${i + 1}` : "This account"}
        >
          <div className="grid gap-6">
            {members.map((agent) => (
              <OwnPositions
                key={agent.agent_id}
                agent={agent}
                answer={answers[agent.agent_id]}
                name={nameOf(agent)}
                exits={exits}
              />
            ))}
            <div className="grid gap-2">
              <h3 className="text-sm font-semibold">
                The account&rsquo;s holdings
              </h3>
              <p className="text-sm text-muted-foreground">
                At its last reconciliation, whichever agent bought them.
              </p>
              <AccountHoldings
                answers={members.map((m) => answers[m.agent_id])}
              />
            </div>
          </div>
        </Section>
      ))}
      {reading.length > 0 ? (
        <p className={NOT_REPORTED}>Reading positions: {names(reading)}.</p>
      ) : null}
      {unreported.length > 0 ? (
        <p className={NOT_REPORTED}>
          Positions not reported: {names(unreported)}.
        </p>
      ) : null}
      {noAccount.length > 0 ? (
        <p className={NOT_REPORTED}>
          Positions not reported, since your deployment found no account:{" "}
          {names(noAccount)}.
        </p>
      ) : null}
    </div>
  );
}

export function ApiPositions() {
  return (
    <WorkspaceGate api>
      <Positions />
    </WorkspaceGate>
  );
}
