"use client";

import { useEffect, useRef, useState } from "react";
import { type BindingVerdict, type Decision, decisionsOf } from "@/api/decisions";
import { Section } from "@/components/screens/common";
import type { ReasonCode } from "@/fixtures/types";
import { datedClock, price, quantity } from "@/lib/format";
import { gateRule } from "@/lib/gate-reasons";
import { type RecordsAnswer, useRuntime } from "@/lib/mock-runtime";

/**
 * Gate decisions over the workspace API (§4.8.1; journal spec §9.1, §9.5): each agent's decisions
 * from the latest records of its stream, newest first, each joined to the binding gate's verdict on
 * the intent it proposed, from the latest records of its account's stream. Every record is read
 * only once its body matched its hash. What the dry run answered is never shown as the binding
 * gate's answer, and a verdict the records read do not hold is said to be not found, never assumed.
 */

/** How many of each stream's latest records are read: the page's most (§4.8.1). */
export const DECISION_RECORDS = 1000;
const POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";

const DRY_RUN: Readonly<Record<Decision["dry_run"], string>> = { allow: "The dry run allowed it", deny: "The dry run denied it", defer: "The dry run left it to the binding gate" };
const SUPPRESSED: Readonly<Record<NonNullable<Decision["ask_suppressed"]>, string>> = {
  budget: "the day's limit of approval requests was reached",
  skipped_today: "you skipped this instrument today",
  recent_timeout: "a recent request for it timed out",
};

/** What autonomy chose (mandate spec §6.2), in words that claim no action the records do not show. */
function autonomyWords(d: Decision): string {
  switch (d.autonomy) {
    case null:
      return "";
    case "auto":
      return "; it was within what the agent may do without asking";
    case "deny":
      return "; the agent was not allowed to place it";
    case "ask":
      return d.ask_suppressed
        ? `; it needed your approval and you were not asked, since ${SUPPRESSED[d.ask_suppressed]}, and no order is placed without it`
        : "; it asked you for approval. If you approved it, the order and the binding gate's verdict are with that approval";
  }
}

/** What decided autonomy (journal spec §9.1 `decided_by`), in words. */
function decidedByWords(code: string): string {
  if (code === "builtin_risk_reducing") return "the built-in rule that a risk-reducing order needs no approval";
  if (code === "default") return "your mandate's default";
  if (code.startsWith("rule:")) return `your mandate's rule ${code.slice(5)}`;
  if (code.startsWith("delegation:")) return "a delegation you granted";
  if (code === "admission_ceiling") return "the ceiling on new instruments";
  if (code === "client_ceiling") return "the ceiling on a connected client's orders";
  if (code === "review_ceiling") return "the ceiling after your review date passed";
  return code.replaceAll("_", " ");
}
const ASKED_BY: Readonly<Record<Decision["requested_by"], string>> = { agent: "", owner: " You asked for this order.", client: " A connected client asked for this order." };
const BINDING: Readonly<Record<BindingVerdict["verdict"], string>> = { allow: "The binding gate allowed it", deny: "The binding gate denied it", hold: "The binding gate held it", defer: "The binding gate deferred it" };
const PURPOSE: Readonly<Record<string, string>> = { open: "open", increase: "add", discretionary_exit: "exit", risk_exit: "risk exit" };

/** The gate's reason in words where the platform has them; otherwise its code, as the record holds it. */
function reasonWords(code: string): string {
  try {
    return gateRule(code as ReasonCode, null);
  } catch {
    return `Reason: ${code}.`;
  }
}

const words = (code: string) => code.replaceAll("_", " ");

/** What was read of an agent's account stream: its records' window, or that they were not read. */
type AccountRead = { read: true; fromStart: boolean } | { read: false };

function DecisionLine({ d, agent, agentId, account, now }: { d: Decision; agent: string; agentId: string; account: AccountRead; now: string }) {
  const failed = d.binding?.checks.filter((c) => c.result !== "pass") ?? [];
  return (
    <li className="grid gap-1 border-b border-border/70 py-3 text-sm last:border-b-0" data-dry-run={d.dry_run} data-binding={d.binding?.verdict ?? "none"} data-event={d.event_id} data-agent={agentId}>
      <span className="flex flex-wrap justify-between gap-x-3">
        <span className="font-medium">
          {agent}: {d.side} {quantity(d.qty)} {d.instrument_id}
          {d.limit_price !== null ? ` at a limit of ${price(d.limit_price)}` : ""}
          {PURPOSE[d.purpose] ? ` (${PURPOSE[d.purpose]})` : ""}
        </span>
        <span className="text-muted-foreground tabular-nums">{datedClock(d.recorded_at, now, "UTC")} UTC</span>
      </span>
      <span>
        {DRY_RUN[d.dry_run]}
        {autonomyWords(d)}.{ASKED_BY[d.requested_by]}
      </span>
      {d.decided_by ? <span className="text-muted-foreground">Decided by {decidedByWords(d.decided_by)}.</span> : null}
      {d.reason_code ? <span className="text-muted-foreground">{reasonWords(d.reason_code)}</span> : null}
      {d.deferred.length > 0 ? <span className="text-muted-foreground">{`Left to the executor's binding gate, which the dry run could not see: ${d.deferred.map(words).join(", ")}.`}</span> : null}
      {d.binding ? (
        <span data-slot="binding">
          {BINDING[d.binding.verdict]}
          {d.binding.reason_code ? `: ${reasonWords(d.binding.reason_code)}` : "."} It ran {d.binding.checks.length === 1 ? "1 check" : `${d.binding.checks.length} checks`}
          {failed.length > 0 ? `; not passed: ${failed.map((c) => `${words(c.id)} (${c.result})`).join(", ")}.` : "."}
        </span>
      ) : d.intent ? (
        <span className={NOT_REPORTED} data-slot="binding">
          {!account.read
            ? "Sent to the executor; the binding gate's verdict was not read: its account's records are not available."
            : account.fromStart
              ? "Sent to the executor; its account's records hold no verdict for it yet."
              : `Sent to the executor; its verdict is not among the account's latest ${DECISION_RECORDS.toLocaleString("en-US")} records read.`}
        </span>
      ) : d.dry_run === "allow" && d.autonomy === "auto" ? (
        <span className={NOT_REPORTED} data-slot="binding">
          {"No order for it is recorded on the agent's stream."}
        </span>
      ) : null}
    </li>
  );
}

