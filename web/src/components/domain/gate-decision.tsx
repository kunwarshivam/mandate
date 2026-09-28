import Link from "next/link";
import { cn } from "@/lib/utils";
import type { Agent, GateDecision } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { PURPOSE_LABEL } from "@/lib/labels";
import { STRETCHED_LINK } from "./positions";

/**
 * A gate decision in plain language: the action, the verdict, and the rule, never an error code.
 * The verdict carries no meaning colour: an allowed action is a quiet chip, a held one is outlined.
 */
export function GateDecisionRow({ decision, agent, showAgent = false, href }: { decision: GateDecision; agent: Agent | undefined; showAgent?: boolean; href?: string }) {
  const label = verdictLabel(decision);
  const rule = decision.reason_code && agent ? gateRule(decision.reason_code, agent.mandate) : null;
  const allowed = decision.verdict === "allow";
  return (
    <li data-verdict={decision.verdict} className={cn("grid grid-cols-[3.25rem_minmax(0,1fr)] gap-3 border-b py-2.5 last:border-b-0", href && "relative hover:bg-muted")}>
      <time dateTime={decision.at} className="pt-0.5 font-mono text-caption text-muted-foreground tabular">
        {clock(decision.at).slice(0, 5)}
      </time>
      <div className="grid gap-1">
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            data-slot="verdict"
            className={cn("inline-flex h-6 items-center px-1.5 field-label", allowed ? "bg-muted text-foreground" : "bg-card text-foreground ring-2 ring-foreground ring-inset")}
          >
            {label}
          </span>
          {href ? (
            <Link href={href} className={cn("font-semibold underline decoration-2 underline-offset-4", STRETCHED_LINK)}>
              {actionSentence(decision.action)}
            </Link>
          ) : (
            <span className="font-semibold">{actionSentence(decision.action)}</span>
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
