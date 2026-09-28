"use client";

import { motion } from "motion/react";
import { cn } from "@/lib/utils";
import { CheckCircle, PauseCircle, Prohibit, SignOut } from "@phosphor-icons/react";
import type { ActiveRestriction, AgentMode } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import { type RestrictionSource, SOURCE_LABEL, describeRestriction } from "@/lib/restrictions";

const MODE_ICON = { normal: CheckCircle, exits_only: SignOut, paused: PauseCircle, stopped: Prohibit } as const;

/**
 * A mode is a sign read from across the room: running is quiet, exits only is outlined in ink
 * (half stopped), paused and stopped are solid ink.
 */
export const MODE_FIELD: Record<AgentMode, string> = {
  normal: "bg-muted text-foreground",
  exits_only: "bg-card text-foreground ring-2 ring-ink ring-inset",
  paused: "bg-ink text-ink-foreground",
  stopped: "bg-ink text-ink-foreground",
};

const EASE_OUT = [0.23, 1, 0.32, 1] as const;

export function ModeBadge({ mode, className }: { mode: AgentMode; className?: string }) {
  const Icon = MODE_ICON[mode];
  return (
    <motion.span
      layout
      transition={{ duration: 0.2, ease: EASE_OUT }}
      data-slot="mode-badge"
      data-mode={mode}
      className={cn(
        "inline-flex h-7 w-fit shrink-0 items-center gap-1.5 px-2 whitespace-nowrap label-caps transition-colors duration-(--duration-hover) [&>svg]:size-3.5",
        MODE_FIELD[mode],
        className,
      )}
    >
      <Icon aria-hidden />
      {MODE_LABEL[mode]}
    </motion.span>
  );
}

/** A restriction wears the colour of whoever imposed it. */
export const SOURCE_FIELD: Record<RestrictionSource, string> = {
  mandate: "bg-mandate-soft",
  account: "bg-lapis-soft",
  owner: "bg-muted",
  market: "bg-muted",
};

export const SOURCE_TAG: Record<RestrictionSource, string> = {
  mandate: "bg-mandate text-mandate-strong ring-1 ring-mandate-edge ring-inset",
  account: "bg-lapis text-lapis-foreground",
  owner: "bg-ink text-ink-foreground",
  market: "bg-card text-foreground ring-1 ring-foreground ring-inset",
};

export function SourceTag({ source, className }: { source: RestrictionSource; className?: string }) {
  return (
    <span data-source={source} className={cn("inline-flex h-6 w-fit shrink-0 items-center px-1.5 label-caps", SOURCE_TAG[source], className)}>
      {SOURCE_LABEL[source]}
    </span>
  );
}

/**
 * The §4.3 mode banner: every active restriction with what it blocks, how it ends, and who acts.
 * Opening actions are explained here rather than hidden. Fields sit on seams, never nested.
 */
export function ModeBanner({
  mode,
  restrictions,
  showMode = true,
  className,
}: {
  mode: AgentMode;
  restrictions: ActiveRestriction[];
  /** Off where the mode already stands as its own field right above, so it is not said twice. */
  showMode?: boolean;
  className?: string;
}) {
  if (mode === "normal" && restrictions.length === 0) return null;
  if (!showMode && restrictions.length === 0) return null;
  return (
    <section aria-label="Restrictions" data-slot="mode-banner" className={cn("reveal grid gap-(--seam) text-foreground", className)}>
      {showMode ? (
        <div data-mode={mode} className={cn("flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-3 transition-colors duration-(--duration-hover)", MODE_FIELD[mode])}>
          <p className="font-display text-heading uppercase">{MODE_LABEL[mode]}</p>
          <p className="font-medium">{MODE_MEANING[mode]}</p>
        </div>
      ) : null}
      {restrictions.length > 0 ? (
        <ul className="grid gap-(--seam)">
          {restrictions.map((r) => {
            const text = describeRestriction(r);
            return (
              <li key={`${r.code}-${r.symbol ?? ""}`} data-source={text.source} className={cn("grid gap-2 px-4 py-3", SOURCE_FIELD[text.source])}>
                <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <SourceTag source={text.source} />
                  <span className="font-bold">{text.title}</span>
                  <span className="text-muted-foreground">since {clock(r.since)}</span>
                </p>
                <dl className="grid gap-x-4 gap-y-0.5 text-sm sm:grid-cols-[auto_1fr]">
                  <dt className="text-muted-foreground">Blocks</dt>
                  <dd>{text.blocks}</dd>
                  <dt className="text-muted-foreground">Ends when</dt>
                  <dd>{text.endsWhen}</dd>
                  <dt className="text-muted-foreground">Who acts</dt>
                  <dd>{text.whoActs}</dd>
                </dl>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