interface AgentRead {
  agent: RecordsAnswer;
  /** `null` when no account stream could be named for the agent. */
  account: RecordsAnswer | null;
}

export function ApiDecisions() {
  const { view, streamRecords, holdings, workspaceId, now } = useRuntime();
  const agents = view?.agents ?? [];
  const ids = agents.map((a) => a.agent_id).join("\n");
  const [read, setRead] = useState<{ answers: Readonly<Record<string, AgentRead>>; resolved: boolean }>({ answers: {}, resolved: false });
  const generation = useRef(0);
  useEffect(() => {
    if (!streamRecords || !holdings || !workspaceId) return;
    let live = true;
    const one = async (id: string): Promise<AgentRead> => {
      const [agent, held] = await Promise.all([streamRecords(`agent:${workspaceId}:${id}`, DECISION_RECORDS), holdings(id)]);
      const ref = held.ok ? held.account_ref : null;
      const account = ref ? await streamRecords(`acct:${workspaceId}:${ref}`, DECISION_RECORDS) : null;
      return { agent, account };
    };
    const load = async () => {
      generation.current += 1;
      const mine = generation.current;
      const listed = ids ? ids.split("\n") : [];
      const answers = await Promise.all(listed.map(one));
      if (!live || mine !== generation.current) return;
      setRead({ answers: Object.fromEntries(listed.map((id, i) => [id, answers[i]])), resolved: true });
    };
    void load();
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [streamRecords, holdings, workspaceId, ids]);
  const lines: Array<{ d: Decision; agent: string; agentId: string; account: AccountRead }> = [];
  const unread: string[] = [];
  const windowed: string[] = [];
  const verdictsUnread: string[] = [];
  let unreadable = 0;
  let forbidden = 0;
  for (const a of agents) {
    const answer = read.answers[a.agent_id];
    const name = a.label ?? a.agent_id;
    if (!answer) continue;
    if (!answer.agent.ok) {
      if (answer.agent.reason === "forbidden") forbidden += 1;
      else if (answer.agent.reason !== "not_found") unread.push(name);
      continue;
    }
    if (!answer.agent.fromStart) windowed.push(`${name}'s stream`);
    const accountRead: AccountRead = answer.account && answer.account.ok ? { read: true, fromStart: answer.account.fromStart } : { read: false };
    if (accountRead.read && !accountRead.fromStart) windowed.push(`${name}'s account`);
    const found = decisionsOf(answer.agent.items, answer.account && answer.account.ok ? answer.account.items : null);
    unreadable += found.unreadable;
    if (!accountRead.read && found.decisions.some((d) => d.intent)) verdictsUnread.push(name);
    for (const d of found.decisions) lines.push({ d, agent: name, agentId: a.agent_id, account: accountRead });
  }
  lines.sort((x, y) => Date.parse(y.d.recorded_at.slice(0, 23) + "Z") - Date.parse(x.d.recorded_at.slice(0, 23) + "Z"));
  const allForbidden = agents.length > 0 && forbidden === agents.length;
  return (
    <Section title="Decisions, newest first">
      <p className="text-sm text-muted-foreground">Each agent&rsquo;s decisions as its journal holds them, with the binding gate&rsquo;s verdict on each order it proposed (times in UTC).</p>
      {!read.resolved ? <p className={NOT_REPORTED}>Reading the journal.</p> : null}
      {allForbidden ? <p className={NOT_REPORTED}>Not available to your role.</p> : null}
      {windowed.length > 0 ? (
        <p className="text-sm text-muted-foreground" data-slot="decisions-window">
          Read from the latest {DECISION_RECORDS.toLocaleString("en-US")} records of {windowed.join(", ")}; an earlier decision or verdict is not shown.
        </p>
      ) : null}
      {unreadable > 0 ? (
        <p className="text-sm font-medium" data-slot="decisions-unreadable">
          {unreadable === 1 ? "1 decision record" : `${unreadable} decision records`} could not be checked against {unreadable === 1 ? "its" : "their"} hash or read, and {unreadable === 1 ? "is" : "are"} not shown.
        </p>
      ) : null}
      {lines.length > 0 ? (
        <ol className="grid" aria-label="Gate decisions">
          {lines.map((l) => (
            <DecisionLine key={l.d.event_id} d={l.d} agent={l.agent} agentId={l.agentId} account={l.account} now={now} />
          ))}
        </ol>
      ) : read.resolved && !allForbidden && unread.length === 0 && unreadable === 0 ? (
        <p className="text-muted-foreground">No decision recorded yet.</p>
      ) : null}
      {unread.length > 0 ? <p className={NOT_REPORTED}>Not reported: {unread.join(", ")}.</p> : null}
      {verdictsUnread.length > 0 ? (
        <p className={NOT_REPORTED} data-slot="verdicts-unread">
          The binding gate&rsquo;s verdicts were not read for {verdictsUnread.join(", ")}: the account&rsquo;s records are not available.
        </p>
      ) : null}
    </Section>
  );
}
