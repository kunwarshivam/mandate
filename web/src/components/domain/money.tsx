"use client";

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
export function AnimatedValue({ value, className, instant = false }: { value: string; className?: string; instant?: boolean }) {
  // MotionConfig's reducedMotion covers named transform keys only, not a raw `transform` string.
  const shift = useReducedMotion() ? 0 : 40;
  if (instant) {
    return (
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
          {value}
        </motion.span>
      </AnimatePresence>
    </span>
  );
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
