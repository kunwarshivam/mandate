"use client";

import { motion } from "motion/react";
import { cn } from "@/lib/utils";
import { CircleCheck, CirclePause, CircleSlash, LogOut } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { ActiveRestriction, AgentMode } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { MODE_LABEL, MODE_MEANING } from "@/lib/labels";
import { describeRestriction } from "@/lib/restrictions";

const MODE_ICON = { normal: CircleCheck, exits_only: LogOut, paused: CirclePause, stopped: CircleSlash } as const;

const MODE_STYLE: Record<AgentMode, string> = {
  normal: "bg-muted text-foreground",
  exits_only: "bg-notice text-foreground ring-1 ring-notice-border",
  paused: "bg-notice text-foreground ring-1 ring-notice-border",
  stopped: "bg-foreground text-background",
};

export function ModeBadge({ mode, className }: { mode: AgentMode; className?: string }) {
  const Icon = MODE_ICON[mode];
  return (
    <Badge asChild className={cn("h-6 gap-1.5 px-2.5 text-caption", MODE_STYLE[mode], className)}>
      <motion.span layout transition={{ duration: 0.24, ease: [0.25, 1, 0.5, 1] }} data-mode={mode}>
        <Icon aria-hidden />
        {MODE_LABEL[mode]}
      </motion.span>
    </Badge>
  );
}

/**
 * The §4.3 mode banner: every active restriction with what it blocks, how it ends, and who acts.
 * Opening actions are explained here rather than hidden.
 */
export function ModeBanner({ mode, restrictions, className }: { mode: AgentMode; restrictions: ActiveRestriction[]; className?: string }) {
  if (mode === "normal" && restrictions.length === 0) return null;
  return (
    <motion.section
      layout
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.24, ease: [0.25, 1, 0.5, 1] }}
      aria-label="Restrictions"
      data-slot="mode-banner"
      className={cn("rounded-lg border border-notice-border bg-notice p-4 text-foreground", className)}
    >
      <p className="flex flex-wrap items-center gap-2 font-medium">
        <ModeBadge mode={mode} />
        <span>{MODE_MEANING[mode]}</span>
      </p>
      {restrictions.length > 0 ? (
        <ul className="mt-3 grid gap-3">
          {restrictions.map((r) => {
            const text = describeRestriction(r);
            return (
              <li key={`${r.code}-${r.symbol ?? ""}`} className="grid gap-1 border-t border-notice-border/70 pt-3 first:border-t-0 first:pt-0">
                <p className="font-medium">
                  {text.title} <span className="font-normal text-muted-foreground">since {clock(r.since)}</span>
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
    </motion.section>
  );
}
