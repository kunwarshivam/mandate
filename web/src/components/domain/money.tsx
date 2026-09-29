"use client";

import type { ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { cn } from "@/lib/utils";
import type { Dec } from "@/lib/decimal";
import { direction, directionWord, signedUsd, usd } from "@/lib/format";

/**
 * A changed value rolls whole: the old figure leaves upward as the new one arrives, 240 ms. Nothing
 * counts up through intermediate figures that were never true. Assistive technology reads the
 * value once, not the two copies that overlap while it changes. `instant` is for a value that
 * follows the owner's finger (a scrubbed chart), which must never lag behind it.
 */
export function AnimatedValue({
  value,
  className,
  instant = false,
  format,
}: {
  value: string;
  className?: string;
  instant?: boolean;
  /** Draws the visible copy in parts; the screen-reader copy stays the whole value. */
  format?: (value: string) => ReactNode;
}) {
  // MotionConfig's reducedMotion covers named transform keys only, not a raw `transform` string.
  const shift = useReducedMotion() ? 0 : 40;
  const shown = format ? format(value) : value;
  if (instant) {
    // Some screen readers read each inline element on its own, so a value drawn in parts gets a whole copy.
    return format ? (
      <span data-instant="" className={cn("relative inline-grid", className)}>
        <span className="sr-only">{value}</span>
        <span aria-hidden className="[grid-area:1/1]">
          {shown}
        </span>
      </span>
    ) : (
      <span data-instant="" className={cn("inline-grid", className)}>
        {value}
      </span>
    );
  }
  return (
    <span className={cn("relative inline-grid overflow-hidden", className)}>
      <span className="sr-only">{value}</span>
      <AnimatePresence initial={false} mode="popLayout">
        <motion.span
          key={value}
          aria-hidden
          initial={{ opacity: 0, transform: `translateY(${shift}%)` }}
          animate={{ opacity: 1, transform: "translateY(0%)" }}
          exit={{ opacity: 0, transform: `translateY(${-shift}%)` }}
          transition={{ duration: 0.24, ease: [0.23, 1, 0.32, 1] }}
          className="[grid-area:1/1]"
        >
          {shown}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

/** "$28,478" and ".36": the cents are drawn smaller, muted, and raised to the digits' cap height. */
function withCents(value: string): ReactNode {
  const dot = value.lastIndexOf(".");
  if (dot < 0) return value;
  return (
    <>
      {value.slice(0, dot)}
      <span data-slot="cents" className="align-[1cap] text-[0.5em] leading-0 tracking-normal text-muted-foreground">
        {value.slice(dot)}
      </span>
    </>
  );
}

/**
 * A screen's one hero figure (account or agent equity) in the display size, with its cents half
 * size. It rolls like any figure, or follows a scrub at once with `instant`.
 */
export function HeroFigure({ value, instant = false }: { value: string; instant?: boolean }) {
  return <AnimatedValue value={value} instant={instant} format={withCents} />;
}

export function Money({ value, places, className }: { value: string | Dec; places?: number; className?: string }) {
  return <AnimatedValue value={usd(value, places)} className={cn("font-mono tabular", className)} />;
}

/** A gain or loss: sign, colour, and the word, so colour never carries the meaning alone. */
export function SignedMoney({
  value,
  className,
  showWord = true,
  instant = false,
}: {
  value: string | Dec;
  className?: string;
  showWord?: boolean;
  instant?: boolean;
}) {
  const d = direction(value);
  return (
    <span
      data-direction={d}
      className={cn("inline-flex items-baseline gap-1.5 font-mono tabular", d === "gain" && "text-gain", d === "loss" && "text-loss", className)}
    >
      <AnimatedValue value={signedUsd(value)} instant={instant} />
      {showWord ? (
        <span className="font-sans text-[max(0.85em,0.75rem)] font-medium">{directionWord(value)}</span>
      ) : (
        <span className="sr-only">{directionWord(value)}</span>
      )}
    </span>
  );
}
