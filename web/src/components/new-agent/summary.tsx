"use client";

import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Undo } from "pixelarticons/react/Undo.js";
import { HatchingOwl } from "@/components/domain/owl";
import { KEY } from "@/components/kumo/key";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StepUpDialog } from "@/components/stop/step-up-dialog";
import type { ContentRef, Provenance } from "@/fixtures/types";
import { type Dec, sub } from "@/lib/decimal";
import { type NewAgent, independentDeployRefusal, mandateVersion } from "@/lib/fixture-journey";
import { PROVENANCE_LABEL } from "@/lib/labels";
import { clock, quantity, price, usd, zoneLabel } from "@/lib/format";
import { type Deployment, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { type Draft, goalWords } from "./draft";

/**
 * The whole agent, shown once in the conversation when nothing is missing (DEC-477). It is the
 * confirmation record (brief A5, mandate §10): every value in compact form, nothing collapsed (PX-1),
 * what the platform drafted marked as such, and the version its confirmation binds. One button
 * creates it, with a passkey (G3); the journal keeps every line shown.
 */

export const SUMMARY_TITLE = "Your agent";
export const LEGEND = "Marked Proposed or Default: set by Owlhead to fit inside the loss you gave. Everything else came from you.";
export const GAP_NOTE = "Price gaps and exit prices can make any of these losses larger. You gave the last one; Owlhead worked out the others from it.";
const NOT_ENFORCED_WHY = "No limit can check this, so it isn't enforced. The agent gets it as a note.";

/** The same words as the mandate page's provenance badges (DEC-513), on what the owner did not say. */
const TAG: Record<Provenance, string | null> = {
  user_stated: null,
  user_entered: null,
  template_structure: PROVENANCE_LABEL.template_structure,
  platform_proposed: PROVENANCE_LABEL.platform_proposed,
  platform_default: PROVENANCE_LABEL.platform_default,
};

/** The agent in plain sentences, one per line. */
export function contractTerms(draft: Draft): string[] {
  const t = draft.terms;
  const model = draft.strategy ? `It decides with ${draft.strategy.model.name.toLowerCase()} (${draft.strategy.model.id}), with the settings you chose.` : "It decides with the model you choose, and you have not chosen one yet.";
  return [
    `This agent may use ${usd(t.allocationUsd)} of simulated money, on paper.`,
    goalWords(t),
    `If it loses ${usd(t.maxLossUsd)} in total, at ${usd(sub(t.allocationUsd, t.maxLossUsd))}, it closes everything and pauses until you change the mandate.`,
    model,
    "It asks you before every buy. A buy you do not answer is skipped.",
    draft.symbols.length > 0 ? `It trades only ${draft.symbols.join(", ")}.` : "It trades only the instruments you list, and you have not listed any yet.",
  ];
}

interface Row {
  label: string;
  value: string;
  provenance: Provenance | null;
}

/** The losses in dollars, with who drafted each, and what could buy without asking (DEC-189). */
function dollarRows(draft: Draft): Row[] {
  const f = draft.figures;
  const row = (label: string, value: Dec, provenance: Provenance): Row => ({ label, value: usd(value), provenance });
  return [
    row("One full position stopped out", f.positionLossAtStop, "platform_proposed"),
    row("Most it may lose in one day", f.dailyLossBudget, "platform_proposed"),
    row("Fall at which it closes everything", f.flattenLoss, "platform_proposed"),
    row("Most it may lose, in total", f.floorLoss, "user_stated"),
    { label: "Could buy without asking you", value: usd(f.unasked), provenance: null },
  ];
}

const rowLine = (r: Row) => `${r.label}: ${r.value}${r.provenance && TAG[r.provenance] ? ` (${TAG[r.provenance]})` : ""}`;

/** Every line the summary shows, in order, as the journal keeps it with `MandateConfirmed` (brief §4.1, mandate §10). */
export function summaryLines(draft: Draft, version: ContentRef): string[] {
  return [
    SUMMARY_TITLE,
    "PAPER: simulated funds",
    LEGEND,
    ...contractTerms(draft),
    "Losses in dollars",
    ...dollarRows(draft).map(rowLine),
    GAP_NOTE,
    "Every setting",
    ...draft.sections.flatMap((s) => [s.title, ...s.fields.map((f) => rowLine(f))]),
    ...(draft.notEnforced.length === 0 ? [] : ["Not enforced", ...draft.notEnforced.flatMap((n) => [`“${n.quote}”`, NOT_ENFORCED_WHY])]),
    `Version 1: ${version}`,
  ];
}

/** The one action the passkey authorizes, in words. */
export function deployAction(draft: Draft): string {
  return `Create this agent on paper with ${usd(draft.terms.allocationUsd)} of simulated money, trading ${draft.symbols.join(", ")}.`;
}

function Rows({ rows }: { rows: Row[] }) {
  return (
    <dl className="grid">
      {rows.map((r) => {
        const tag = r.provenance ? TAG[r.provenance] : null;
        return (
          <div key={r.label} data-slot="summary-row" data-provenance={r.provenance ?? undefined} className="grid gap-x-4 gap-y-0.5 border-b border-mandate-strong/10 py-2 last:border-b-0 sm:grid-cols-[minmax(0,12rem)_minmax(0,1fr)]">
            <dt className="text-sm text-mandate-muted">{r.label}</dt>
            <dd className="min-w-0 text-sm text-pretty tabular wrap-anywhere">
              {r.value}
              {tag ? (
                <span data-slot="tag" className="ml-2 inline-block rounded-sm border border-dashed border-mandate-strong/40 px-1.5 text-caption text-mandate-muted">
                  {tag}
                </span>
              ) : null}
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

function Part({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="grid gap-1">
      <h3 className="text-label text-mandate-muted">{title}</h3>
      {children}
    </div>
  );
}

/** What happened after the owner created it, from the runtime's journal: never part of the record. */
function Progress({ deployment }: { deployment: Deployment }) {
  const { ws, now } = useRuntime();
  const agent = ws.agents.find((a) => a.agent_id === deployment.agentId);
  const firstAsk = ws.approvals.find((a) => a.agent_id === deployment.agentId);
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  switch (deployment.phase) {
    case "sent":
      return <p data-phase="sent">Creating it… Sent at {at(deployment.sentAt)}. Nothing is active until the runtime records it.</p>;
    case "recorded":
      return (
        <div data-phase="recorded" className="grid gap-4">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
            <HatchingOwl seed={deployment.agentId} className="size-16" />
            <div className="grid min-w-0 gap-0.5">
              <p className="font-semibold">{agent ? `${agent.label} is running on paper.` : "It is running on paper."}</p>
              <p className="text-sm text-pretty text-muted-foreground">Recorded in the journal at {at(deployment.recordedAt ?? now)}, as version 1. This owl is its own, drawn from its ID.</p>
            </div>
          </div>
          {firstAsk ? (
            <p data-slot="first-ask" className="text-pretty">
              Its first check asks you: buy {quantity(firstAsk.bound.qty)} {firstAsk.bound.symbol} at {price(firstAsk.bound.limit)}. If you do not answer by {at(firstAsk.deadline)}, it is skipped.
            </p>
          ) : (
            <p data-slot="first-check" className="text-muted-foreground">
              Its first check runs now.
            </p>
          )}
          <div className="flex flex-wrap gap-3">
            {firstAsk && firstAsk.status === "delivered" ? (
              <Link href={`/approvals/${firstAsk.approval_id}`} className={cn("w-full sm:w-auto", KEY)}>
                Review the request
                <ArrowRight className="size-6" aria-hidden />
              </Link>
            ) : null}
            {agent ? (
              <Link href={`/agents/${agent.agent_id}`} className={cn("w-full sm:w-auto", KEY)}>
                Open {agent.label}
              </Link>
            ) : null}
          </div>
        </div>
      );
    case "rejected":
      return (
        <div data-phase="rejected" role="alert" className="grid gap-1">
          <p className="font-semibold">Not created. Nothing was confirmed, and version 1 does not exist.</p>
          <p className="text-pretty" data-rule={deployment.rule}>
            {deployment.reason}
          </p>
          <p className="text-pretty">Tell me what to change.</p>
        </div>
      );
    case "undelivered":
      return (
        <p data-phase="undelivered" role="alert" className="text-pretty">
          Not delivered: your deployment did not answer, so nothing was confirmed and no agent exists. Try again once it is reachable.
        </p>
      );
    case "unknown":
      return (
        <p data-phase="unknown" className="text-pretty">
          The result is unknown; we are checking. Look in All agents before creating it again, so the same agent is not created twice.
        </p>
      );
    default: {
      const unhandled: never = deployment.phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}

/** A deployment that went through, or may have: the summary it came from can no longer be created again. */
export function isConfirmed(d: Deployment | null): boolean {
  return d !== null && d.phase !== "rejected" && d.phase !== "undelivered";
}

/**
 * The summary card and its one action. `current` is false once the owner changed something after it
 * was shown: the card then gives way to the newer one below it.
 */
export function Summary({
  draft,
  request,
  current,
  blocked,
  deployment,
  onSent,
  onStartOver,
}: {
  draft: Draft;
  request: NewAgent;
  current: boolean;
  blocked: string | null;
  deployment: Deployment | null;
  onSent: (id: string) => void;
  onStartOver: () => void;
}) {
  const { ws, deploy } = useRuntime();
  const mayDeploy = useCan("agents.deploy");
  const headingId = useId();
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const progress = useRef<HTMLElement>(null);
  const version = mandateVersion(request.mandate);
  const independence = independentDeployRefusal(ws);
  const confirmed = isConfirmed(deployment);
  const sentId = deployment?.id ?? null;
  useEffect(() => {
    if (!sentId) return;
    progress.current?.querySelector<HTMLElement>("h3")?.focus({ preventScroll: true });
    progress.current?.scrollIntoView({ block: "nearest" });
  }, [sentId]);
  const phase = deployment?.phase ?? null;
  useEffect(() => {
    if (phase === "recorded") progress.current?.scrollIntoView({ block: "nearest" });
  }, [phase]);

  const create = () => {
    const sent = deploy(request, { screen: "A5", environment: ws.environment, shown: summaryLines(draft, version) });
    setNotice(null);
    onSent(sent.id);
  };

  return (
    <div className="grid gap-4">
      <section aria-labelledby={headingId} data-slot="summary" className="grid gap-5 rounded-2xl bg-mandate px-5 py-5 text-mandate-foreground sm:px-6">
        <div className="grid gap-1">
          <div className="flex flex-wrap items-center gap-3">
            <h2 id={headingId} className="text-h3 text-mandate-strong">
              {SUMMARY_TITLE}
            </h2>
            <EnvironmentBadge environment="paper" />
          </div>
          <p className="text-caption text-pretty text-mandate-muted">{LEGEND}</p>
        </div>

        <ul className="grid gap-1.5 text-pretty tabular" data-slot="terms">
          {contractTerms(draft).map((term) => (
            <li key={term}>{term}</li>
          ))}
        </ul>

        <Part title="Losses in dollars">
          <Rows rows={dollarRows(draft)} />
          <p className="pt-1 text-caption text-pretty text-mandate-muted">{GAP_NOTE}</p>
        </Part>

        <Part title="Every setting">
          <div className="grid gap-3">
            {draft.sections.map((s) => (
              <div key={s.key} data-slot="summary-section" data-section={s.key} className="grid">
                <h4 className="pt-1 text-sm font-semibold">{s.title}</h4>
                <Rows rows={s.fields} />
              </div>
            ))}
          </div>
        </Part>

        {draft.notEnforced.length > 0 ? (
          <Part title="Not enforced">
            <ul className="grid gap-2" data-slot="not-enforced">
              {draft.notEnforced.map((n) => (
                <li key={n.quote} data-slot="not-enforced-item" className="grid gap-0.5">
                  <span className="text-sm text-pretty wrap-anywhere">“{n.quote}”</span>
                  <span className="text-caption text-pretty text-mandate-muted">{NOT_ENFORCED_WHY}</span>
                </li>
              ))}
            </ul>
          </Part>
        ) : null}

        <p className="text-caption text-mandate-muted">
          Version 1:{" "}
          <span className="font-mono wrap-anywhere" translate="no" data-slot="mandate-version">
            {version}
          </span>
        </p>
      </section>

      {current && !confirmed ? (
        !mayDeploy ? (
          <p className="text-sm">Only the workspace owner can create an agent.</p>
        ) : independence ? (
          <p className="font-medium text-pretty" data-slot="blocked" data-rule={independence.rule}>
            {independence.reason}
          </p>
        ) : blocked ? (
          <p className="font-medium text-pretty" data-slot="blocked">
            {blocked}
          </p>
        ) : (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={() => setAsking(true)}>
              Create agent
            </Button>
            <p className="text-sm text-muted-foreground">Uses your passkey. Or tell me what to change.</p>
          </div>
        )
      ) : null}

      <div role="status" aria-live="polite" className="grid gap-3">
        {notice ? <p className="text-sm">{notice}</p> : null}
        {deployment ? (
          <section ref={progress} aria-labelledby={`${headingId}-after`} data-slot="after-confirm" className="grid gap-3 border-l-2 border-border pl-4">
            <h3 id={`${headingId}-after`} tabIndex={-1} className="text-caption text-muted-foreground outline-none">
              After you created it
            </h3>
            <Progress deployment={deployment} />
          </section>
        ) : null}
      </div>

      {deployment?.phase === "recorded" ? (
        <div>
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={onStartOver}>
            <Undo className="size-6" aria-hidden />
            Set up another agent
          </Button>
        </div>
      ) : null}

      <StepUpDialog
        open={asking}
        action={deployAction(draft)}
        finalFocus={() => progress.current?.querySelector<HTMLElement>("h3") ?? true}
        onVerified={() => {
          setAsking(false);
          create();
        }}
        onCancel={() => {
          setAsking(false);
          setNotice("Passkey check canceled. Nothing was created or sent.");
        }}
        onFailed={() => {
          setAsking(false);
          setNotice("Passkey check failed. Nothing was created or sent.");
        }}
      />
    </div>
  );
}
