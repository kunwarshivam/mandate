"use client";

import { useEffect, useRef, useState } from "react";
import type { JournalRecord } from "@/api/records";
import { Section } from "@/components/screens/common";
import type { ReasonCode } from "@/fixtures/types";
import { datedClock, quantity } from "@/lib/format";
import { gateRule } from "@/lib/gate-reasons";
import { type RecordsAnswer, useRuntime } from "@/lib/mock-runtime";

/**
 * Gate decisions over the workspace API (§4.8.1; journal spec §9.1 `DecisionMade`): every
 * `DecisionMade` among the latest records of each agent's stream, newest first, each read from its
 * body only once the body matched its hash. It says what the runtime proposed, what its dry run
 * answered and why, and what autonomy then chose, as the record holds them; a version 3 record also
 * names the checks the dry run left to the executor's binding gate, which it never reported passed.
 * A record whose body does not match its hash is shown as that, never read.
 */

/** How many of each agent stream's latest records are searched for decisions. */
export const DECISIONS_TAIL = 200;
const POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";

const DRY_RUN: Readonly<Record<string, string>> = { allow: "Allowed by the dry run", deny: "Denied by the dry run", defer: "Left by the dry run to the binding gate" };
const AUTONOMY: Readonly<Record<string, string>> = { auto: "it acted without asking", ask: "it asked you", deny: "its mandate said no" };
const PURPOSE: Readonly<Record<string, string>> = { open: "open", increase: "add to", discretionary_exit: "exit", risk_exit: "risk exit" };

const text = (value: unknown): string | null => (typeof value === "string" ? value : null);
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : []);

/** The gate's reason in words where the platform has them; otherwise its code, as the record holds it. */
function reasonWords(code: string): string {
  try {
    return gateRule(code as ReasonCode, null);
  } catch {
    return `Reason: ${code}.`;
  }
}

/** One decision's lines, from its payload. */
function DecisionLine({ record, agent, agentId, now }: { record: JournalRecord; agent: string; agentId: string; now: string }) {
  if (!record.matches || !record.payload)
    return (
      <li className="grid gap-0.5 border-b border-border/70 py-3 text-sm last:border-b-0" data-matches={record.matches ? "true" : "false"}>
        <span className="font-medium">A decision of {agent}</span>
        <span className={NOT_REPORTED}>{record.matches ? "Its record could not be read." : "Its record does not match its hash, so it is not read."}</span>
      </li>
    );
  const p = record.payload;
  const side = text(p.side);
  const qty = text(p.qty);
  const instrument = text(p.instrument_id);
  const dry = text(p.dry_run);
  const reason = text(p.reason_code);
  const autonomy = text(p.autonomy);
  const purpose = text(p.purpose);
  const deferred = texts(p.deferred_to_binding);
  return (
    <li className="grid gap-1 border-b border-border/70 py-3 text-sm last:border-b-0" data-dry-run={dry ?? "not_reported"} data-event={record.event_id} data-agent={agentId}>
      <span className="flex flex-wrap justify-between gap-x-3">
        <span className="font-medium">
          {agent}: {side === "buy" ? "buy" : side === "sell" ? "sell" : "trade"} {qty !== null ? quantity(qty) : ""} {instrument ?? ""}
          {purpose && PURPOSE[purpose] ? ` (${PURPOSE[purpose]})` : ""}
        </span>
        <span className="text-muted-foreground tabular-nums">{datedClock(record.recorded_at, now, "UTC")} UTC</span>
      </span>
      <span>
        {dry && DRY_RUN[dry] ? DRY_RUN[dry] : "Dry run not reported"}
        {autonomy && AUTONOMY[autonomy] ? `; ${AUTONOMY[autonomy]}` : ""}.
      </span>
      {reason ? <span className="text-muted-foreground">{reasonWords(reason)}</span> : null}
      {deferred.length > 0 ? <span className="text-muted-foreground">Left to the executor&rsquo;s binding gate, never reported passed: {deferred.join(", ").replaceAll("_", " ")}.</span> : null}
    </li>
  );
}

export function ApiDecisions() {
  const { view, streamRecords, workspaceId, now } = useRuntime();
  const agents = view?.agents ?? [];
  const ids = agents.map((a) => a.agent_id).join("\n");
  const [read, setRead] = useState<{ answers: Readonly<Record<string, RecordsAnswer>>; resolved: boolean }>({ answers: {}, resolved: false });
  const generation = useRef(0);
  useEffect(() => {
    if (!streamRecords || !workspaceId) return;
    let live = true;
    const load = async () => {
      generation.current += 1;
      const mine = generation.current;
      const listed = ids ? ids.split("\n") : [];
      const answers = await Promise.all(listed.map((id) => streamRecords(`agent:${workspaceId}:${id}`, DECISIONS_TAIL)));
      if (!live || mine !== generation.current) return;
      setRead({ answers: Object.fromEntries(listed.map((id, i) => [id, answers[i]])), resolved: true });
    };
    void load();
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [streamRecords, workspaceId, ids]);
  const decisions: Array<{ record: JournalRecord; agent: string; agentId: string }> = [];
  const unread: string[] = [];
  let forbidden = 0;
  for (const a of agents) {
    const answer = read.answers[a.agent_id];
    const name = a.label ?? a.agent_id;
    if (!answer) continue;
    if (!answer.ok) {
      if (answer.reason === "forbidden") forbidden += 1;
      else if (answer.reason !== "not_found") unread.push(name);
      continue;
    }
    for (const r of answer.items) if (r.event_type === "DecisionMade") decisions.push({ record: r, agent: name, agentId: a.agent_id });
  }
  decisions.sort((x, y) => (x.record.recorded_at === y.record.recorded_at ? y.record.seq - x.record.seq : x.record.recorded_at < y.record.recorded_at ? 1 : -1));
  return (
    <Section title="Decisions, newest first">
      <p className="text-sm text-muted-foreground">Each decision among the latest {DECISIONS_TAIL} records of every agent&rsquo;s stream, as the journal holds it (times in UTC).</p>
      {!read.resolved ? <p className={NOT_REPORTED}>Reading the journal.</p> : null}
      {agents.length > 0 && forbidden === agents.length ? <p className={NOT_REPORTED}>Not available to your role.</p> : null}
      {decisions.length > 0 ? (
        <ol className="grid" aria-label="Gate decisions">
          {decisions.map((d) => (
            <DecisionLine key={d.record.event_id} record={d.record} agent={d.agent} agentId={d.agentId} now={now} />
          ))}
        </ol>
      ) : read.resolved && forbidden < agents.length && unread.length === 0 ? (
        <p className="text-muted-foreground">No decision recorded yet.</p>
      ) : null}
      {unread.length > 0 ? <p className={NOT_REPORTED}>Not reported: {unread.join(", ")}.</p> : null}
    </Section>
  );
}
