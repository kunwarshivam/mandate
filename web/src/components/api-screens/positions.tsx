"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import type { AgentHolding } from "@/api/activity";
import { isAssetId } from "@/api/commands";
import type { AgentDetail, ReadPosition } from "@/api/read-models";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { Section, WorkspaceGate } from "@/components/screens/common";
import { clock, quantity } from "@/lib/format";
import { type CommandRecord, type ExitOutcome, type PositionsAnswer, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

/**
 * Positions over the workspace API (§4.7 D3, DEC-2111, DEC-1096): each agent's own positions, the net
 * quantity its fills hold per instrument, grouped by the account its document names, and beside them
 * what that account held at its last reconciliation, whichever agent bought it, from the same reads. The cost basis, risk
 * mark and protection are served `null` until their folds are (DEC-2111 item 5), so each says "not
 * reported", never a figure computed here. A list that cannot be read is never shown as empty.
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
type ExitState = { step: "idle" } | { step: "confirming" } | { step: "sending" } | { step: "recorded" } | { step: "done"; outcome: ExitOutcome };

/** An exit's outcome in words, never by a code: what happened, and what was not sold. */
function exitWords(state: ExitState): string {
  if (state.step === "sending") return "Sending your exit…";
  if (state.step === "recorded") return "Recorded in the journal: your exit. Waiting for the agent's runtime to take it.";
  if (state.step !== "done") return "";
  switch (state.outcome.phase) {
    case "applied":
      return "Applied: the agent's runtime took your exit. Its sell shows in the agent's orders.";
    case "refused":
      return "Refused by the agent's runtime: nothing was sold. Its records say why.";
    case "ended":
      return "Ended without a sell: nothing was sold. The agent's records say why.";
    case "recorded":
      return "Recorded in the journal: your exit. Its outcome is not reported yet; look at the agent's orders.";
    case "undelivered":
      return "Not recorded: your deployment refused your exit. Nothing was sold.";
    case "unknown":
      return "The result is unknown; we are checking. Look at the agent's orders before you exit again.";
  }
}

/**
 * The owner's exit from one instrument (§5.4, rule 13): asked first on the screen, then sent once.
 * The bid confirmation is never sent, since this screen shows no quote with its size and time, so
 * the confirmation says an equity sell outside the regular session waits for it.
 */
function ExitCell({ agent, holding, name }: { agent: AgentDetail; holding: AgentHolding; name: string }) {
  const { ownerExit, ws } = useRuntime();
  const allowed = useCan("stop.full");
  const [state, setState] = useState<ExitState>({ step: "idle" });
  const sent = useRef(false);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const instrument = holding.instrument_id;
  if (!ownerExit || !allowed || holding.filled_qty.startsWith("-") || /^0(\.0*)?$/.test(holding.filled_qty)) return null;
  if (!isAssetId(instrument)) return <span className={NOT_REPORTED}>{"No exit here: this position names no asset id. The agent's kill switch still sells it."}</span>;
  const lines = [
    `Sell ${name}'s whole position in ${instrument}: ${quantity(holding.filled_qty)} held by its fills.`,
    "Your exit is never denied for the agent's limits or conduct controls; only participation caps pace it.",
    "In the regular session it is sent at once. Outside the regular session, an equity sell waits for it: this screen has no bid you could confirm.",
    "The journal records it with what this screen showed.",
  ];
  const sell = () => {
    if (sent.current) return;
    sent.current = true;
    const record: CommandRecord = { screen: "D10", environment: ws.environment, title: `Exit ${instrument}`, shown: lines, modes: [{ agent: agent.agent_id, badge: agent.mode ?? "not reported" }] };
    setState({ step: "sending" });
    void ownerExit(agent.agent_id, instrument, record, () => {
      if (live.current) setState({ step: "recorded" });
    }).then((outcome) => {
      if (live.current) setState({ step: "done", outcome });
    });
  };
  if (state.step === "idle")
    return (
      <Button type="button" variant="secondary" aria-label={`Exit ${instrument}`} onClick={() => setState({ step: "confirming" })}>
        Exit
      </Button>
    );
  if (state.step === "confirming")
    return (
      <div role="group" aria-label={`Exit ${instrument}`} className="grid gap-2">
        <ul className="grid gap-1">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
        <div className="flex gap-2">
          <Button type="button" variant="primary" onClick={sell}>
            Sell it
          </Button>
          <Button type="button" variant="secondary" onClick={() => setState({ step: "idle" })}>
            Cancel
          </Button>
        </div>
      </div>
    );
  return (
    <p role="status" aria-label={`Your exit from ${instrument}`}>
      {exitWords(state)}
    </p>
  );
}

/** One agent's own positions, with the three members its deployment does not report yet. */
function OwnPositions({
  agent,
  answer,
  name,
}: {
  agent: AgentDetail;
  answer: Holdings;
  name: string;
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
                <td className={`py-2.5 ${NOT_REPORTED}`}>Not reported</td>
                <td className={`py-2.5 ${NOT_REPORTED}`}>Not reported</td>
                <td className={`py-2.5 ${NOT_REPORTED}`}>Not reported</td>
                <td className="py-2.5">
                  <ExitCell agent={agent} holding={h} name={name} />
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
