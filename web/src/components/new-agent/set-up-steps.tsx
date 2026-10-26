"use client";

import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import Link from "next/link";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Undo } from "pixelarticons/react/Undo.js";
import { confirmStepUpDigest } from "@/api/confirm";
import { HatchingOwl } from "@/components/domain/owl";
import { KEY } from "@/components/kumo/key";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { StepUpDialog } from "@/components/stop/step-up-dialog";
import type { ContentRef } from "@/fixtures/types";
import { type NewAgent, independentDeployRefusal } from "@/lib/fixture-journey";
import { clock, price, quantity, zoneLabel } from "@/lib/format";
import { type SetUp, type SetUpStep, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";
import type { Draft } from "./draft";
import { confirmAction, confirmedPaths, deployAction, summaryLines } from "./summary";

/**
 * Setting up an agent's last stretch as three steps, each its own record and each shown only once the
 * one before it is recorded (DEC-2401): create the version (`MandateVersionCreated`), confirm it (A5,
 * `MandateConfirmed`, with a passkey bound to the version when it is risk-increasing), and deploy it
 * to paper (B3, `AgentDeployed`, with a passkey). Nothing reads as recorded before the runtime says
 * so, a refusal at any step says what was and was not recorded, and an unknown result never invites
 * the owner to try again.
 */

type StepKey = "version" | "confirm" | "deploy";

const NO_BACKTEST = "None: no backtest of this version has run.";
const PAPER_ONLY = "Paper only: simulated funds. Going live is not available.";
const NOT_SENT = "Not sent: your deployment does not take this step from here yet.";

/** A set-up the owner has confirmed, or may have, and that has not been refused at deployment: its summary can no longer change. */
export function isBound(setUp: SetUp | null): boolean {
  const confirmed = setUp?.confirmed?.phase;
  if (confirmed === undefined || confirmed === "rejected" || confirmed === "undelivered") return false;
  return setUp?.deployed?.phase !== "rejected";
}

/** A step to offer again: refused, or not delivered for a reason another attempt could change. */
const again = (step: SetUpStep) => step.phase === "rejected" || (step.phase === "undelivered" && step.reason !== "not_sent");

/** The step the owner may take next, if any. */
function nextStep(setUp: SetUp | null): StepKey | null {
  if (!setUp || again(setUp.created)) return "version";
  if (setUp.created.phase !== "recorded") return null;
  if (!setUp.confirmed || again(setUp.confirmed)) return "confirm";
  if (setUp.confirmed.phase !== "recorded") return null;
  if (!setUp.deployed || again(setUp.deployed)) return "deploy";
  return null;
}

/** Every line the deploy step shows, in order, as the journal keeps it with the deployment. */
export function deployLines(version: ContentRef, backtestId: string | null): string[] {
  return ["Deploy to paper", "PAPER: simulated funds", `Version 1: ${version}`, `Backtest shown: ${backtestId ?? NO_BACKTEST}`, PAPER_ONLY];
}

/** What the deploy step shows: the version, the backtest shown (or none), and paper as the only place. */
export function DeployDetails({ version, backtestId }: { version: ContentRef; backtestId: string | null }) {
  return (
    <div className="grid gap-2">
      <EnvironmentBadge environment="paper" className="justify-self-start" />
      <dl className="grid gap-x-4 gap-y-1 text-sm sm:grid-cols-[minmax(0,10rem)_minmax(0,1fr)]">
        <dt className="text-muted-foreground">Version 1</dt>
        <dd className="min-w-0 font-mono wrap-anywhere" translate="no" data-slot="deploy-version">
          {version}
        </dd>
        <dt className="text-muted-foreground">Backtest shown</dt>
        <dd className="min-w-0 wrap-anywhere" data-slot="backtest-shown">
          {backtestId ? (
            <Link href={`/backtests/${backtestId}`} className="underline underline-offset-4">
              Backtest{" "}
              <span className="font-mono" translate="no">
                {backtestId}
              </span>
            </Link>
          ) : (
            NO_BACKTEST
          )}
        </dd>
      </dl>
      <p className="text-sm text-pretty">{PAPER_ONLY}</p>
    </div>
  );
}

/** A step refused because the one before it is not recorded stands where that step left things. */
function standing(which: StepKey, reason: string | undefined): string {
  if (reason === "not_created") return STANDING.version.failed;
  if (reason === "not_confirmed") return STANDING.confirm.failed;
  return STANDING[which].failed;
}

/** What exists after each step fails or is unknown: the owner always learns what was and was not recorded. */
const STANDING: Record<StepKey, { failed: string; unknown: string }> = {
  version: {
    failed: "No version was created, nothing was confirmed, and no agent exists.",
    unknown: "Whether version 1 was created is not known yet; nothing was confirmed, and no agent exists.",
  },
  confirm: {
    failed: "Version 1 was created and is not confirmed, and no agent exists.",
    unknown: "Version 1 was created; whether it is confirmed is not known yet, and no agent exists.",
  },
  deploy: {
    failed: "Version 1 was created and confirmed, and no agent exists.",
    unknown: "Version 1 was created and confirmed; whether the agent exists is not known yet. Look in All agents before deploying it again, so the same agent is not created twice.",
  },
};

/** Why nothing was recorded, in words; a server's code stays out of the sentence. */
function undeliveredWords(reason: string | undefined): string {
  switch (reason) {
    case "not_sent":
      return NOT_SENT;
    case "unreachable":
      return "Not delivered: your deployment did not answer.";
    case "step_up_required":
      return "Not recorded: your deployment asked for a passkey check that was not sent.";
    case "step_up_missing":
    case "step_up_stale":
    case "step_up_method":
      return "Not recorded: your deployment did not accept the passkey check.";
    case "passkey_unavailable":
      return "Not sent: this browser can't use a passkey.";
    case "no_passkey":
      return "Not sent: you have no passkey on your deployment yet. Add one, then try again.";
    case "passkey_cooling_off":
      return "Not sent: your passkey is new, and confirms only once its first 24 hours have passed.";
    case "passkey_canceled":
      return "Not sent: the passkey check was canceled or timed out.";
    case "passkey_failed":
      return "Not sent: your passkey's answer could not be read.";
    case "not_created":
      return "Not sent: version 1 was not created, so there is nothing to confirm.";
    case "not_confirmed":
      return "Not sent: version 1 is not confirmed, so there is nothing to deploy.";
    default:
      return "Not recorded: your deployment refused it.";
  }
}

const REFUSED: Record<StepKey, string> = {
  version: "Not created.",
  confirm: "Not confirmed.",
  deploy: "Not deployed.",
};

const SENDING: Record<StepKey, string> = {
  version: "Creating version 1…",
  confirm: "Confirming version 1…",
  deploy: "Deploying it to paper…",
};

const UNTIL: Record<StepKey, string> = {
  version: "Nothing is recorded until the journal has it.",
  confirm: "Nothing is confirmed until the journal records it.",
  deploy: "Nothing is active until the runtime records it.",
};

function stepOf(which: StepKey, setUp: SetUp): SetUpStep | null {
  return which === "version" ? setUp.created : which === "confirm" ? setUp.confirmed : setUp.deployed;
}

/**
 * Over the API, the deployment once the journal records it: the agent's id and its owl. The
 * deployment manager starts its runtime from that record, so nothing here says it runs before its
 * own screen does.
 */
function DeployedOverApi({ setUp }: { setUp: SetUp }) {
  const { now } = useRuntime();
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
        <HatchingOwl seed={setUp.agentId} className="size-16" />
        <div className="grid min-w-0 gap-0.5">
          <p className="font-semibold">Deployed to paper.</p>
          <p className="text-sm text-pretty text-muted-foreground">
            Recorded in the journal at {at(setUp.deployed?.recordedAt ?? now)}, as version 1, for agent{" "}
            <span className="font-mono wrap-anywhere" translate="no" data-slot="deployed-agent">
              {setUp.agentId}
            </span>
            . Its screen shows when it starts and what it does.
          </p>
        </div>
      </div>
      <div className="flex flex-wrap gap-3">
        <Link href={`/agents/${setUp.agentId}`} className={cn("w-full sm:w-auto", KEY)}>
          Open the agent
          <ArrowRight className="size-6" aria-hidden />
        </Link>
      </div>
    </div>
  );
}

