"use client";

import { type CSSProperties, useId } from "react";
import Link from "next/link";
import { VERDICT_COLUMN, VerdictChip } from "@/components/domain/gate-decision";
import { AgentOwl } from "@/components/domain/owl";
import { STRETCHED_LINK } from "@/components/domain/positions";
import { DayHeading, EntryTime } from "@/components/domain/timeline";
import type { GateDecision, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { RECORD_ZONE, type RecordEntry, byRecordDay } from "@/lib/format";
import { actionSentence, decidedRule, verdictBadge } from "@/lib/gate-reasons";
import { PURPOSE_LABEL, withRuleSentences } from "@/lib/labels";
import { mandateAt } from "@/lib/mandate-history";
import { useRuntime } from "@/lib/mock-runtime";
import { decisionHref } from "@/lib/screens";
import { cn } from "@/lib/utils";

/** How many of each verdict the timeline shows, in the order the gate's words are read. */
export function tally(decisions: GateDecision[]): string {
  const counts = new Map<string, number>();
  for (const d of decisions) {
    const label = verdictBadge(d);
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  const parts = ["Allowed", "Asked you", "Not allowed", "Held", "Waiting"].flatMap((label) => {
    const n = counts.get(label);
    return n ? [`${n} ${label.toLowerCase()}`] : [];
  });
  return `${decisions.length === 1 ? "1 decision" : `${decisions.length} decisions`}: ${parts.join(", ")}`;
}

/**
 * What the agents set out to do, newest first, narrow enough for a rail or a phone: each entry hangs
 * off one hairline by the agent's owl, with the action, the gate's verdict in words, who and when, and
 * one line of why: the rule that held it, or else what happened next, the owner's rules named by
 * their sentence (critique C-6) where the audit keeps their ids. A rule reads as the mandate version
 * the gate decided under said it, never as the agent's current version says it; a decision whose
 * version is not in the agent's history keeps the recorded id. The verdict sits first, in
 * one column of one width on every row, so it never wraps and the eye reads it down the list
 * (DEC-512); it wears no meaning colour. Entries sit under a heading for each day ("Today",
 * "25 September") with times alone under each (C-19); the day and the time are both read in Eastern
 * time, the record's zone, and "Today" is the runtime's now.
 */
export function DecisionTimeline({ ws, decisions }: { ws: Workspace; decisions: GateDecision[] }) {
  const { now } = useRuntime();
  const id = useId();
  const days = byRecordDay(decisions, (d) => d.at, now, RECORD_ZONE);
  const firstIndex = days.map((_, n) => days.slice(0, n).reduce((count, day) => count + day.entries.length, 0));
  return (
    <ol aria-label="Decisions, newest first" data-slot="decision-timeline" className={cn("grid gap-2", VERDICT_COLUMN)}>
      {days.map((day, n) => (
        <li key={`${n}-${day.heading}`} className="grid gap-1">
          <DayHeading level={3} id={`${id}-day-${n}`}>
            {day.heading}
          </DayHeading>
          <ol aria-labelledby={`${id}-day-${n}`} className="grid">
            {day.entries.map((entry, e) => (
              <DecisionEntry key={entry.item.event_id} ws={ws} entry={entry} index={firstIndex[n] + e} />
            ))}
          </ol>
        </li>
      ))}
    </ol>
  );
}

/** One decision under its day's heading; `index` counts down the whole timeline, for the reveal's stagger. */
function DecisionEntry({ ws, entry: { item: d, date, time }, index }: { ws: Workspace; entry: RecordEntry<GateDecision>; index: number }) {
  const agent = findAgent(ws, d.agent_id);
  const rule = agent ? decidedRule(d, agent) : null;
  const decidedUnder = agent ? mandateAt(agent, d.mandate_version) : null;
  return (
    <li
      data-verdict={d.verdict}
      data-slot="timeline-entry"
      className="group/entry reveal relative -mx-2 grid grid-cols-[2rem_minmax(0,1fr)] gap-x-3 rounded-xl px-2 transition-colors duration-(--duration-hover) hover:bg-background"
      style={{ "--i": index + 1 } as CSSProperties}
    >
      <span aria-hidden className="relative flex justify-center pt-2.5 before:absolute before:top-12 before:bottom-0 before:w-px before:bg-border group-last/entry:before:hidden">
        {agent ? <AgentOwl agent={agent} still className="size-8" /> : <span className="mt-3 size-2 rounded-full bg-muted-foreground" />}
      </span>
      <div className="grid min-w-0 content-start gap-1 pt-3 pb-4">
        <p className="grid grid-cols-[var(--verdict-w)_minmax(0,1fr)] items-start gap-x-2">
          <VerdictChip decision={d} />
          <Link href={decisionHref(d.agent_id, d.event_id)} className={cn("font-medium text-pretty underline-offset-4 group-hover/entry:underline after:rounded-xl", STRETCHED_LINK)}>
            {actionSentence(d.action)}
          </Link>
        </p>
        <p className="flex flex-wrap gap-x-1.5 text-caption text-muted-foreground">
          <span className="font-medium text-foreground">{agent?.label ?? "An agent"}</span>
          <span aria-hidden>·</span>
          <span>{PURPOSE_LABEL[d.action.purpose]}</span>
          <span aria-hidden>·</span>
          <EntryTime at={d.at} date={date} time={time.slice(0, 5)} className="font-mono tabular" />
        </p>
        {rule ? (
          <p className="text-sm text-pretty" data-slot="gate-rule">
            {rule}
          </p>
        ) : d.then ? (
          <p className="text-sm text-pretty text-muted-foreground">{decidedUnder ? withRuleSentences(d.then, decidedUnder.autonomy.rules) : d.then}</p>
        ) : null}
      </div>
    </li>
  );
}
