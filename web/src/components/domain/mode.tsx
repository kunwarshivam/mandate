"use client";

import { motion } from "motion/react";
import { openStop } from "@/components/shell/open-stop";
import { cn } from "@/lib/utils";
import type { ActiveRestriction, AgentMode } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import { useCan } from "@/lib/roles";
import { type RestrictionSource, SOURCE_LABEL, describeRestriction } from "@/lib/restrictions";

/**
 * Trading is quiet; selling only is outlined in ink (half stopped); paused and stopped are solid
 * ink. The word carries the mode and a dot marks it as a state (DEC-512): never a glyph that reads
 * as a control, so no colour has to carry it either.
 */
export const MODE_FIELD: Record<AgentMode, string> = {
  normal: "bg-background text-muted-foreground",
  exits_only: "bg-card text-foreground ring-1 ring-ink ring-inset",
  paused: "bg-ink text-ink-foreground",
  stopped: "bg-ink text-ink-foreground",
};

const EASE_OUT = [0.23, 1, 0.32, 1] as const;

export function ModeBadge({ mode, className }: { mode: AgentMode; className?: string }) {
  return (
    <motion.span
      layout
      transition={{ duration: 0.2, ease: EASE_OUT }}
      data-slot="mode-badge"
      data-mode={mode}
      className={cn(
        "inline-flex h-6 w-fit shrink-0 items-center gap-1.5 rounded-md px-2 text-label whitespace-nowrap transition-colors duration-(--duration-hover)",
        MODE_FIELD[mode],
        className,
      )}
    >
      <span aria-hidden data-slot="mode-dot" className={cn("size-1.5 shrink-0 rounded-full", mode === "exits_only" ? "ring-1 ring-current ring-inset" : "bg-current")} />
      {MODE_LABEL[mode]}
    </motion.span>
  );
}

/** A restriction wears the colour of whoever imposed it, and names them. */
export const SOURCE_FIELD: Record<RestrictionSource, string> = {
  mandate: "bg-mandate-soft",
  account: "bg-lapis-soft",
  owner: "bg-background",
  market: "bg-background",
};

export const SOURCE_TAG: Record<RestrictionSource, string> = {
  mandate: "bg-mandate text-mandate-strong ring-1 ring-mandate-edge ring-inset",
  account: "bg-lapis-soft text-lapis",
  owner: "bg-ink text-ink-foreground",
  market: "bg-card text-foreground ring-1 ring-border ring-inset",
};

export function SourceTag({ source, className }: { source: RestrictionSource; className?: string }) {
  return (
    <span data-source={source} className={cn("inline-flex h-6 w-fit shrink-0 items-center rounded-md px-2.5 text-label", SOURCE_TAG[source], className)}>
      {SOURCE_LABEL[source]}
    </span>
  );
}

/**
 * The §4.3 mode banner: every active restriction with what it blocks, how it ends, and who acts.
 * Where the owner ends one in Stop, the banner opens Stop; Stop keeps its passkey (C-24). A role
 * that cannot resume or release there is not offered the door.
 * Opening actions are explained here rather than hidden. One calm panel; restrictions sit on
 * hairlines inside it rather than as boxes within a box.
 */
export function ModeBanner({
  mode,
  restrictions,
  showMode = true,
  className,
}: {
  mode: AgentMode;
  restrictions: ActiveRestriction[];
  /** Off where the mode is already stated right above, so it is not said twice. */
  showMode?: boolean;
  className?: string;
}) {
  const canEnd = useCan("stop.full");
  if (mode === "normal" && restrictions.length === 0) return null;
  if (!showMode && restrictions.length === 0) return null;
  return (
    <section aria-label="Restrictions" data-slot="mode-banner" data-mode={mode} className={cn("reveal grid gap-4 rounded-2xl bg-background px-5 py-4 text-foreground sm:px-6", className)}>
      {showMode ? (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <ModeBadge mode={mode} />
          <p className="font-medium">{MODE_MEANING[mode]}</p>
        </div>
      ) : null}
      {restrictions.length > 0 ? (
        <ul className="grid divide-y divide-border">
          {restrictions.map((r) => {
            const text = describeRestriction(r);
            return (
              <li key={`${r.code}-${r.symbol ?? ""}`} data-source={text.source} className="grid gap-2 py-3 first:pt-0 last:pb-0">
                <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <SourceTag source={text.source} />
                  <span className="font-semibold">{text.title}</span>
                  <span className="text-sm text-muted-foreground">since {clock(r.since)}</span>
                </p>
                <dl className="grid gap-x-5 gap-y-1 text-sm sm:grid-cols-[6rem_1fr]">
                  <dt className="text-muted-foreground">Blocks</dt>
                  <dd>{text.blocks}</dd>
                  <dt className="text-muted-foreground">Ends when</dt>
                  <dd>{text.endsWhen}</dd>
                  <dt className="text-muted-foreground">Who acts</dt>
                  <dd>
                    {text.whoActs}
                    {text.endsInStop && canEnd ? (
                      <>
                        {" "}
                        <button
                          type="button"
                          onClick={openStop}
                          aria-haspopup="dialog"
                          className="font-medium text-lapis underline decoration-lapis/30 underline-offset-4 outline-none hover:decoration-current focus-visible:ring-3 focus-visible:ring-ring"
                        >
                          Open Stop
                        </button>
                      </>
                    ) : null}
                  </dd>
                </dl>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
