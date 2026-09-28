import type { ReactNode } from "react";
import { LOCKUP_GAP, OwlheadLockup, OwlheadMark, OwlheadWordmark } from "./Logo";
import { BRAND_PALETTE, NAVY, OFF_WHITE, hexContrast } from "@/lib/brand-palette";

const DO = [
  "Navy on a light surface, always: one colour at a time.",
  "The mark alone where the lockup does not fit: a phone header, a collapsed sidebar, a favicon.",
  "Clear space of a quarter of the mark's height on every side, the same as the gap inside the lockup.",
  "The committed SVGs or the Logo components, scaled as a whole.",
];

const DONT = [
  "Reverse it: no off-white mark on a navy block or any other dark fill, in the UI, the app icons, or the share image.",
  "Recolour it: no brass mark, no second colour, no colour fade, shadow, or outline.",
  "Stretch, rotate, crop, or redraw it, or rebuild the wordmark in live type or another face.",
  "Use the founder's shaded original in product UI: the UI is flat colour (DEC-200).",
  "Go below 16 px for the mark or 96 px wide for the lockup, or set it on a busy field.",
];

function Tile({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <figure className={`grid content-between gap-3 rounded-2xl bg-background p-5 ${className ?? ""}`}>
      <div className="flex min-h-20 items-center">{children}</div>
      <figcaption className="text-label font-medium text-muted-foreground">{label}</figcaption>
    </figure>
  );
}

/** The Brand block of `/design`: the mark, the wordmark, and the lockup, and how to use them (DEC-202, DEC-203). */
export function BrandSpecimen() {
  return (
    <div className="grid gap-(--block-gap)">
      <div data-slot="brand-surface" className="grid gap-6 rounded-2xl p-6 ring-1 ring-border" style={{ background: OFF_WHITE, color: NAVY }}>
        <span className="text-label font-medium">On off-white</span>
        <div className="flex flex-wrap items-end gap-8">
          <OwlheadMark className="h-16 w-auto" />
          <OwlheadWordmark className="h-10 w-auto" />
          <OwlheadLockup className="h-12 w-auto max-w-full" />
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Tile label="Clear space: a quarter of the mark's height" className="sm:col-span-2">
          <div className="rounded-lg border border-dashed border-muted-foreground" style={{ padding: `${48 * LOCKUP_GAP}px`, color: NAVY }}>
            <OwlheadLockup className="block h-12 w-auto" title="" />
          </div>
        </Tile>
        <Tile label="Minimum: mark 16 px">
          <span style={{ color: NAVY }}>
            <OwlheadMark className="h-4 w-auto" />
          </span>
        </Tile>
        <Tile label="Minimum: lockup 96 px wide">
          <span style={{ color: NAVY }}>
            <OwlheadLockup className="block h-auto w-24" />
          </span>
        </Tile>
      </div>

      <div className="grid gap-3 lg:grid-cols-2">
        <div className="grid content-start gap-2 rounded-2xl bg-background p-5">
          <h3 className="text-h3">Do</h3>
          <ul className="grid gap-1.5 text-sm">
            {DO.map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
        </div>
        <div className="grid content-start gap-2 rounded-2xl bg-background p-5">
          <h3 className="text-h3">Don&apos;t</h3>
          <ul className="grid gap-1.5 text-sm">
            {DONT.map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
        </div>
      </div>

      <ul className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3" aria-label="Navy and brass palette">
        {BRAND_PALETTE.map((c) => (
          <li key={c.name} data-slot="brand-color" className="grid grid-cols-[3rem_1fr] gap-3 rounded-xl bg-background p-2.5">
            <span className="size-12 rounded-lg ring-1 ring-border" style={{ background: c.hex }} aria-hidden />
            <span className="grid min-w-0 content-center gap-0.5">
              <span className="text-caption font-semibold">
                {c.name} <span className="font-mono">{c.hex}</span>
              </span>
              <span className="text-label font-normal text-muted-foreground">{c.role}</span>
              <span className="font-mono text-label font-normal text-muted-foreground tabular">
                {hexContrast(c.hex, OFF_WHITE).toFixed(2)}:1 on off-white · {hexContrast(c.hex, NAVY).toFixed(2)}:1 on navy
              </span>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
