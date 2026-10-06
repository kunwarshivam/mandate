"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowCounterClockwise, ArrowRight } from "@phosphor-icons/react";
import { Owl } from "@/components/domain/owl";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StepUpDialog } from "@/components/stop/step-up-dialog";
import type { ContentRef } from "@/fixtures/types";
import { type NewAgent, mandateVersion } from "@/lib/fixture-journey";
import { clock, quantity, price, usd, zoneLabel } from "@/lib/format";
import { PROVENANCE_LABEL } from "@/lib/labels";
import { type Deployment, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { type Draft, SECTION_KEYS } from "./draft";
import { BackButton, STEP_HEADING, StepHeading } from "./steps";
import {
  CONTRACT_LEAD_CONFIRMED,
  CONTRACT_TITLE,
  ContractCard,
  DraftFields,
  GAP_NOTE,
  NOT_ENFORCED_EMPTY,
  NOT_ENFORCED_LEAD,
  NotEnforcedList,
  UNASKED_LABEL,
  WORDS_TITLE,
  contractTerms,
  lossFigures,
  unaskedNote,
} from "./mandate-parts";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

export const CONFIRM_TITLE = "Confirm your mandate";
const FIELDS_TITLE = "What you confirmed";
const FIELDS_LEAD = "Every field, with who wrote it, exactly as you saw it.";
const ENVIRONMENT = "Paper: simulated funds, no real money.";

function sectionsLine(): string {
  return `${SECTION_KEYS.length} of ${SECTION_KEYS.length}, each by you, one at a time.`;
}

/**
 * Every line of A5 as shown, in order, as the journal keeps it with `MandateConfirmed` (brief §4.1,
 * mandate §10). Nothing on the screen collapses, so every line is here.
 */
export function confirmationLines(draft: Draft, version: ContentRef): string[] {
  return [
    CONFIRM_TITLE,
    `Version 1: ${version}`,
    `Environment: ${ENVIRONMENT}`,
    `Sections confirmed: ${sectionsLine()}`,
    CONTRACT_TITLE,
    CONTRACT_LEAD_CONFIRMED,
    ...contractTerms(draft),
    WORDS_TITLE,
    ...draft.answers.map((a) => `${a.label}: “${a.quote}”`),
    UNASKED_LABEL,
    usd(draft.figures.unasked),
    unaskedNote(draft),
    "Losses in dollars",
    ...lossFigures(draft).map((f) => `${f.label}: ${usd(f.value)} (${PROVENANCE_LABEL[f.provenance]})`),
    GAP_NOTE,
    FIELDS_TITLE,
    FIELDS_LEAD,
    ...draft.sections.flatMap((s) => [
      s.title,
      ...s.fields.flatMap((f) => [`${f.label}: ${f.value}${f.provenance ? ` (${PROVENANCE_LABEL[f.provenance]})` : ""}`, ...(f.quote ? [`You said “${f.quote}”`] : [])]),
    ]),
    "Not enforced",
    NOT_ENFORCED_LEAD,
    ...(draft.notEnforced.length === 0 ? [NOT_ENFORCED_EMPTY] : draft.notEnforced.flatMap((n) => [`“${n.quote}”`, n.why])),
  ];
}

/** The one action the passkey authorizes, in words. */
export function deployAction(draft: Draft): string {
  return `Confirm this mandate and deploy a new agent to paper with ${usd(draft.terms.allocationUsd)} of simulated money, trading ${draft.symbols.join(", ")}.`;
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="grid gap-0.5 border-b border-border/70 py-3 last:border-b-0 sm:grid-cols-[minmax(0,11rem)_minmax(0,1fr)] sm:gap-x-6">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="min-w-0 wrap-anywhere">{children}</dd>
    </div>
  );
}

