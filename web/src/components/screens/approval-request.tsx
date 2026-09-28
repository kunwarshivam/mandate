"use client";

import Link from "next/link";
import { ArrowLeft, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import type { Approval } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { dec, mul } from "@/lib/decimal";
import { clock, price, quantity, usd } from "@/lib/format";
import { APPROVAL_STATUS_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "@/lib/labels";
import { type ApprovalResponse, approvalAt, useRuntime } from "@/lib/mock-runtime";
import { Panel, WorkspaceGate } from "./common";

/** Approve and Skip share one variant and one size, and neither is focused or selected first (PX-10). */
const CHOICE = "press h-12 w-full text-base font-semibold";

function ResponseStatus({ approval, response }: { approval: Approval; response: ApprovalResponse }) {
  if (response.phase === "sent") {
    return (
      <p className="text-sm" data-phase="sent">
        {response.response === "approve" ? "Your approval was sent." : "Your skip was sent."} Not recorded yet; if the runtime does not record it before the deadline (
        {clock(approval.deadline)}), the action is skipped.
      </p>
    );
  }
  if (approval.status === "delivered") {
    return (
      <p className="text-sm" data-phase="recorded">
        Recorded at {clock(response.recordedAt ?? approval.deadline)}: you approved.{" "}
        {approval.approvals_so_far.length < approval.approvers_required
          ? `Waiting for ${approval.approvers_required - approval.approvals_so_far.length} more approver before the deadline.`
          : "The gate is re-running its checks on the bound quantity, limit price, and mandate version."}
      </p>
    );
  }
  return null;
}

function Outcome({ approval }: { approval: Approval }) {
  if (approval.status === "delivered" || !approval.resolution) return null;
  return (
    <section aria-label="Outcome" data-status={approval.status} className="grid gap-1 rounded-xl border bg-muted/60 p-4">
      <p className="font-semibold">{APPROVAL_STATUS_LABEL[approval.status]}</p>
      <p className="text-sm">
        {clock(approval.resolution.at)}: {approval.resolution.text}
      </p>
    </section>
  );
}

function Request({ approvalId }: { approvalId: string }) {
  const { ws, now, responses, respond } = useRuntime();
  const raw = ws.approvals.find((a) => a.approval_id === approvalId);
  if (!raw) {
    return (
      <Panel className="max-w-xl">
        <h1 className="text-heading">No request with this ID</h1>
        <p className="mt-2 text-muted-foreground">This workspace has no approval request with that ID.</p>
        <Link href="/approvals" className="mt-4 inline-block text-primary underline-offset-4 hover:underline">
          See all approvals
        </Link>
      </Panel>
    );
  }
  const approval = approvalAt(raw, now);
  const agent = findAgent(ws, approval.agent_id);
  const response = responses[approval.approval_id];
  const b = approval.bound;
  const orderValue = usd(mul(dec(b.qty), dec(b.limit)));
  const open = approval.status === "delivered";

  return (
    <article className="mx-auto grid w-full max-w-2xl gap-6" aria-labelledby="request-title">
      <Link href="/approvals" className="inline-flex w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
        <ArrowLeft className="size-4" aria-hidden />
        Approvals
      </Link>

      <header className="grid gap-2">
        <h1 id="request-title" className="text-heading text-muted-foreground">
          Approval request
        </h1>
        <p className="text-sm text-muted-foreground">
          {agent?.label ?? "An agent"} ({agent?.mandate.name ?? "unknown mandate"}) proposes:
        </p>
        <p className="font-display text-title sm:text-display">
          Buy <span className="font-mono tabular">{quantity(b.qty)}</span> {b.symbol} at a limit of <span className="font-mono tabular">{price(b.limit)}</span>
        </p>
      </header>

      <Panel className="grid gap-4">
        <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm sm:grid-cols-3">
          <div>
            <dt className="text-caption text-muted-foreground">Order value</dt>
            <dd className="font-mono tabular">{orderValue}</dd>
          </div>
          <div>
            <dt className="text-caption text-muted-foreground">Purpose</dt>
            <dd>{PURPOSE_LABEL[b.purpose]}</dd>
          </div>
          <div>
            <dt className="text-caption text-muted-foreground">Mandate version</dt>
            <dd className="font-mono text-caption">{b.mandate_version.slice(7, 19)}</dd>
          </div>
        </dl>
        <div className="grid gap-1 border-t pt-4">
          <p className="text-caption text-muted-foreground">Why you are asked</p>
          <p>{approval.trigger}</p>
        </div>
        <div className="grid gap-1 border-t pt-4">
          <p className="text-caption text-muted-foreground">Combined model score, not a probability of profit</p>
          <p className="font-mono tabular">{b.combined_score}</p>
        </div>
      </Panel>

      <section aria-labelledby="risk-title" className="grid gap-2">
        <h2 id="risk-title" className="text-heading">
          Risk impact in dollars
        </h2>
        <Panel className="py-1 sm:py-1">
          <dl className="divide-y divide-border/60 text-sm">
            {approval.risk_impact.map((f) => (
              <div key={f.field} className="flex items-baseline justify-between gap-4 py-3">
                <dt>{RISK_FIGURE_LABEL[f.field]}</dt>
                <dd className="text-right font-mono tabular">
                  {usd(f.value)}
                  {f.cap ? (
                    <span className="block font-sans text-caption text-muted-foreground">
                      of the {usd(f.cap)} {RISK_CAP_LABEL[f.field]}
                    </span>
                  ) : null}
                </dd>
              </div>
            ))}
          </dl>
        </Panel>
      </section>

      {approval.approvers_required > 1 ? (
        <p className="text-sm">
          Needs {approval.approvers_required} approvers. Approved so far:{" "}
          {approval.approvals_so_far.length === 0 ? "nobody" : approval.approvals_so_far.map((a) => `${a.user_label} at ${clock(a.at)}`).join(", ")}.
        </p>
      ) : null}

      <Collapsible className="rounded-xl border bg-card">
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-3 rounded-xl px-4 py-3 text-left font-medium outline-none focus-visible:ring-3 focus-visible:ring-ring/50">
          View model output
          <ChevronDown className="size-4 transition-transform duration-200 group-data-[state=open]:rotate-180" aria-hidden />
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-3 px-4 pb-4">
          {approval.evidence.map((e) => (
            <figure key={e.model_id} className="grid gap-1.5 rounded-lg bg-muted/60 p-3">
              <figcaption className="text-caption text-muted-foreground">
                {e.author === "owner_selected" ? "Output of software you selected" : <span className="text-orchid-text">Platform-authored</span>}: {e.model_id} {e.version}, at{" "}
                {clock(e.produced_at)}
              </figcaption>
              <blockquote className="grid gap-0.5 font-mono text-caption">
                {e.lines.map((line) => (
                  <p key={line}>{line}</p>
                ))}
              </blockquote>
            </figure>
          ))}
        </CollapsibleContent>
      </Collapsible>

      {open ? (
        <section aria-label="Your response" className="sticky bottom-16 z-10 -mx-4 grid gap-3 border-t bg-background/95 px-4 py-4 backdrop-blur sm:static sm:mx-0 sm:rounded-xl sm:border sm:bg-card sm:px-5 lg:bottom-0">
          <p className="font-medium">If you do nothing, this action is skipped.</p>
          <Deadline deadline={approval.deadline} now={now} />
          {response ? (
            <div role="status" aria-live="polite">
              <ResponseStatus approval={approval} response={response} />
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3" data-slot="approval-choices">
              <Button variant="outline" size="lg" className={CHOICE} onClick={() => respond(approval.approval_id, "approve")}>
                Approve
              </Button>
              <Button variant="outline" size="lg" className={CHOICE} onClick={() => respond(approval.approval_id, "skip")}>
                Skip
              </Button>
            </div>
          )}
        </section>
      ) : (
        <Outcome approval={approval} />
      )}
    </article>
  );
}

export function ApprovalRequestScreen({ approvalId }: { approvalId: string }) {
  return (
    <WorkspaceGate>
      <Request approvalId={approvalId} />
    </WorkspaceGate>
  );
}
