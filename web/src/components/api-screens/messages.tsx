"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import type { JournalLine } from "@/api/activity";
import { ALERT_KINDS, type Alert } from "@/api/alerts";
import type { AgentDetail, ApprovalDetail } from "@/api/read-models";
import { ModeBadge } from "@/components/domain/mode";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { ApiNotYetNotice, Section, WorkspaceGate } from "@/components/screens/common";
import { datedClock } from "@/lib/format";
import { type ActivityAnswer, type AlertsAnswer, useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";

/**
 * Messages over the workspace API (DEC-479): each agent's thread is its journal, read from what the
 * deployment serves (§4.7, §4.8.1). A thread is the agent stream's latest records, oldest first, by
 * type and the time the journal recorded each, never a body retold in other words. A record that is
 * one of the agent's alerts (`GET /alerts`, DEC-2141) is named by its kind, and a request that still
 * waits for the owner links to it. The desk's questions and answers are not served, so the desk
 * says so rather than showing an example. A read that fails is named, never shown as an empty thread.
 */

/** How many of the agent stream's latest records a thread reads. */
export const THREAD_TAIL = 50;
/** How often an open thread is read again. */
const POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";

interface ThreadRead {
  journal: ActivityAnswer["journal"] | null;
  alerts: AlertsAnswer | null;
}

/** Reads the open thread's records and the workspace's alerts on open and every few seconds; a slower earlier read never replaces a later one. */
function useThread(agentId: string): ThreadRead {
  const { streamTail, alerts, workspaceId } = useRuntime();
  const [read, setRead] = useState<ThreadRead>({ journal: null, alerts: null });
  const generation = useRef(0);
  useEffect(() => {
    if (!streamTail || !alerts || !workspaceId) return;
    let live = true;
    const load = async () => {
      generation.current += 1;
      const mine = generation.current;
      const [journal, alerted] = await Promise.all([streamTail(`agent:${workspaceId}:${agentId}`, THREAD_TAIL), alerts()]);
      if (live && mine === generation.current) setRead({ journal, alerts: alerted });
    };
    void load();
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [streamTail, alerts, workspaceId, agentId]);
  return read;
}

function nameOf(agent: AgentDetail): string {
  return agent.label ?? agent.agent_id;
}

function waiting(agent: AgentDetail): string {
  return agent.pending_approvals === 0 ? "Nothing waits for you" : agent.pending_approvals === 1 ? "1 request waits for you" : `${agent.pending_approvals} requests wait for you`;
}

function Threads({ agents, current }: { agents: AgentDetail[]; current: string | null }) {
  if (agents.length === 0) return <p className="text-muted-foreground">Your deployment reports no agents.</p>;
  const ordered = [...agents].sort((a, b) => b.pending_approvals - a.pending_approvals);
  return (
    <ul className="grid" aria-label="Threads">
      {ordered.map((agent) => (
        <li key={agent.agent_id} data-slot="api-thread">
          <Link
            href={`/messages/${encodeURIComponent(agent.agent_id)}`}
            aria-current={agent.agent_id === current ? "page" : undefined}
            className={cn("press -mx-3 grid gap-1 rounded-xl px-3 py-3 hover:bg-background", agent.agent_id === current && "bg-background")}
          >
            <span className="flex flex-wrap items-center gap-2">
              <span className="font-semibold">{nameOf(agent)}</span>
              {agent.mode ? <ModeBadge mode={agent.mode} /> : <span className={cn("text-sm", NOT_REPORTED)}>Mode not reported</span>}
            </span>
            <span className="text-sm text-muted-foreground">{waiting(agent)}</span>
          </Link>
        </li>
      ))}
    </ul>
  );
}

/** One record of the thread: an alert by its kind's name, a waiting request with its link, else the record's type. */
function Record({ line, alert, request, now }: { line: JournalLine; alert: Alert | undefined; request: ApprovalDetail | undefined; now: string }) {
  const at = <span className="text-muted-foreground tabular-nums">{datedClock(line.recorded_at, now, "UTC")}</span>;
  if (alert) {
    return (
      <li data-slot="thread-record" data-type={line.event_type} data-kind={alert.kind} className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 py-2.5 text-sm">
        <span className="flex flex-wrap items-center gap-2">
          <span className="rounded-sm border border-foreground px-1.5 text-label">Alert</span>
          <span className="font-medium">{ALERT_KINDS[alert.kind].label}</span>
        </span>
        {at}
      </li>
    );
  }
  if (request) {
    return (
      <li data-slot="thread-record" data-type={line.event_type} className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 py-2.5 text-sm">
        <Link href={`/approvals/${encodeURIComponent(request.approval_id)}`} className="font-medium text-lapis underline-offset-4 hover:underline">
          A request waits for you
        </Link>
        {at}
      </li>
    );
  }
  return (
    <li data-slot="thread-record" data-type={line.event_type} className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 py-2.5 text-sm">
      <span className="font-medium">{line.event_type}</span>
      {at}
    </li>
  );
}

function Thread({ agent, approvals }: { agent: AgentDetail; approvals: ApprovalDetail[] }) {
  const { now } = useRuntime();
  const { journal, alerts } = useThread(agent.agent_id);
  const alertsById = new Map<string, Alert>((alerts?.ok ? alerts.read.alerts : []).map((a) => [a.alert_id, a]));
  const pending = new Map(approvals.filter((a) => a.agent_id === agent.agent_id && a.state === "pending").map((a) => [a.approval_id, a]));
  return (
    <Section title={`${nameOf(agent)}'s thread`} id="api-thread">
      <p className="text-sm text-muted-foreground">The agent&apos;s latest {THREAD_TAIL} records, as its journal holds them, oldest first, with when it recorded each (UTC).</p>
      {journal === null ? (
        <p className={NOT_REPORTED}>Reading the thread.</p>
      ) : !journal.ok ? (
        <p className={NOT_REPORTED}>{journal.reason === "not_found" ? "Nothing recorded yet." : journal.reason === "forbidden" ? "Not available to your role." : `The thread is not reported: your deployment answered ${journal.reason}.`}</p>
      ) : journal.items.length === 0 ? (
        <p className="text-muted-foreground">Nothing recorded yet.</p>
      ) : (
        <ol className="grid divide-y divide-border/70" aria-label="Thread records">
          {journal.items.map((line) => (
            <Record key={line.seq} line={line} alert={alertsById.get(line.event_id)} request={pending.get(line.event_id)} now={now} />
          ))}
        </ol>
      )}
      {alerts !== null && !alerts.ok ? <p className={cn("text-sm", NOT_REPORTED)}>Alerts not reported, so an alert shows by its record&apos;s type.</p> : null}
    </Section>
  );
}

function Messages({ agentId, view }: { agentId: string | null; view: "chat" | "desk" }) {
  const { view: read } = useRuntime();
  if (!read) return null;
  const agent = agentId ? read.agents.find((a) => a.agent_id === agentId) : undefined;
  return (
    <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[19rem_minmax(0,1fr)] lg:gap-x-10">
      <Section title="Threads" id="api-threads">
        <Threads agents={read.agents} current={agent?.agent_id ?? null} />
      </Section>
      {agentId === null ? (
        <p className="text-muted-foreground max-lg:hidden">Pick a thread: each agent&apos;s is its journal.</p>
      ) : !agent ? (
        <p className={NOT_REPORTED}>Your deployment reports no such agent.</p>
      ) : view === "desk" ? (
        <ApiNotYetNotice />
      ) : (
        <Thread agent={agent} approvals={read.approvals} />
      )}
    </div>
  );
}

export function ApiMessages({ agentId = null, view = "chat" }: { agentId?: string | null; view?: "chat" | "desk" }) {
  return (
    <div className="grid">
      <PageHeader title="Messages" description="Every agent's thread, read from its journal. Requests waiting for you come first." />
      <WorkspaceGate api>
        <Messages agentId={agentId} view={view} />
      </WorkspaceGate>
    </div>
  );
}
