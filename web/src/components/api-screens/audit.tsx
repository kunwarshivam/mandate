"use client";

import { useEffect, useState } from "react";
import type { JournalLine } from "@/api/activity";
import { Section } from "@/components/screens/common";
import { clock } from "@/lib/format";
import { type ActivityAnswer, useRuntime } from "@/lib/mock-runtime";

/**
 * The audit timeline over the workspace API (§4.8.1, API-15, DEC-760): the latest records of the
 * workspace's control stream, each agent's stream, and the stream of each account the agents' reads
 * name, merged newest first by the time the journal recorded them. Each line is a record as the
 * journal holds it: its type, its stream, its sequence number and its time, never a body re-told in
 * other words. A stream that cannot be read is named as not reported, never left out silently.
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

type Tails = Readonly<Record<string, ActivityAnswer["journal"]>>;

/** The streams to read: the control stream, each agent's, then each account its agents' positions reads name. */
function useStreams(): { streams: Stream[] } {
  const { view, holdings, workspaceId } = useRuntime();
  const agents = view?.agents ?? [];
  const [accounts, setAccounts] = useState<readonly string[]>([]);
  const ids = agents.map((a) => a.agent_id).join("\n");
  useEffect(() => {
    if (!holdings || !ids) return;
    let live = true;
    void Promise.all(ids.split("\n").map((id) => holdings(id))).then(
      (answers) => {
        if (!live) return;
        const refs = new Set<string>();
        for (const a of answers)
          if (a.ok && a.account_ref !== null) refs.add(a.account_ref);
        setAccounts([...refs].sort());
      },
    );
    return () => {
      live = false;
    };
  }, [holdings, ids]);
  if (!workspaceId) return { streams: [] };
  return {
    streams: [
      { id: `ctl:${workspaceId}`, label: "Workspace" },
      ...agents.map((a) => ({
        id: `agent:${workspaceId}:${a.agent_id}`,
        label: a.label ?? a.agent_id,
      })),
      ...accounts.map((ref, i) => ({
        id: `acct:${workspaceId}:${ref}`,
        label: accounts.length > 1 ? `Account ${i + 1}` : "Account",
      })),
    ],
  };
}

function useTails(streams: readonly Stream[]): Tails {
  const { streamTail } = useRuntime();
  const [tails, setTails] = useState<Tails>({});
  const key = streams.map((s) => s.id).join("\n");
  useEffect(() => {
    if (!streamTail || !key) return;
    let live = true;
    const load = () => {
      for (const id of key.split("\n"))
        void streamTail(id, TIMELINE_TAIL).then(
          (answer) => live && setTails((all) => ({ ...all, [id]: answer })),
        );
    };
    load();
    const poll = window.setInterval(load, POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [streamTail, key]);
  return tails;
}

export function ApiTimeline() {
  const { streams } = useStreams();
  const tails = useTails(streams);
  const lines: Array<JournalLine & { stream: Stream }> = [];
  const unread: Stream[] = [];
  const forbidden: Stream[] = [];
  for (const stream of streams) {
    const tail = tails[stream.id];
    if (!tail) continue;
    if (!tail.ok)
      (tail.reason === "forbidden" ? forbidden : unread).push(stream);
    else for (const line of tail.items) lines.push({ ...line, stream });
  }
  lines.sort((a, b) =>
    a.recorded_at === b.recorded_at
      ? b.seq - a.seq
      : a.recorded_at < b.recorded_at
        ? 1
        : -1,
  );
  const reading = streams.filter((s) => !tails[s.id]);
  return (
    <Section title="Records, newest first">
      <p className="text-sm text-muted-foreground">
        The latest {TIMELINE_TAIL} records of each stream, as the journal holds
        them, with when it recorded each.
      </p>
      {forbidden.length > 0 && forbidden.length === streams.length ? (
        <p className={NOT_REPORTED}>Not available to your role.</p>
      ) : null}
      {lines.length > 0 ? (
        <ol className="grid" aria-label="Journal records">
          {lines.map((l) => (
            <li
              key={`${l.stream.id}#${l.seq}`}
              className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 border-b border-border/70 py-2.5 text-sm last:border-b-0"
              data-stream={l.stream.id} data-at={l.recorded_at}
              data-type={l.event_type}
            >
              <span>
                <span className="font-medium">{l.event_type}</span>{" "}
                <span className="text-muted-foreground">
                  in {l.stream.label}, #{l.seq}
                </span>
              </span>
              <span className="text-muted-foreground tabular-nums">
                {clock(l.recorded_at)} UTC
              </span>
            </li>
          ))}
        </ol>
      ) : reading.length === 0 &&
        unread.length === 0 &&
        forbidden.length < streams.length ? (
        <p className="text-muted-foreground">Nothing recorded yet.</p>
      ) : null}
      {reading.length > 0 ? (
        <p className={NOT_REPORTED}>
          Reading: {reading.map((s) => s.label).join(", ")}.
        </p>
      ) : null}
      {unread.length > 0 ? (
        <p className={NOT_REPORTED}>
          Not reported: {unread.map((s) => s.label).join(", ")}.
        </p>
      ) : null}
    </Section>
  );
}
