"use client";

import { FixtureTag } from "@/components/domain/placeholders";
import { headlinesFor } from "@/fixtures/news";
import type { Workspace } from "@/fixtures/types";
import { ago } from "@/lib/format";
import { type SliceTone, assetTones } from "@/lib/holdings";
import { cn } from "@/lib/utils";
import { Section } from "./common";

const TAG: Record<SliceTone, string> = {
  "series-1": "bg-series-1",
  "series-2": "bg-series-2",
  "series-3": "bg-series-3",
  "series-4": "bg-series-4",
  "series-5": "bg-series-5",
  muted: "bg-muted",
};

/** Every instrument on the account or in a mandate: what the headlines are filtered by. */
function symbolsOf(ws: Workspace): Set<string> {
  return new Set([
    ...ws.agents.flatMap((a) => a.positions.map((p) => p.instrument.symbol)),
    ...ws.agents.flatMap((a) => a.mandate.universe.pinned_instruments.map((i) => i.symbol)),
    ...ws.external_positions.map((e) => e.instrument.symbol),
  ]);
}

/**
 * Headlines about what the account holds or may trade, newest first, each with its source, age and
 * instruments. Sample text until a news source is connected: labelled so, and never a link.
 */
export function NewsSection({ ws, now, className }: { ws: Workspace; now: string; className?: string }) {
  const tones = assetTones(ws);
  const items = headlinesFor(symbolsOf(ws));
  return (
    <Section title="News" action={<FixtureTag />} className={className}>
      {items.length === 0 ? (
        <p className="text-sm text-muted-foreground">No headlines about what your agents hold or may trade.</p>
      ) : (
        <ol aria-label="Sample headlines" className="grid">
          {items.map((h) => (
            <li key={h.id} data-slot="headline" className="grid gap-1.5 border-b border-border/70 py-3.5 last:border-b-0">
              <p className="flex flex-wrap items-center gap-x-2 text-caption text-muted-foreground">
                <span className="font-medium text-foreground">{h.source}</span>
                <span aria-hidden>·</span>
                <time dateTime={h.at}>{ago(h.at, now)}</time>
              </p>
              <p className="font-medium text-pretty">{h.title}</p>
              <p className="flex flex-wrap gap-1.5">
                {h.symbols.map((s) => {
                  const tone = tones.get(s);
                  return (
                    <span key={s} className="inline-flex h-5 items-center gap-1.5 rounded-sm bg-background px-1.5 font-mono text-label">
                      <span aria-hidden className={cn("size-2 rounded-xs", tone ? TAG[tone] : "ring-1 ring-foreground/40 ring-inset")} />
                      {s}
                    </span>
                  );
                })}
              </p>
            </li>
          ))}
        </ol>
      )}
    </Section>
  );
}
