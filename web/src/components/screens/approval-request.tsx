"use client";

import { type CSSProperties, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { motion, useReducedMotion } from "motion/react";
import { ArrowLeft, CaretDown } from "@phosphor-icons/react";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Collapsible } from "@cloudflare/kumo/primitives/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import { ResponseProgress, responseSteps } from "@/components/approvals/response-progress";
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
import { cn } from "@/lib/utils";
import { WorkspaceGate } from "./common";

/** Approve and Skip share one variant and one size, and neither is focused or selected first (PX-10). */
const CHOICE = "h-12 w-full justify-center rounded-lg text-base font-semibold";
const BACK = "-ml-2 inline-flex h-11 w-fit items-center gap-1.5 rounded-lg px-2 text-sm text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring lg:h-9";

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
    <section aria-label="Outcome" data-status={approval.status} className="reveal grid gap-1.5 rounded-2xl bg-background px-5 py-4">
      <p className="text-h3">{APPROVAL_STATUS_LABEL[approval.status]}</p>
      <p className="text-sm">
        {clock(approval.resolution.at)}: {approval.resolution.text}
      </p>
    </section>
  );
}

/**
 * The response bar once the owner has chosen: the choice takes the buttons' place, with where it
 * stands and what happened, so nothing moves above it and the result lands where they were looking.
 */
function AfterResponse({ approval, response, focusOnShow }: { approval: Approval; response: ApprovalResponse; focusOnShow: boolean }) {
  const heading = useRef<HTMLHeadingElement>(null);
  const reduceMotion = useReducedMotion();
  useEffect(() => {
    if (focusOnShow) heading.current?.focus({ preventScroll: true });
  }, [focusOnShow]);
  return (
    <motion.div
      data-slot="after-confirm"
      className="grid gap-3"
      initial={reduceMotion ? false : { opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2, ease: [0.23, 1, 0.32, 1] }}
    >
      <div className="grid gap-0.5">
        <h2 id="after-response" ref={heading} tabIndex={-1} className="text-h3 outline-none">
          {response.response === "approve" ? "You chose Approve" : "You chose Skip"}
        </h2>
        <p className="text-caption text-muted-foreground">Live progress. It is not part of the request above.</p>
      </div>
      <ResponseProgress steps={responseSteps(approval, response)} />
      <div role="status" aria-live="polite" className="grid gap-2">
        <ResponseStatus approval={approval} response={response} />
        {approval.approvers_required > 1 ? <p className="text-sm">{approversText(approval)}</p> : null}
        <Outcome approval={approval} />
      </div>
    </motion.div>
  );
}

function RiskFigureRow({ figure }: { figure: RiskFigure }) {
  const label = RISK_FIGURE_LABEL[figure.field];
  if (!figure.cap) {
    return (
      <div className="flex items-baseline justify-between gap-4 text-sm">
        <dt className="font-medium">{label}</dt>
        <dd className="text-right font-mono font-medium tabular">{usd(figure.value)}</dd>
      </div>
    );
  }
  return <LimitRail rail={{ key: figure.field, label, used: dec(figure.value), cap: dec(figure.cap), atCap: "" }} caption={riskCaption(figure)!} />;
}

