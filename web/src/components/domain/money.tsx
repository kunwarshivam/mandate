"use client";

import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import NumberFlow from "@number-flow/react";
import { cn } from "@/lib/utils";
import type { Dec } from "@/lib/decimal";
import { direction, directionWord, signedUsd, usd } from "@/lib/format";

const USD = /^([+\u2212-]?)\$(\d{1,3}(?:,\d{3})*)(?:\.(\d+))?$/;

/** A formatted dollar figure ("+$1,234.50", "−$5.55") as Number Flow's parts; anything else is null. */
export function parseUsd(value: string): { sign: string; amount: number; places: number } | null {
  const m = USD.exec(value);
  if (!m) return null;
  const places = m[3]?.length ?? 0;
  return { sign: m[1] === "-" ? "\u2212" : m[1], amount: Number(`${m[2].replaceAll(",", "")}.${m[3] ?? "0"}`), places };
}

/** Digits roll on the spring the rest of the app settles on; Number Flow's soft edge mask is set to nothing, so edges stay flat. */
const ROLL = { duration: 520, easing: "cubic-bezier(0.23, 1, 0.32, 1)" };
const FADE = { duration: 260, easing: "ease-out" };

/**
 * Number Flow's server render leaves a copy of the value in the element's light DOM, which the
 * client never updates once the shadow DOM draws it, so the copy would go stale in `textContent`.
 */
function dropServerCopy(el: HTMLElement | null) {
  if (el?.shadowRoot) el.replaceChildren();
}

/**
 * A changed figure rolls digit by digit (Number Flow, DEC-216): only the digits that change move,
 * upward when the value rises and downward when it falls, so the eye lands on what changed.
 * Assistive technology reads the whole value once from a plain copy. `instant` is for a value that
 * follows the owner's finger (a scrubbed chart), which must never lag behind it. With reduced motion,
 * the figure changes in place.
 */
export function AnimatedValue({
  value,
  className,
  instant = false,
  cents = false,
}: {
  value: string;
  className?: string;
  instant?: boolean;
  /** Draws the cents half size, muted, and raised to the digits' cap height. */
  cents?: boolean;
}) {
  const shift = useReducedMotion() ? 0 : 40;
  const parsed = parseUsd(value);
  if (parsed) {
    return (
      <span data-slot="figure" data-instant={instant ? "" : undefined} className={cn("relative inline-grid", className)}>
        <span className="sr-only">{value}</span>
        <NumberFlow
          ref={dropServerCopy}
          aria-hidden="true"
          value={parsed.amount}
          prefix={parsed.sign}
          locales="en-US"
          format={{ style: "currency", currency: "USD", minimumFractionDigits: parsed.places, maximumFractionDigits: parsed.places }}
          animated={!instant}
          transformTiming={ROLL}
          spinTiming={ROLL}
          opacityTiming={FADE}
          className={cn("number-flow [grid-area:1/1]", cents && "number-flow-cents")}
        />
      </span>
    );
  }
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

/**
 * A screen's one hero figure (account or agent equity) in the display size, with its cents half
 * size. It rolls like any figure, or follows a scrub at once with `instant`.
 */
export function HeroFigure({ value, instant = false }: { value: string; instant?: boolean }) {
  return <AnimatedValue value={value} instant={instant} cents />;
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
