import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import type { Agent, GateDecision } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { PURPOSE_LABEL } from "@/lib/labels";

/** A gate decision in plain language: the action, the verdict, and the rule, never an error code. */
export function GateDecisionRow({ decision, agent, showAgent = false }: { decision: GateDecision; agent: Agent | undefined; showAgent?: boolean }) {
  const label = verdictLabel(decision);
  const rule = decision.reason_code && agent ? gateRule(decision.reason_code, agent.mandate) : null;
  return (
    <li data-verdict={decision.verdict} className="grid grid-cols-[3.75rem_1fr] gap-3 py-3">
      <time dateTime={decision.at} className="pt-0.5 font-mono text-caption tabular text-muted-foreground">
        {clock(decision.at).slice(0, 5)}
      </time>
      <div className="grid gap-1">
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <Badge
            variant="secondary"
            className={cn("text-[0.6875rem]", decision.verdict === "allow" ? "bg-muted text-foreground" : "bg-notice text-foreground ring-1 ring-notice-border")}
          >
            {label}
          </Badge>
          <span className="font-medium">{actionSentence(decision.action)}</span>
          <span className="text-caption text-muted-foreground">
            {PURPOSE_LABEL[decision.action.purpose]}
            {showAgent && agent ? `, ${agent.label}` : ""}
          </span>
        </p>
        {rule ? <p className="text-sm text-muted-foreground" data-slot="gate-rule">{rule}</p> : null}
        {decision.then ? <p className="text-sm text-muted-foreground">{decision.then}</p> : null}
      </div>
    </li>
  );
}
