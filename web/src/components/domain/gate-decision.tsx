import { cn } from "@/lib/utils";
import type { Agent, GateDecision } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { PURPOSE_LABEL } from "@/lib/labels";

/**
 * A gate decision in plain language: the action, the verdict, and the rule, never an error code.
 * The verdict carries no meaning colour: an allowed action is a quiet chip, a held one is outlined.
 */
export function GateDecisionRow({ decision, agent, showAgent = false }: { decision: GateDecision; agent: Agent | undefined; showAgent?: boolean }) {
  const label = verdictLabel(decision);
  const rule = decision.reason_code && agent ? gateRule(decision.reason_code, agent.mandate) : null;
  const allowed = decision.verdict === "allow";
  return (
    <li data-verdict={decision.verdict} className="grid grid-cols-[3.25rem_minmax(0,1fr)] gap-3 border-b py-2.5 last:border-b-0">
      <time dateTime={decision.at} className="pt-0.5 font-mono text-caption text-muted-foreground tabular">
        {clock(decision.at).slice(0, 5)}
      </time>
      <div className="grid gap-1">
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            data-slot="verdict"
            className={cn("inline-flex h-6 items-center px-1.5 label-caps", allowed ? "bg-muted text-foreground" : "bg-card text-foreground ring-2 ring-foreground ring-inset")}
          >
            {label}
          </span>
          <span className="font-bold">{actionSentence(decision.action)}</span>
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
