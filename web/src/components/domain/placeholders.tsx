"use client";

import { type ReactNode, createContext, useContext, useId } from "react";
import { Popover } from "@cloudflare/kumo/components/popover";
import { Info } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";

/**
 * Compliance text appears only as a named placeholder until counsel drafts it (brief §5, rule 9;
 * DEC-79). These components are the only place the placeholder names are written.
 */
const NAMES = {
  performance: "[[DISCLOSURE-PERFORMANCE]]",
  hypothetical: "[[LEGEND-HYPOTHETICAL]]",
  retailAutoLive: "[[RETAIL-AUTO-LIVE]]",
} as const;

/**
 * DEC-210: the performance disclosure sits behind an info symbol beside each P&L, one hover or tap
 * away. Counsel must confirm that meets the "clear and prominent" standard before launch; if they
 * require it visible, set this to true and every P&L shows the full text inline again.
 */
const PERFORMANCE_INLINE = false;

const TAG = "inline-block rounded-sm border border-dashed border-muted-foreground px-1.5 py-0.5 font-mono text-label font-normal text-muted-foreground";

const InlineContext = createContext(false);

/**
 * Disclosures inside show their full text, never a symbol: for what cannot be hovered or opened, such
 * as a frozen record screen, where collapsed content counts as not shown (brief §4.1).
 */
export function InlineDisclosures({ inline = true, children }: { inline?: boolean; children: ReactNode }) {
  return <InlineContext value={inline}>{children}</InlineContext>;
}

/**
 * The symbol takes one line of the text beside it and no more, so it moves nothing: a zero-width
 * space gives it the text's line height and baseline, and the 24px target (44px on touch) is a
 * pseudo-element around the 16px glyph. The text it opens is also its description, kept in the
 * same row so a screen reader hears it without opening, and printed in full where nothing opens.
 */
function PerformanceDisclosure({ className }: { className?: string }) {
  const id = useId();
  return (
    <span data-slot="disclosure" className={cn("inline-flex", className)}>
      <Popover>
        <Popover.Trigger
          openOnHover
          delay={150}
          aria-label="Performance disclosure"
          aria-describedby={id}
          data-slot="disclosure-trigger"
          className="relative inline-block w-4 shrink-0 cursor-pointer text-muted-foreground outline-none transition-colors duration-(--duration-hover) ease-(--ease-out) before:absolute before:top-1/2 before:left-1/2 before:size-6 before:-translate-1/2 before:rounded-full before:content-[''] hover:text-foreground focus-visible:before:ring-2 focus-visible:before:ring-ring data-popup-open:text-foreground pointer-coarse:before:size-11 print:hidden"
        >
          <span aria-hidden>{"\u200b"}</span>
          <Info aria-hidden weight="regular" className="pointer-events-none absolute top-1/2 left-0 size-4 -translate-y-1/2" />
        </Popover.Trigger>
        <Popover.Content sideOffset={10} className="max-w-[min(20rem,calc(100vw-2rem))]">
          <Popover.Title className="sr-only">Performance disclosure</Popover.Title>
          <p data-placeholder="performance" className="font-mono text-caption text-muted-foreground">
            {NAMES.performance}
          </p>
        </Popover.Content>
      </Popover>
      <span id={id} data-placeholder="performance" className={cn(TAG, "not-print:sr-only")}>
        {NAMES.performance}
      </span>
    </span>
  );
}

export function Placeholder({ name, className }: { name: keyof typeof NAMES; className?: string }) {
  const inline = useContext(InlineContext);
  if (name === "performance" && !PERFORMANCE_INLINE && !inline) return <PerformanceDisclosure className={className} />;
  return (
    <span data-placeholder={name} className={cn(TAG, className)}>
      {NAMES[name]}
    </span>
  );
}

/** Marks fixture values so no one reads them as a record. */
export function FixtureTag({ className }: { className?: string }) {
  return (
    <span data-slot="fixture-tag" className={cn("inline-flex h-5 shrink-0 items-center rounded-sm border border-dashed border-muted-foreground px-1.5 text-label text-muted-foreground", className)}>
      Fixture data
    </span>
  );
}
