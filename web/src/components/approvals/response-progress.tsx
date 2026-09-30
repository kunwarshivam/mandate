import { CheckCircle, Circle, CircleDashed } from "@phosphor-icons/react";
import type { Approval } from "@/fixtures/types";
import { APPROVAL_STATUS_LABEL } from "@/lib/labels";
import type { ApprovalResponse } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";

export type StepState = "done" | "current" | "waiting";

export interface Step {
  key: "sent" | "recorded" | "result";
  label: string;
  state: StepState;
}

/**
 * Where a response stands, in three steps: sent, recorded in the journal, and the result. A step is
 * done only once the runtime says so, so nothing reads as finished before the journal records it.
 */
export function responseSteps(approval: Approval, response: ApprovalResponse): Step[] {
  const resolved = approval.status !== "delivered";
  const recorded = resolved || response.phase === "recorded" || response.phase === "decided";
  const waitingOn = approval.approvers_required - approval.approvals_so_far.length;
  return [
    { key: "sent", label: "Sent", state: "done" },
    {
      key: "recorded",
      label: response.phase === "unknown" && !recorded ? "Checking the journal" : "Recorded in the journal",
      state: recorded ? "done" : "current",
    },
    {
      key: "result",
      label: resolved ? APPROVAL_STATUS_LABEL[approval.status] : waitingOn > 0 && recorded ? "Waiting for approvers" : "Result",
      state: resolved ? "done" : recorded ? "current" : "waiting",
    },
  ];
}

const GLYPH = { done: CheckCircle, current: CircleDashed, waiting: Circle } as const;
const STATE_WORD: Record<StepState, string> = { done: "done", current: "in progress", waiting: "not yet" };

export function ResponseProgress({ steps, className }: { steps: Step[]; className?: string }) {
  return (
    <ol aria-label="Progress" data-slot="response-progress" className={cn("grid grid-cols-3 gap-2 text-caption", className)}>
      {steps.map((s) => {
        const Glyph = GLYPH[s.state];
        return (
          <li
            key={s.key}
            data-state={s.state}
            className={cn(
              "flex min-w-0 items-start gap-1.5 border-t-2 pt-2",
              s.state === "waiting" ? "border-border text-muted-foreground" : "border-foreground text-foreground",
              s.state === "current" && "font-medium",
            )}
          >
            <Glyph className="mt-px size-4 shrink-0" weight={s.state === "done" ? "fill" : "regular"} aria-hidden />
            <span className="min-w-0 text-pretty">
              {s.label}
              <span className="sr-only">, {STATE_WORD[s.state]}</span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
