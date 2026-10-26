"use client";

import { useEffect, useRef, useState } from "react";
import type { JournalLine } from "@/api/activity";
import type { AgentDetail } from "@/api/read-models";
import { Section } from "@/components/screens/common";
import { datedClock } from "@/lib/format";
import { type ActivityAnswer, useRuntime } from "@/lib/mock-runtime";

/**
 * The audit timeline over the workspace API (§4.8.1, API-15, DEC-760): the latest records of the
 * workspace's control stream, each agent's stream, and the stream of each account the agents'
 * positions reads name, merged newest first by the time the journal recorded them. Each line is a
 * record as the journal holds it: its type, its stream, its sequence number and its time, never a
 * body re-told in other words. A stream, or an agent's account, that cannot be read is named, never
 * left out silently.
 */

/** How many of each stream's latest records the timeline reads. */
export const TIMELINE_TAIL = 50;
/** How often the timeline is read again while it is open. */
const POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";

interface Stream {
  id: string;
  /** What the owner calls it: the workspace, an agent's name, or an account. */
  label: string;
}

type Tail = ActivityAnswer["journal"];

interface Read {
  /** Each stream's latest records, or why not. */
  tails: Readonly<Record<string, Tail>>;
  /** The account streams the agents' positions reads named, in order. */
  accounts: readonly string[];
  /** The ids of agents whose positions read failed, so their account may be missing. */
  unresolved: readonly string[];
  /** Whether a whole load has answered once. */
  resolved: boolean;
}

/**
 * Reads the streams on open and every few seconds: first each agent's positions, for the accounts
 * they name, then every stream's tail. Each load is numbered, and a slower earlier one never
 * replaces a later one.
 */
function useTimeline(): Read & { streams: Stream[]; agents: readonly AgentDetail[] } {
  const { view, holdings, streamTail, workspaceId } = useRuntime();
  const agents = view?.agents ?? [];
  const [read, setRead] = useState<Read>({ tails: {}, accounts: [], unresolved: [], resolved: false });
  const generation = useRef(0);
  const ids = agents.map((a) => a.agent_id).join("\n");
  useEffect(() => {
    if (!holdings || !streamTail || !workspaceId) return;
    let live = true;
    const load = async () => {
      generation.current += 1;
      const mine = generation.current;
      const listed = ids ? ids.split("\n") : [];
      const answers = await Promise.all(listed.map((id) => holdings(id)));
      const refs = new Set<string>();
      const failed: string[] = [];
      answers.forEach((a, i) => {
        if (!a.ok) failed.push(listed[i]);
        else if (a.account_ref !== null) refs.add(a.account_ref);
      });
      const accounts = [...refs].sort();
      const streamIds = [`ctl:${workspaceId}`, ...listed.map((id) => `agent:${workspaceId}:${id}`), ...accounts.map((ref) => `acct:${workspaceId}:${ref}`)];
      const tails = await Promise.all(streamIds.map((id) => streamTail(id, TIMELINE_TAIL)));
      if (!live || mine !== generation.current) return;
      setRead({ tails: Object.fromEntries(streamIds.map((id, i) => [id, tails[i]])), accounts, unresolved: failed, resolved: true });
    };
    void load();
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [holdings, streamTail, workspaceId, ids]);
  const streams: Stream[] = workspaceId
    ? [
        { id: `ctl:${workspaceId}`, label: "Workspace" },
        ...agents.map((a) => ({ id: `agent:${workspaceId}:${a.agent_id}`, label: a.label ?? a.agent_id })),
        ...read.accounts.map((ref, i) => ({ id: `acct:${workspaceId}:${ref}`, label: read.accounts.length > 1 ? `Account ${i + 1}` : "Account" })),
      ]
    : [];
  return { ...read, streams, agents };
}

export function ApiTimeline() {
  const { now } = useRuntime();
  const { streams, tails, unresolved, resolved, agents } = useTimeline();
  const lines: Array<JournalLine & { stream: Stream }> = [];
  const unread: Stream[] = [];
  const empty: Stream[] = [];
  const forbidden: Stream[] = [];
  for (const stream of streams) {
    const tail = tails[stream.id];
    if (!tail) continue;
    if (tail.ok) for (const line of tail.items) lines.push({ ...line, stream });
    else if (tail.reason === "forbidden") forbidden.push(stream);
    else if (tail.reason === "not_found") empty.push(stream);
    else unread.push(stream);
  }
  lines.sort((a, b) => (a.recorded_at === b.recorded_at ? b.seq - a.seq : a.recorded_at < b.recorded_at ? 1 : -1));
  const labels = (list: readonly Stream[]) => list.map((s) => s.label).join(", ");
  const everyForbidden = streams.length > 0 && forbidden.length === streams.length;
  const unresolvedNames = agents.filter((a) => unresolved.includes(a.agent_id)).map((a) => a.label ?? a.agent_id);
  return (
    <Section title="Records, newest first">
      <p className="text-sm text-muted-foreground">The latest {TIMELINE_TAIL} records of each stream, as the journal holds them, with when it recorded each (UTC).</p>
      {everyForbidden ? <p className={NOT_REPORTED}>Not available to your role.</p> : null}
      {lines.length > 0 ? (
        <ol className="grid" aria-label="Journal records">
          {lines.map((l) => (
            <li
              key={`${l.stream.id}#${l.seq}`}
              className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 border-b border-border/70 py-2.5 text-sm last:border-b-0"
              data-stream={l.stream.id}
              data-type={l.event_type}
              data-at={l.recorded_at}
            >
              <span>
                <span className="font-medium">{l.event_type}</span> <span className="text-muted-foreground">in {l.stream.label}, #{l.seq}</span>
              </span>
              <span className="text-muted-foreground tabular-nums">{datedClock(l.recorded_at, now, "UTC")}</span>
            </li>
          ))}
        </ol>
      ) : resolved && !everyForbidden && unread.length === 0 && unresolved.length === 0 ? (
        <p className="text-muted-foreground">Nothing recorded yet.</p>
      ) : null}
      {!resolved ? <p className={NOT_REPORTED}>Reading the journal.</p> : null}
      {empty.length > 0 ? <p className="text-sm text-muted-foreground">No records yet: {labels(empty)}.</p> : null}
      {forbidden.length > 0 && !everyForbidden ? <p className={NOT_REPORTED}>Not available to your role: {labels(forbidden)}.</p> : null}
      {unread.length > 0 ? <p className={NOT_REPORTED}>Not reported: {labels(unread)}.</p> : null}
      {unresolvedNames.length > 0 ? <p className={NOT_REPORTED}>The account of {unresolvedNames.join(", ")} is not reported, so its records may be missing.</p> : null}
    </Section>
  );
}
