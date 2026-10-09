import type { TimelineEvent } from "@/fixtures/types";
import { RECORD_ZONE, datedClock } from "@/lib/format";

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

export function Timeline({ events, now }: { events: TimelineEvent[]; now: string }) {
  if (events.length === 0) return <p className="text-sm text-muted-foreground">Nothing recorded yet.</p>;
  return (
    <ol className="grid gap-4 border-l border-border pl-5">
      {events.map((e) => (
        <li key={e.event_id} className="relative grid gap-0.5">
          <span className="absolute top-1.5 -left-[1.5625rem] size-2 rounded-full bg-muted-foreground ring-4 ring-card" aria-hidden />
          <p className="flex flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground">
            <time dateTime={e.at} className="font-mono tabular">
              {datedClock(e.at, now, RECORD_ZONE)}
            </time>
            <span className="text-label text-foreground">{KIND_LABEL[e.kind]}</span>
          </p>
          <p className="text-sm">{e.text}</p>
        </li>
      ))}
    </ol>
  );
}
