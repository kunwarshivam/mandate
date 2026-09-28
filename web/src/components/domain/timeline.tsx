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
    <ol className="grid gap-3 border-l-2 border-foreground pl-4">
      {events.map((e) => (
        <li key={e.event_id} className="relative grid gap-0.5">
          <span className="absolute top-1.5 -left-[1.3125rem] size-2 bg-foreground" aria-hidden />
          <p className="flex flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground">
            <time dateTime={e.at} className="font-mono tabular">
              {e.at.slice(0, 10) === today ? clock(e.at) : `${dateLabel(e.at)}, ${clock(e.at).slice(0, 5)}`}
            </time>
            <span className="field-label text-foreground">{KIND_LABEL[e.kind]}</span>
          </p>
          <p className="text-sm">{e.text}</p>
        </li>
      ))}
    </ol>
  );
}
