import type { TimelineEvent } from "@/fixtures/types";
import { clock, dateLabel } from "@/lib/format";

const KIND_LABEL: Record<TimelineEvent["kind"], string> = {
  fill: "Fill",
  order: "Order",
  mode: "Mode",
  approval: "Approval",
  gate: "Gate",
  protection: "Protection",
  reconciliation: "Reconciliation",
  version: "Version",
};

export function Timeline({ events, today }: { events: TimelineEvent[]; today: string }) {
  if (events.length === 0) return <p className="text-sm text-muted-foreground">Nothing recorded yet.</p>;
  return (
    <ol className="relative grid gap-4 border-l border-border pl-4">
      {events.map((e) => (
        <li key={e.event_id} className="relative grid gap-0.5">
          <span className="absolute top-1.5 -left-[1.3rem] size-2 rounded-full bg-border ring-4 ring-card" aria-hidden />
          <p className="text-caption text-muted-foreground">
            <time dateTime={e.at} className="font-mono tabular">
              {e.at.slice(0, 10) === today ? clock(e.at) : `${dateLabel(e.at)}, ${clock(e.at).slice(0, 5)}`}
            </time>{" "}
            {KIND_LABEL[e.kind]}
          </p>
          <p className="text-sm">{e.text}</p>
        </li>
      ))}
    </ol>
  );
}