function NotFound() {
  return (
    <section aria-labelledby="missing-title" className="reveal grid max-w-2xl gap-4 pt-6 sm:pt-12">
      <h1 id="missing-title" className="text-h1">
        No request with this ID
      </h1>
      <p className="max-w-measure text-muted-foreground">This workspace has no approval request with that ID.</p>
      <LinkButton href="/approvals" variant="outline" size="lg" className="h-11 w-fit rounded-lg px-5">
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

/* The bar's height changes with its contents, so toasts read it to rise clear of it on a phone. */
function publishBarHeight(el: HTMLElement | null) {
  if (!el || typeof ResizeObserver === "undefined") return;
  const root = document.documentElement;
  const observer = new ResizeObserver(() => root.style.setProperty("--response-bar", `${el.offsetHeight}px`));
  observer.observe(el);
  return () => {
    observer.disconnect();
    root.style.removeProperty("--response-bar");
  };
}

function Request({ approvalId }: { approvalId: string }) {
  const { ws, now, responses, respond } = useRuntime();
  const canRespond = useCan("approvals.respond");
  const [modelOutputExpanded, setModelOutputExpanded] = useState(false);
  const [answered, setAnswered] = useState(false);
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
  const answer = (choice: "approve" | "skip") => {
    setAnswered(true);
    respond(approval.approval_id, choice, { screen: "D6", environment: snap.environment, shown: requestLines(snap, modelOutputExpanded), modelOutputExpanded });
  };

  /*
   * A sticky bar cannot pass the end of its article. On a phone the shell leaves main's 2.5rem and the
   * footer's 2rem below the article, besides the tab bar's clearance, so while the choices are the last
   * thing on the page the article gives that space back and they stay on the tab bar to the very end.
   */
  return (
    <article className={cn("mx-auto grid w-full max-w-2xl grid-cols-1 gap-6", (open || response) && "max-lg:-mb-[calc(4.5rem-1px)]")} aria-labelledby="request-title">
      <Link href="/approvals" className={BACK}>
        <ArrowLeft className="size-4" aria-hidden />
        Approvals
      </Link>

      <div data-slot="record" className="grid grid-cols-1 gap-(--section-gap)">
        <header className="reveal grid gap-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h1 id="request-title" className="text-sm font-medium text-muted-foreground">
              Approval request
            </h1>
            <EnvironmentBadge environment={snap.environment} />
          </div>
          <div className="grid gap-2">
            <p className="text-muted-foreground">{snap.proposer}</p>
            <p className="grid gap-1">
              <span className="text-hero">
                Buy <span className="tabular">{quantity(b.qty)}</span> {b.symbol}
              </span>{" "}
              <span className="text-h1 font-medium">
                at a limit of <span className="tabular">{price(b.limit)}</span>
              </span>
            </p>
          </div>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-4 pt-2 sm:grid-cols-3">
            <div className="grid gap-1">
              <dt className="text-sm text-muted-foreground">Order value</dt>
              <dd className="font-mono text-figure tabular">{orderValueOf(a)}</dd>
            </div>
            <div className="grid gap-1">
              <dt className="text-sm text-muted-foreground">Purpose</dt>
              <dd className="text-figure">{PURPOSE_LABEL[b.purpose]}</dd>
            </div>
            <div className="col-span-2 grid gap-1 sm:col-span-1">
              <dt className="text-sm text-muted-foreground">Mandate version</dt>
              <dd className="font-mono text-sm leading-7">{b.mandate_version.slice(7, 19)}</dd>
            </div>
          </dl>
        </header>

        <section aria-labelledby="why-title" className="reveal grid gap-4" style={{ "--i": 1 } as CSSProperties}>
          <div className="grid gap-1.5">
            <h2 id="why-title" className="text-h2">
              Why you are asked
            </h2>
            <p className="text-lg text-pretty">{a.trigger}</p>
          </div>
          <p className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-t border-border/70 pt-3">
            <span className="text-sm text-muted-foreground">Combined model score, not a probability of profit</span>
            <span className="font-mono text-figure tabular">{b.combined_score}</span>
          </p>
        </section>

        <section
          aria-labelledby="risk-title"
          className="reveal grid gap-5 rounded-2xl bg-mandate px-5 py-5 text-mandate-foreground"
          style={{ "--i": 2 } as CSSProperties}
        >
          <div className="grid gap-1">
            <h2 id="risk-title" className="text-h2 text-mandate-strong">
              Risk impact in dollars
            </h2>
            <p className="text-sm text-mandate-muted">Measured against your mandate, as if this order fills.</p>
          </div>
          {capped.length > 0 ? (
            <div className="grid gap-5">
              {capped.map((f) => (
                <RiskFigureRow key={f.field} figure={f} />
              ))}
            </div>
          ) : null}
          {uncapped.length > 0 ? (
            <dl className="grid gap-2.5 border-t border-mandate-strong/15 pt-4">
              {uncapped.map((f) => (
                <RiskFigureRow key={f.field} figure={f} />
              ))}
            </dl>
          ) : null}
        </section>

        <ApprovalChart approval={a} className="reveal" />

        {a.approvers_required > 1 ? (
          <p className="rounded-2xl bg-background px-5 py-4 text-sm" data-slot="approvers">
            {approversText(a)}
          </p>
        ) : null}

        <Collapsible.Root className="border-y border-border/70" onOpenChange={(opened) => (opened ? setModelOutputExpanded(true) : undefined)}>
          <Collapsible.Trigger className="group -mx-2 flex min-h-12 w-[calc(100%+1rem)] scroll-mb-72 items-center justify-between gap-3 rounded-lg px-2 py-3 text-left font-medium outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset lg:scroll-mb-40">
            View model output
            <CaretDown className="size-4 text-muted-foreground transition-transform duration-200 ease-(--ease-out) group-data-[panel-open]:rotate-180" aria-hidden />
          </Collapsible.Trigger>
          <Collapsible.Panel className="grid gap-2 pb-4">
            {a.evidence.map((e) => (
              <figure key={e.model_id} className="grid gap-2 rounded-xl bg-background px-4 py-3">
                <figcaption className="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
                  {e.author === "owner_selected" ? (
                    authorText(e.author)
                  ) : (
                    <span className="inline-flex h-6 items-center rounded-md border border-dashed border-foreground px-2.5 text-label text-foreground">{authorText(e.author)}</span>
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

      {open || response ? (
        <section
          ref={publishBarHeight}
          aria-label="Your response"
          data-slot="response-bar"
          className="sticky bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) grid gap-3 border-t border-border/70 bg-card px-(--page-x) pt-4 pb-4 lg:bottom-0 lg:mx-0 lg:px-0 lg:pb-[calc(var(--dock-clearance)+1.5rem)]"
        >
          {response ? (
            <AfterResponse approval={approval} response={response} focusOnShow={answered} />
          ) : (
            <div className="grid gap-1">
              <p className="text-h3">If you do nothing, this action is skipped.</p>
              <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
            </div>
          )}
          {response ? null : !canRespond ? (
            <p className="text-sm" data-slot="read-only">
              Your role can read requests. An owner, operator, or approver responds.
            </p>
          ) : stale ? (
            <div data-slot="record-changed" className="grid gap-2">
              <p className="text-sm font-medium">This request changed since the page opened, so the record above is out of date. Nothing was sent.</p>
              <Button variant="outline" size="lg" className="h-11 w-fit rounded-lg px-5" onClick={refresh}>
                Show the current version
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3" data-slot="approval-choices">
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

      {response || open ? null : <Outcome approval={approval} />}
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
