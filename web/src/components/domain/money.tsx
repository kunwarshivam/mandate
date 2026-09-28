"use client";

import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";
import type { Dec } from "@/lib/decimal";
import { direction, directionWord, signedUsd, usd } from "@/lib/format";

/**
 * Money is right-aligned tabular mono. A changed value swaps whole, with a short cross-blur;
 * nothing counts up through intermediate figures that were never true.
 */
export function AnimatedValue({ value, className }: { value: string; className?: string }) {
  return (
    <span className={cn("relative inline-grid", className)}>
      <AnimatePresence initial={false} mode="popLayout">
        <motion.span
          key={value}
          initial={{ opacity: 0, filter: "blur(2px)", y: 2 }}
          animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
          exit={{ opacity: 0, filter: "blur(2px)", y: -2 }}
          transition={{ duration: 0.2, ease: [0.25, 1, 0.5, 1] }}
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
export function SignedMoney({ value, className, showWord = true }: { value: string | Dec; className?: string; showWord?: boolean }) {
  const d = direction(value);
  return (
    <span
      data-direction={d}
      className={cn(
        "inline-flex items-baseline gap-1.5 font-mono tabular",
        d === "gain" && "text-lagoon-text",
        d === "loss" && "text-rose-text",
        className,
      )}
    >
      <AnimatedValue value={signedUsd(value)} />
      {showWord ? <span className="font-sans text-caption font-medium">{directionWord(value)}</span> : <span className="sr-only">{directionWord(value)}</span>}
    </span>
  );
}
