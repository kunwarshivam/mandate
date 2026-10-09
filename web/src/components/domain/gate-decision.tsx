import Link from "next/link";
import { cn } from "@/lib/utils";
import type { Agent, GateDecision } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { actionSentence, decidedRule, verdictBadge } from "@/lib/gate-reasons";
import { PURPOSE_LABEL } from "@/lib/labels";
import { STRETCHED_LINK } from "./positions";

/** The one width every verdict chip takes, so "Not allowed" and "Allowed" line up down a list. */
export const VERDICT_COLUMN = "[--verdict-w:5.75rem]";

/**
 * The gate's verdict as one chip in one style everywhere (DEC-512): a quiet chip on the well for an
 * allow that went out, and an ink ring for everything that did not (asked you, not allowed, held,
 * waiting). It takes the verdict column's width and never wraps; it carries no meaning colour. The
 * quiet chip keeps a hairline in its own secondary ink, inset so the width holds: in dark mode the
 * well sits a step from the card, and without the edge the chip vanished (critique C-18).
 */
export function VerdictChip({ decision, className }: { decision: GateDecision; className?: string }) {
  const sent = decision.verdict === "allow" && !decision.approval_id;
  return (
    <span
      data-slot="verdict"
      className={cn(
        "inline-flex h-6 w-(--verdict-w) shrink-0 items-center justify-center rounded-md px-1 text-label whitespace-nowrap",
        sent ? "bg-background text-muted-foreground ring-1 ring-muted-foreground ring-inset" : "bg-card text-foreground ring-1 ring-foreground ring-inset",
        className,
      )}
    >
      {verdictBadge(decision)}
    </span>
  );
}

/**
 * A gate decision in plain language: the verdict first, in its column, then the action and the
 * rule as the decision's own mandate version states it, never an error code.
 */
export function GateDecisionRow({
  decision,
  agent,
  showAgent = false,
  href,
  className,
}: {
  decision: GateDecision;
  agent: Agent | undefined;
  showAgent?: boolean;
  href?: string;
  className?: string;
}) {
  const rule = agent ? decidedRule(decision, agent) : null;
  return (
    <li
      data-verdict={decision.verdict}
      className={cn(
        "grid grid-cols-[3.25rem_var(--verdict-w)_minmax(0,1fr)] gap-3 border-b border-border/70 py-(--row-y) last:border-b-0",
        VERDICT_COLUMN,
        href && "group relative transition-colors duration-(--duration-hover) hover:bg-background",
        className,
      )}
    >
      <time dateTime={decision.at} className="pt-0.5 font-mono text-caption text-muted-foreground tabular">
        {clock(decision.at).slice(0, 5)}
      </time>
      <VerdictChip decision={decision} />
      <div className="grid gap-1">
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
          {href ? (
            <Link href={href} className={cn("font-medium underline-offset-4 group-hover:underline", STRETCHED_LINK)}>
              {actionSentence(decision.action)}
            </Link>
          ) : (
            <span className="font-medium">{actionSentence(decision.action)}</span>
          )}
          <span className="text-caption text-muted-foreground">
            {PURPOSE_LABEL[decision.action.purpose]}
            {showAgent && agent ? `, ${agent.label}` : ""}
          </span>
        </p>
        {rule ? (
          <p className="text-sm text-muted-foreground" data-slot="gate-rule">
            {rule}
          </p>
        ) : null}
        {decision.then ? <p className="text-sm text-muted-foreground">{decision.then}</p> : null}
      </div>
    </li>
  );
}
