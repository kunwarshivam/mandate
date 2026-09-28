"use client";

import { type CSSProperties, useState } from "react";
import Link from "next/link";
import { ArrowLeft, CaretDown } from "@phosphor-icons/react";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Collapsible } from "@cloudflare/kumo/primitives/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import { LimitRail } from "@/components/domain/envelope";
import { ApprovalChart } from "@/components/charts/price-chart";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { Approval, Environment, ModelOutput, RiskFigure, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { dec, max, mul, sub, ZERO } from "@/lib/decimal";
import { clock, price, quantity, usd, zoneLabel } from "@/lib/format";
import { useFrozen } from "@/lib/frozen";
import { APPROVAL_STATUS_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "@/lib/labels";
import { type ApprovalResponse, approvalAt, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { WorkspaceGate } from "./common";

/** Approve and Skip share one variant and one size, and neither is focused or selected first (PX-10). */
const CHOICE = "h-12 w-full justify-center text-base";
const BACK = "inline-flex h-11 w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground lg:h-8";

function ResponseStatus({ approval, response }: { approval: Approval; response: ApprovalResponse }) {
  if (response.phase === "sent") {
    return (
      <p className="text-sm" data-phase="sent">
        {response.response === "approve" ? "Your approval was sent." : "Your skip was sent."} Not recorded yet; if the runtime does not record it before the deadline (
        {clock(approval.deadline)}), the action is skipped.
      </p>
    );
  }
  if (response.phase === "unknown") {
    return (
      <p className="text-sm" data-phase="unknown">
        The result is unknown; we are checking. Until the journal answers, treat the action as not approved; it is skipped at {clock(approval.deadline)} if no record arrives.
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
    <section aria-label="Outcome" data-status={approval.status} className="reveal grid gap-1 border-t-4 border-foreground bg-muted px-3 py-3 sm:px-4">
      <p className="font-display text-heading uppercase">{APPROVAL_STATUS_LABEL[approval.status]}</p>
      <p className="text-sm">
        {clock(approval.resolution.at)}: {approval.resolution.text}
      </p>
    </section>
  );
}

function RiskFigureRow({ figure }: { figure: RiskFigure }) {
  const label = RISK_FIGURE_LABEL[figure.field];
  if (!figure.cap) {
    return (
      <div className="flex items-baseline justify-between gap-4 text-sm">
        <dt className="font-bold">{label}</dt>
        <dd className="text-right font-mono font-bold tabular">{usd(figure.value)}</dd>
      </div>
    );
  }
  return <LimitRail rail={{ key: figure.field, label, used: dec(figure.value), cap: dec(figure.cap), atCap: "" }} caption={riskCaption(figure)!} />;
}

function NotFound() {
  return (
    <section aria-labelledby="missing-title" className="reveal grid max-w-3xl gap-3 border-t-4 border-foreground bg-muted p-4 sm:p-6">
      <h1 id="missing-title" className="text-title sm:text-display">
        No request with this ID
      </h1>
      <p className="max-w-prose">This workspace has no approval request with that ID.</p>
      <LinkButton href="/approvals" variant="outline" size="lg" className="h-11 w-fit">
        See all approvals
      </LinkButton>
    </section>
  );
}

/** The request as D6 shows it, fixed at first render; its status and resolution stay live, outside the record. */
interface RequestSnapshot {
  environment: Environment;
  proposer: string;
  approval: Pick<Approval, "requested_at" | "deadline" | "bound" | "trigger" | "risk_impact" | "evidence" | "approvers_required" | "approvals_so_far">;
}

function snapshotOf(ws: Workspace, approval: Approval): RequestSnapshot {
  const agent = findAgent(ws, approval.agent_id);
  const { requested_at, deadline, bound, trigger, risk_impact, evidence, approvers_required, approvals_so_far } = approval;
  return {
    environment: ws.environment,
    proposer: `${agent?.label ?? "An agent"} (${agent?.mandate.name ?? "unknown mandate"}) proposes:`,
    approval: { requested_at, deadline, bound, trigger, risk_impact, evidence, approvers_required, approvals_so_far },
  };
}

function approversText(a: Pick<Approval, "approvers_required" | "approvals_so_far">): string {
  return `Needs ${a.approvers_required} approvers. Approved so far: ${a.approvals_so_far.length === 0 ? "nobody" : a.approvals_so_far.map((x) => `${x.user_label} at ${clock(x.at)}`).join(", ")}.`;
}

function orderValueOf(a: RequestSnapshot["approval"]): string {
  return usd(mul(dec(a.bound.qty), dec(a.bound.limit)));
}

function authorText(author: ModelOutput["author"]): string {
  return author === "owner_selected" ? "Output of software you selected" : "Platform-authored";
}

function riskCaption(figure: RiskFigure): string | null {
  if (!figure.cap) return null;
  const used = dec(figure.value);
  const cap = dec(figure.cap);
  return used > cap ? `The ${RISK_CAP_LABEL[figure.field]} is ${usd(cap)}.` : `${usd(max(sub(cap, used), ZERO))} under the ${RISK_CAP_LABEL[figure.field]}.`;
}

/** Every line of the request as shown, in order, and model output's lines only if the owner opened it. */
export function requestLines(snap: RequestSnapshot, modelOutputExpanded: boolean): string[] {
  const a = snap.approval;
  const b = a.bound;
  return [
    "Approval request",
    snap.proposer,
    `Buy ${quantity(b.qty)} ${b.symbol} at a limit of ${price(b.limit)}`,
    `Order value: ${orderValueOf(a)}`,
    `Purpose: ${PURPOSE_LABEL[b.purpose]}`,
    `Mandate version: ${b.mandate_version.slice(7, 19)}`,
    `Why you are asked: ${a.trigger}`,
    `Combined model score, not a probability of profit: ${b.combined_score}`,
    "Risk impact in dollars",
    ...a.risk_impact.flatMap((f) => [`${RISK_FIGURE_LABEL[f.field]}: ${usd(f.value)}`, ...(riskCaption(f) ? [riskCaption(f)!] : [])]),
    ...(a.approvers_required > 1 ? [approversText(a)] : []),
    "If you do nothing, this action is skipped.",
    `Skipped at ${clock(a.deadline)} ${zoneLabel(a.deadline)} if you do nothing`,
    ...(modelOutputExpanded ? a.evidence.flatMap((e) => [`${authorText(e.author)}: ${e.model_id} ${e.version}, at ${clock(e.produced_at)}`, ...e.lines]) : []),
  ];
}

function Request({ approvalId }: { approvalId: string }) {
  const { ws, now, responses, respond } = useRuntime();
  const canRespond = useCan("approvals.respond");
  const [modelOutputExpanded, setModelOutputExpanded] = useState(false);
  const raw = ws.approvals.find((a) => a.approval_id === approvalId);
  const approval = raw ? approvalAt(raw, now) : null;
  const response = approval ? responses[approval.approval_id] : undefined;
  const open = approval?.status === "delivered";
  const { shown: snap, stale, refresh } = useFrozen(approval ? snapshotOf(ws, approval) : null, Boolean(response) || !open);
  if (!approval || !snap) return <NotFound />;
  const a = snap.approval;
  const b = a.bound;
  const capped = a.risk_impact.filter((f) => f.cap);
  const uncapped = a.risk_impact.filter((f) => !f.cap);
  const answer = (choice: "approve" | "skip") =>
    respond(approval.approval_id, choice, { screen: "D6", environment: snap.environment, shown: requestLines(snap, modelOutputExpanded), modelOutputExpanded });

  return (
    <article className="grid w-full max-w-3xl grid-cols-1 gap-(--seam)" aria-labelledby="request-title">
      <Link href="/approvals" className={BACK}>
        <ArrowLeft className="size-4" aria-hidden />
        Approvals
      </Link>

      <div data-slot="record" className="grid grid-cols-1 gap-(--seam)">
        <header className="reveal grid gap-2 bg-card px-3 py-3 sm:px-4 sm:py-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h1 id="request-title" className="label-caps text-muted-foreground">
              Approval request
            </h1>
            <EnvironmentBadge environment={snap.environment} />
          </div>
          <p className="text-sm text-muted-foreground">{snap.proposer}</p>
          <p className="font-display text-title leading-[0.95] font-bold uppercase sm:text-display">
            Buy <span className="tabular">{quantity(b.qty)}</span> {b.symbol} at a limit of <span className="tabular">{price(b.limit)}</span>
          </p>
          <dl className="mt-1 grid grid-cols-2 gap-x-4 gap-y-2 border-t pt-3 text-sm sm:grid-cols-3">
            <div>
              <dt className="label-caps text-muted-foreground">Order value</dt>
              <dd className="font-mono tabular">{orderValueOf(a)}</dd>
            </div>
            <div>
              <dt className="label-caps text-muted-foreground">Purpose</dt>
              <dd>{PURPOSE_LABEL[b.purpose]}</dd>
            </div>
            <div>
              <dt className="label-caps text-muted-foreground">Mandate version</dt>
              <dd className="font-mono text-caption">{b.mandate_version.slice(7, 19)}</dd>
            </div>
          </dl>
          <ApprovalChart approval={a} className="border-t pt-3" />
          <div className="grid gap-0.5 border-t pt-3">
            <p className="label-caps text-muted-foreground">Why you are asked</p>
            <p>{a.trigger}</p>
          </div>
          <div className="grid gap-0.5 border-t pt-3">
            <p className="text-caption text-muted-foreground">Combined model score, not a probability of profit</p>
            <p className="font-mono tabular">{b.combined_score}</p>
          </div>
        </header>

        <section aria-labelledby="risk-title" className="reveal grid gap-4 border-t-4 border-mandate-edge bg-mandate px-3 pt-2 pb-3 text-mandate-foreground sm:px-4 sm:pt-3 sm:pb-4" style={{ "--i": 1 } as CSSProperties}>
          <div className="grid gap-1">
            <h2 id="risk-title" className="text-heading text-mandate-strong">
              Risk impact in dollars
            </h2>
            <p className="text-sm text-mandate-muted">Measured against your mandate, as if this order fills.</p>
          </div>
          {capped.length > 0 ? (
            <div className="grid gap-4">
              {capped.map((f) => (
                <RiskFigureRow key={f.field} figure={f} />
              ))}
            </div>
          ) : null}
          {uncapped.length > 0 ? (
            <dl className="grid gap-2 border-t border-mandate-strong/25 pt-3">
              {uncapped.map((f) => (
                <RiskFigureRow key={f.field} figure={f} />
              ))}
            </dl>
          ) : null}
        </section>

        {a.approvers_required > 1 ? (
          <p className="bg-card px-3 py-3 text-sm sm:px-4" data-slot="approvers">
            {approversText(a)}
          </p>
        ) : null}

        <Collapsible.Root className="bg-card" onOpenChange={(opened) => (opened ? setModelOutputExpanded(true) : undefined)}>
          <Collapsible.Trigger className="group flex min-h-11 w-full scroll-mb-60 items-center justify-between gap-3 px-3 py-3 text-left font-bold outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset sm:px-4 lg:scroll-mb-0">
            View model output
            <CaretDown className="size-4 transition-transform duration-200 ease-(--ease-out) group-data-[panel-open]:rotate-180" aria-hidden />
          </Collapsible.Trigger>
          <Collapsible.Panel className="grid gap-(--seam) px-3 pb-3 sm:px-4 sm:pb-4">
            {a.evidence.map((e) => (
              <figure key={e.model_id} className="grid gap-1.5 bg-muted p-3">
                <figcaption className="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
                  {e.author === "owner_selected" ? (
                    authorText(e.author)
                  ) : (
                    <span className="inline-flex h-6 items-center border-2 border-dashed border-foreground px-1.5 label-caps text-foreground">{authorText(e.author)}</span>
                  )}
                  <span>
                    {e.model_id} {e.version}, at {clock(e.produced_at)}
                  </span>
                </figcaption>
                <blockquote className="grid gap-0.5 font-mono text-caption">
                  {e.lines.map((line) => (
                    <p key={line}>{line}</p>
                  ))}
                </blockquote>
              </figure>
            ))}
          </Collapsible.Panel>
        </Collapsible.Root>
      </div>

      {open ? (
        <section
          aria-label="Your response"
          className="sticky bottom-[calc(3.5rem+2px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) grid gap-2 border-t-2 border-foreground bg-muted px-(--page-x) py-3 lg:static lg:mx-0 lg:border-t-4 lg:px-4 lg:py-4"
        >
          <p className="font-display text-xl leading-none font-extrabold uppercase lg:text-heading">If you do nothing, this action is skipped.</p>
          <Deadline deadline={a.deadline} now={now} />
          {response ? null : !canRespond ? (
            <p className="text-sm" data-slot="read-only">
              Your role can read requests. An owner, operator, or approver responds.
            </p>
          ) : stale ? (
            <div data-slot="record-changed" className="grid gap-2">
              <p className="text-sm font-bold">This request changed since the page opened, so the record above is out of date. Nothing was sent.</p>
              <Button variant="outline" size="lg" className="h-11 w-fit" onClick={refresh}>
                Show the current version
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-(--seam) pt-1" data-slot="approval-choices">
              <Button variant="secondary" size="lg" data-variant="secondary" data-size="lg" className={CHOICE} onClick={() => answer("approve")}>
                Approve
              </Button>
              <Button variant="secondary" size="lg" data-variant="secondary" data-size="lg" className={CHOICE} onClick={() => answer("skip")}>
                Skip
              </Button>
            </div>
          )}
        </section>
      ) : null}

      {response ? (
        <section aria-labelledby="after-response" data-slot="after-confirm" className="grid gap-2 border-2 border-dashed border-foreground bg-background px-3 py-3 sm:px-4">
          <h2 id="after-response" className="text-heading">
            After you responded
          </h2>
          <p className="text-sm text-muted-foreground">Live progress. It is not part of the request above.</p>
          <div role="status" aria-live="polite" className="grid gap-2">
            <ResponseStatus approval={approval} response={response} />
            {approval.approvers_required > 1 ? <p className="text-sm">{approversText(approval)}</p> : null}
            <Outcome approval={approval} />
          </div>
        </section>
      ) : open ? null : (
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