/** What happened after the owner confirmed, from the runtime's journal: never part of the record. */
function Progress({ deployment, onBack }: { deployment: Deployment; onBack: () => void }) {
  const { ws, now } = useRuntime();
  const agent = ws.agents.find((a) => a.agent_id === deployment.agentId);
  const firstAsk = ws.approvals.find((a) => a.agent_id === deployment.agentId);
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  switch (deployment.phase) {
    case "sent":
      return (
        <p data-phase="sent" className="rounded-xl bg-background px-4 py-3 text-sm">
          Sent at {at(deployment.sentAt)}; waiting for the runtime to record it. Nothing is active until it does.
        </p>
      );
    case "recorded":
      return (
        <div data-phase="recorded" className="grid gap-4">
          <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
            <Owl seed={deployment.agentId} mood="awake" className="size-16 sm:size-20" />
            <div className="grid min-w-0 gap-1">
              <p className="text-h3">{agent ? `${agent.label} is running on paper` : "Deployed to paper"}</p>
              <p className="text-sm text-pretty text-muted-foreground">
                Recorded in the journal at {at(deployment.recordedAt ?? now)}, as version 1. This is its owl; every agent gets its own, drawn from its ID.
              </p>
            </div>
          </div>
          {firstAsk ? (
            <p data-slot="first-ask" className="rounded-xl bg-background px-4 py-3 text-sm text-pretty">
              Its first check asks you: buy {quantity(firstAsk.bound.qty)} {firstAsk.bound.symbol} at {price(firstAsk.bound.limit)}. If you do not answer by{" "}
              {at(firstAsk.deadline)}, it is skipped.
            </p>
          ) : (
            <p data-slot="first-check" className="text-sm text-muted-foreground">
              Its first check runs now.
            </p>
          )}
          <div className="flex flex-wrap gap-3">
            {firstAsk && firstAsk.status === "delivered" ? (
              <Link href={`/approvals/${firstAsk.approval_id}`} className={cn("w-full sm:w-auto", KEY)}>
                Review the request
                <ArrowRight className="size-4" aria-hidden />
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
        <div data-phase="rejected" role="alert" className="grid gap-3 rounded-xl bg-background px-4 py-3 text-sm">
          <p className="font-semibold">Not deployed. Nothing was confirmed, and version 1 does not exist.</p>
          <p className="text-pretty">{deployment.reason}</p>
          <BackButton onClick={onBack}>Change your answers</BackButton>
        </div>
      );
    case "undelivered":
      return (
        <p data-phase="undelivered" role="alert" className="rounded-xl bg-background px-4 py-3 text-sm text-pretty">
          Not delivered: your deployment did not answer, so nothing was confirmed and no agent exists. Try again once it is reachable.
        </p>
      );
    case "unknown":
      return (
        <p data-phase="unknown" className="rounded-xl bg-background px-4 py-3 text-sm text-pretty">
          The result is unknown; we are checking. Look in All agents before confirming again, so the same mandate is not deployed twice.
        </p>
      );
    default: {
      const unhandled: never = deployment.phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}

/**
 * A5, a record screen (brief §4.1): the whole mandate, expanded, with the version its confirmation
 * binds. The owner confirms it with a passkey (G3), and the journal keeps every line shown. What
 * happens next comes from the runtime and sits apart from the record, under "After you confirmed".
 */
export function Confirmation({ draft, request, onBack, onStartOver }: { draft: Draft; request: NewAgent; onBack: () => void; onStartOver: () => void }) {
  const { ws, deploy, deployments } = useRuntime();
  const mayDeploy = useCan("agents.deploy");
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [sentId, setSentId] = useState<string | null>(null);
  const deployment = deployments.find((d) => d.id === sentId) ?? null;
  const shownVersion = mandateVersion(request.mandate);
  const confirmed = deployment !== null && deployment.phase !== "rejected" && deployment.phase !== "undelivered";
  const progress = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!sentId) return;
    progress.current?.querySelector<HTMLElement>("h2")?.focus({ preventScroll: true });
    progress.current?.scrollIntoView({ block: "start" });
  }, [sentId]);
  const phase = deployment?.phase ?? null;
  useEffect(() => {
    if (phase === "recorded") progress.current?.scrollIntoView({ block: "nearest" });
  }, [phase]);

  const confirm = () => {
    const sent = deploy(request, { screen: "A5", environment: ws.environment, shown: confirmationLines(draft, shownVersion) });
    setNotice(null);
    setSentId(sent.id);
  };

  return (
    <article aria-labelledby={STEP_HEADING} data-slot="new-agent-record" className="grid gap-(--section-gap)">
      <div data-slot="record" className="grid gap-(--section-gap)">
        <header className="grid gap-3">
          <p className="text-label text-muted-foreground">Confirm</p>
          <StepHeading className="flex flex-wrap items-center gap-3">
            {CONFIRM_TITLE}
            <EnvironmentBadge environment="paper" />
          </StepHeading>
          <p className="max-w-measure text-pretty text-muted-foreground">Everything you confirmed, as the journal keeps it with your confirmation. Nothing here is collapsed.</p>
        </header>

        <dl className="grid">
          <Fact term="Version">
            <span className="grid gap-0.5">
              <span>
                Version <span className="tabular">1</span>
              </span>
              <span className="font-mono text-caption text-muted-foreground" translate="no" data-slot="mandate-version">
                {shownVersion}
              </span>
            </span>
          </Fact>
          <Fact term="Environment">{ENVIRONMENT}</Fact>
          <Fact term="Sections confirmed">
            <span className="tabular">{sectionsLine()}</span>
          </Fact>
        </dl>

        <ContractCard draft={draft} confirmed />

        <section aria-labelledby="record-fields" className="grid gap-(--section-gap)">
          <div className="grid gap-1.5">
            <h2 id="record-fields" className="text-h2">
              {FIELDS_TITLE}
            </h2>
            <p className="max-w-measure text-muted-foreground">{FIELDS_LEAD}</p>
          </div>
          {draft.sections.map((section) => (
            <section key={section.key} aria-labelledby={`record-${section.key}`} data-slot="record-section" className="grid gap-2">
              <h3 id={`record-${section.key}`} className="text-h3">
                {section.title}
              </h3>
              <DraftFields fields={section.fields} confirmed />
            </section>
          ))}
        </section>

        <NotEnforcedList draft={draft} />
      </div>

      <section aria-label="Confirm" className="grid gap-3 rounded-2xl bg-background px-5 py-5 sm:px-6">
        {confirmed ? (
          <p className="text-sm" data-slot="confirmed">
            You confirmed the record above. It stays as you saw it; progress is under “After you confirmed”.
          </p>
        ) : !mayDeploy ? (
          <p className="text-sm">Only the workspace owner confirms a new mandate.</p>
        ) : (
          <>
            <p className="max-w-measure text-sm text-pretty">Confirming binds version 1 to the record above and deploys it to your paper account. It needs your passkey.</p>
            <div className="flex flex-col-reverse gap-3 sm:flex-row sm:items-center sm:justify-between">
              <BackButton onClick={onBack}>Back to the review</BackButton>
              <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={() => setAsking(true)}>
                Confirm and deploy to paper
              </Button>
            </div>
          </>
        )}
      </section>

      <div role="status" aria-live="polite" className="grid gap-3">
        {notice ? <p className="rounded-xl bg-background px-4 py-3 text-sm">{notice}</p> : null}
        {deployment ? (
          <section ref={progress} aria-labelledby="after-confirm" data-slot="after-confirm" className="grid gap-3 rounded-2xl border border-dashed border-muted-foreground px-5 py-4">
            <h2 id="after-confirm" tabIndex={-1} className="text-h3 outline-none">
              After you confirmed
            </h2>
            <p className="text-sm text-muted-foreground">Live progress. It is not part of the record above.</p>
            <Progress deployment={deployment} onBack={onBack} />
          </section>
        ) : null}
      </div>

      {deployment?.phase === "recorded" ? (
        <div className="grid justify-items-start gap-3 border-t border-border/70 pt-6">
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={onStartOver}>
            <ArrowCounterClockwise className="size-4" aria-hidden />
            Set up another agent
          </Button>
        </div>
      ) : null}

      <StepUpDialog
        open={asking}
        action={deployAction(draft)}
        onVerified={() => {
          setAsking(false);
          confirm();
        }}
        onCancel={() => {
          setAsking(false);
          setNotice("Passkey check canceled. Nothing was confirmed or sent.");
        }}
        onFailed={() => {
          setAsking(false);
          setNotice("Passkey check failed. Nothing was confirmed or sent.");
        }}
      />
    </article>
  );
}