/** The deployment, once recorded: the new agent, its owl, and its first check. */
function Deployed({ setUp }: { setUp: SetUp }) {
  const { ws, now, source } = useRuntime();
  if (source === "api") return <DeployedOverApi setUp={setUp} />;
  const agent = ws.agents.find((a) => a.agent_id === setUp.agentId);
  const firstAsk = ws.approvals.find((a) => a.agent_id === setUp.agentId);
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
        <HatchingOwl seed={setUp.agentId} className="size-16" />
        <div className="grid min-w-0 gap-0.5">
          <p className="font-semibold">{agent ? `${agent.label} is running on paper.` : "It is running on paper."}</p>
          <p className="text-sm text-pretty text-muted-foreground">Recorded in the journal at {at(setUp.deployed?.recordedAt ?? now)}, as version 1. This owl is its own, drawn from its ID.</p>
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
}

/** What one step's record says, in words: what was and was not recorded. */
export function StepStatus({ which, setUp }: { which: StepKey; setUp: SetUp }) {
  const step = stepOf(which, setUp);
  if (!step) return null;
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  switch (step.phase) {
    case "sent":
      if (step.awaitingPasskey)
        return (
          <p data-slot="awaiting-passkey">
            Waiting for your passkey. Nothing is sent until your browser&apos;s passkey check is done.
          </p>
        );
      return (
        <p>
          {SENDING[which]} Sent at {at(step.sentAt)}. {UNTIL[which]}
        </p>
      );
    case "recorded":
      if (which === "version") {
        const shown = setUp.created;
        return (
          <div className="grid gap-1">
            <p>Version 1 created at {at(shown.recordedAt ?? shown.sentAt)}. Its validation passed.</p>
            <p className="text-sm">
              Version 1:{" "}
              <span className="font-mono wrap-anywhere" translate="no" data-slot="created-version">
                {setUp.version}
              </span>
            </p>
            {shown.classification ? (
              <p className="text-sm text-pretty" data-slot="classification" data-classification={shown.classification}>
                {classificationWords(shown.classification, shown.base)}
              </p>
            ) : null}
          </div>
        );
      }
      if (which === "confirm") {
        return (
          <p>
            Confirmed by you at {at(step.recordedAt ?? step.sentAt)}
            {setUp.confirmed?.stepUp ? ", with your passkey" : ""}. Version 1 is confirmed; no agent exists until you deploy it.
          </p>
        );
      }
      return <Deployed setUp={setUp} />;
    case "rejected":
      return (
        <div role="alert" className="grid gap-1">
          <p className="font-semibold">
            {REFUSED[which]} {STANDING[which].failed}
          </p>
          <p className="text-pretty" data-rule={step.rule}>
            {step.reason}
          </p>
          <p className="text-pretty">Tell me what to change.</p>
        </div>
      );
    case "undelivered":
      return (
        <p role="alert" className="text-pretty" data-reason={step.reason}>
          {undeliveredWords(step.reason)} {standing(which, step.reason)}
          {step.reason === "unreachable" ? " Try again once it is reachable." : ""}
        </p>
      );
    case "unknown":
      return <p className="text-pretty">The result is unknown; we are checking. {STANDING[which].unknown}</p>;
    default: {
      const unhandled: never = step.phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}

/** Mandate spec §9.2's classification as the version step states it. A first version has no base. */
function classificationWords(classification: NonNullable<SetUp["created"]["classification"]>, base: ContentRef | null): string {
  if (base === null) return "A first version has no earlier version to compare with, so it counts as risk-increasing: confirming it needs your passkey.";
  switch (classification) {
    case "risk_increasing":
      return "Risk-increasing against the version in force: confirming it needs your passkey.";
    case "risk_reducing":
      return "Risk-reducing against the version in force.";
    case "neutral":
      return "Neither increases nor reduces risk against the version in force.";
    default: {
      const unhandled: never = classification;
      throw new Error(`unhandled classification ${String(unhandled)}`);
    }
  }
}

const HEADINGS: Record<StepKey, string> = {
  version: "1. Create the version",
  confirm: "2. Confirm it",
  deploy: "3. Deploy to paper",
};

function StepItem({ which, setUp, children }: { which: StepKey; setUp: SetUp | null; children?: ReactNode }) {
  const step = setUp ? stepOf(which, setUp) : null;
  return (
    <li data-step={which} data-phase={step?.phase} className="grid gap-2 border-l-2 border-border pl-4">
      <h4 tabIndex={-1} className="text-sm font-semibold outline-none">
        {HEADINGS[which]}
      </h4>
      {children}
      {setUp && step ? <StepStatus which={which} setUp={setUp} /> : null}
    </li>
  );
}

/** The three steps under the summary card. */
export function SetUpSteps({
  draft,
  request,
  current,
  blocked,
  setUp,
  onCreated,
  onStartOver,
}: {
  draft: Draft;
  request: NewAgent;
  current: boolean;
  blocked: string | null;
  setUp: SetUp | null;
  onCreated: (setUpId: string) => void;
  onStartOver: () => void;
}) {
  const { source, ws, createVersion, confirmVersion, deployVersion, backtestFor } = useRuntime();
  /**
   * Over the fixtures the step-up dialog stands in for the passkey. Over the API the browser's own
   * WebAuthn prompt is the check (DEC-2128), raised by the runtime once the server has issued the
   * challenge, so no stand-in dialog opens.
   */
  const passkey = source === "fixtures";
  const mayDeploy = useCan("agents.deploy");
  const headingId = useId();
  const root = useRef<HTMLElement>(null);
  /** The step the passkey check is for; it outlives the dialog's close, so focus and words stay with that step. */
  const [asked, setAsked] = useState<"confirm" | "deploy">("confirm");
  const [asking, setAsking] = useState(false);
  const ask = (which: "confirm" | "deploy") => {
    setAsked(which);
    setAsking(true);
  };
  const [notice, setNotice] = useState<string | null>(null);
  /** The step just sent, whose heading takes focus once it is on the page. */
  const focusNext = useRef<StepKey | null>(null);
  /** Over the API the server judges V-047 with every other rule at deployment, from the policy it holds. */
  const independence = source === "api" ? null : independentDeployRefusal(ws);
  const offer = current ? nextStep(setUp) : null;
  const backtestId = setUp ? backtestFor(setUp.version) : null;

  useEffect(() => {
    const which = focusNext.current;
    if (!which) return;
    const item = root.current?.querySelector<HTMLElement>(`[data-step=${which}]`);
    if (!item?.hasAttribute("data-phase")) return;
    focusNext.current = null;
    item.querySelector<HTMLElement>("h4")?.focus({ preventScroll: true });
    item.scrollIntoView({ block: "nearest" });
  }, [setUp]);
  const deployed = setUp?.deployed?.phase ?? null;
  useEffect(() => {
    if (deployed === "recorded") root.current?.querySelector("[data-step=deploy]")?.scrollIntoView({ block: "nearest" });
  }, [deployed]);

  const create = () => {
    focusNext.current = "version";
    setNotice(null);
    onCreated(createVersion(request).id);
  };
  const confirm = (verified: boolean) => {
    if (!setUp) return;
    focusNext.current = "confirm";
    setNotice(null);
    const classification = setUp.created.classification ?? "risk_increasing";
    const stepUp = verified ? confirmStepUpDigest(setUp.version, null, setUp.created.base) : null;
    confirmVersion(setUp.id, { screen: "A5", environment: ws.environment, shown: summaryLines(draft, setUp.version), paths: confirmedPaths(draft), classification }, stepUp);
  };
  const deploy = () => {
    if (!setUp) return;
    focusNext.current = "deploy";
    setNotice(null);
    deployVersion(setUp.id, { screen: "B3", environment: "paper", shown: deployLines(setUp.version, backtestId), backtestId });
  };

  const stepHint = passkey ? "Uses your passkey." : "Your browser asks for your passkey.";
  const action = (which: StepKey): ReactNode => {
    if (offer !== which) return null;
    if (!mayDeploy) return <p className="text-sm">Only the workspace owner can create an agent.</p>;
    if (which === "version" && independence)
      return (
        <p className="font-medium text-pretty" data-slot="blocked" data-rule={independence.rule}>
          {independence.reason}
        </p>
      );
    if (which === "version" && blocked)
      return (
        <p className="font-medium text-pretty" data-slot="blocked">
          {blocked}
        </p>
      );
    const [label, hint, onClick] =
      which === "version"
        ? (["Create the version", "Records this agent as version 1. Nothing runs until you confirm it and deploy it.", create] as const)
        : which === "confirm"
          ? (["Confirm version 1", stepHint, () => (passkey ? ask("confirm") : confirm(false))] as const)
          : (["Deploy to paper", stepHint, () => (passkey ? ask("deploy") : deploy())] as const);
    return (
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={onClick}>
          {label}
        </Button>
        <p className="text-sm text-muted-foreground">{hint}</p>
      </div>
    );
  };

  const created = setUp?.created.phase === "recorded";
  const confirmed = created && setUp?.confirmed?.phase === "recorded";
  const verb = asked === "deploy" ? "deployed" : "confirmed";

  return (
    <div className="grid gap-4">
      <section ref={root} aria-labelledby={headingId} aria-live="polite" data-slot="set-up-steps" className="grid gap-3">
        <h3 id={headingId} className="sr-only">
          Setting it up
        </h3>
        <ol className="grid gap-5">
          <StepItem which="version" setUp={setUp}>
            {action("version")}
          </StepItem>
          {created ? (
            <StepItem which="confirm" setUp={setUp}>
              {action("confirm")}
            </StepItem>
          ) : null}
          {confirmed && setUp ? (
            <StepItem which="deploy" setUp={setUp}>
              <DeployDetails version={setUp.version} backtestId={backtestId} />
              {action("deploy")}
            </StepItem>
          ) : null}
        </ol>
      </section>

      <div role="status" aria-live="polite">
        {notice ? <p className="text-sm">{notice}</p> : null}
      </div>

      {deployed === "recorded" ? (
        <div>
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={onStartOver}>
            <Undo className="size-6" aria-hidden />
            Set up another agent
          </Button>
        </div>
      ) : null}

      <StepUpDialog
        open={asking}
        action={asked === "deploy" ? deployAction(draft) : confirmAction(draft)}
        finalFocus={() => root.current?.querySelector<HTMLElement>(`[data-step=${asked}] h4`) ?? true}
        onVerified={() => {
          const which = asked;
          setAsking(false);
          if (which === "deploy") deploy();
          else confirm(true);
        }}
        onCancel={() => {
          setAsking(false);
          setNotice(`Passkey check canceled. Nothing was ${verb} or sent.`);
        }}
        onFailed={() => {
          setAsking(false);
          setNotice(`Passkey check failed. Nothing was ${verb} or sent.`);
        }}
      />
    </div>
  );
}
