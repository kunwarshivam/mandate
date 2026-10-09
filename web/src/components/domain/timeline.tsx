import { useId } from "react";
import type { TimelineEvent } from "@/fixtures/types";
import { RECORD_ZONE, byRecordDay } from "@/lib/format";

export const KIND_LABEL: Record<TimelineEvent["kind"], string> = {
  fill: "Fill",
  order: "Order",
  mode: "Mode",
  approval: "Approval",
  gate: "Gate",
  protection: "Protection",
  reconciliation: "Reconciliation",
  version: "Version",
};

/** A day's heading on a timeline, one level below the section it sits in; its list takes its name. */
export function DayHeading({ level, id, children }: { level: 2 | 3; id: string; children: string }) {
  const Tag = level === 2 ? "h2" : "h3";
  return (
    <Tag id={id} data-slot="day-heading" className="text-label text-foreground">
      {children}
    </Tag>
  );
}

/** An entry's time under its day's heading: the clock alone on screen, the full date and time for assistive tech. */
export function EntryTime({ at, date, time, className }: { at: string; date: string; time: string; className?: string }) {
  return (
    <time dateTime={at} className={className}>
      <span data-slot="full-date" className="sr-only">
        {`${date}, `}
      </span>
      {time}
    </time>
  );
}

/**
 * The record of what happened, newest first, under a heading for each day ("Today", "25 September")
 * with times alone under each (C-19). The day and the time are both read in Eastern time, the
 * record's zone, and "Today" is the `now` the caller passes.
 */
export function Timeline({ events, now, headingLevel = 3 }: { events: TimelineEvent[]; now: string; headingLevel?: 2 | 3 }) {
  const id = useId();
  if (events.length === 0) return <p className="text-sm text-muted-foreground">Nothing recorded yet.</p>;
  return (
    <ol className="grid gap-5">
      {byRecordDay(events, (e) => e.at, now, RECORD_ZONE).map((day, n) => (
        <li key={`${n}-${day.heading}`} className="grid gap-3">
          <DayHeading level={headingLevel} id={`${id}-day-${n}`}>
            {day.heading}
          </DayHeading>
          <ol aria-labelledby={`${id}-day-${n}`} className="grid gap-4 border-l border-border pl-5">
            {day.entries.map(({ item: e, date, time }) => (
              <li key={e.event_id} data-slot="record-entry" className="relative grid gap-0.5">
                <span className="absolute top-1.5 -left-[1.5625rem] size-2 rounded-full bg-muted-foreground ring-4 ring-card" aria-hidden />
                <p className="flex flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground">
                  <EntryTime at={e.at} date={date} time={time} className="font-mono tabular" />
                  <span className="text-label text-foreground">{KIND_LABEL[e.kind]}</span>
                </p>
                <p className="text-sm">{e.text}</p>
              </li>
            ))}
          </ol>
        </li>
      ))}
    </ol>
  );
}
