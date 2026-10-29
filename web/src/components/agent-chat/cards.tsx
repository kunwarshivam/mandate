"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import type { Card } from "@/api/chat";
import { confirmStepUpDigest } from "@/api/confirm";
import { KEY } from "@/components/kumo/key";
import { price, quantity } from "@/lib/format";
import { type SetUp, type SetUpStep, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";

const CARD = "grid max-w-[min(100%,32rem)] gap-2 rounded-2xl border border-border bg-card px-4 py-3";
const LABEL = "text-label text-muted-foreground";

function Shell({ kind, title, children }: { kind: string; title: string; children?: ReactNode }) {
  return (
    <section data-slot="chat-card" data-kind={kind} aria-label={title} className={CARD}>
      <p className={LABEL}>{title}</p>
      {children}
    </section>
  );
}

const sideWord = (side: string) => (side === "sell" ? "Sell" : "Buy");

/** A step's state in words, as the runtime reports it; never success before the journal says so. */
function stepWords(step: SetUpStep | null, done: string): string | null {
  if (!step) return null;
  switch (step.phase) {
    case "recorded":
      return done;
    case "sent":
      return step.awaitingPasskey ? "Waiting for your passkey." : "Sent. Waiting for your deployment to record it.";
    case "rejected":
      return `Refused: ${step.reason ?? "your deployment gave no reason"}.`;
    case "undelivered":
      return `Not sent: ${step.reason ?? "your deployment did not answer"}. Nothing was recorded.`;
    case "unknown":
      return "Your deployment took it but has not said what happened. Check Home before trying again.";
    default:
      return null;
  }
}

/**
 * A new agent's draft (`confirm_mandate`): the server created and validated the version; here the
 * owner confirms it and deploys it to paper, through the runtime's own confirm and deploy, with the
 * passkey checks they ask for. The chat never confirms or deploys (DEC-2293 item 5).
 */
export function ConfirmCard({ card }: { card: Extract<Card, { kind: "confirm_mandate" }> }) {
  const { adoptVersion, confirmVersion, deployVersion, setUps, ws } = useRuntime();
  const mayDeploy = useCan("agents.deploy");
  const router = useRouter();
  const [setUpId, setSetUpId] = useState<string | null>(null);
  const adopted = useRef(false);
  useEffect(() => {
    if (adopted.current) return;
    adopted.current = true;
    setSetUpId(adoptVersion(card.version as SetUp["version"]).id);
  }, [adoptVersion, card.version]);
  const setUp = setUps.find((s) => s.id === setUpId) ?? null;
  const shown = [card.name ? `Agent: ${card.name}` : "Agent: unnamed", ...card.lines.map((l) => `${l.label}: ${l.value}`), `Version: ${card.version}`];
  const created = setUp?.created.phase === "recorded";
  const confirmed = setUp?.confirmed?.phase === "recorded";
  const deployed = setUp?.deployed?.phase === "recorded";
  useEffect(() => {
    if (deployed && setUp?.agentId) router.push(`/agents/${setUp.agentId}`);
  }, [deployed, setUp?.agentId, router]);
  const confirm = () => {
    if (!setUp) return;
    const classification = setUp.created.classification ?? "risk_increasing";
    confirmVersion(setUp.id, { screen: "A5", environment: ws.environment, shown, paths: setUp.created.requiredPaths ?? [], classification }, confirmStepUpDigest(setUp.version, null, null));
  };
  const deploy = () => {
    if (!setUp) return;
    deployVersion(setUp.id, { screen: "B3", environment: "paper", shown, backtestId: null });
  };
  return (
    <Shell kind="confirm_mandate" title="Your new agent">
      {card.name ? <p className="text-h3">{card.name}</p> : null}
      <dl className="grid gap-1">
        {card.lines.map((line) => (
          <div key={line.label} className="flex flex-wrap justify-between gap-x-4">
            <dt className="text-sm text-muted-foreground">{line.label}</dt>
            <dd className="text-sm tabular">{line.value}</dd>
          </div>
        ))}
      </dl>
      <p className="text-caption text-muted-foreground">On paper, with simulated money. Nothing runs until you confirm it and deploy it.</p>
      {!mayDeploy ? <p className="text-sm">Only the workspace owner can create an agent.</p> : null}
      {mayDeploy && created && !setUp?.confirmed ? (
        <button type="button" className={cn(KEY, "w-full sm:w-auto")} onClick={confirm}>
          Confirm this agent
        </button>
      ) : null}
      {mayDeploy && confirmed && !setUp?.deployed ? (
        <button type="button" className={cn(KEY, "w-full sm:w-auto")} onClick={deploy}>
          Deploy to paper
        </button>
      ) : null}
      <p role="status" aria-live="polite" className="text-sm">
        {setUp && !created ? (stepWords(setUp.created, "") ?? "Reading the version from your deployment.") : null}
        {stepWords(setUp?.confirmed ?? null, "Confirmed.")} {stepWords(setUp?.deployed ?? null, "Deployed to paper. Opening its thread.")}
      </p>
    </Shell>
  );
}

/** One card, from the server's typed data; a kind this page does not know shows nothing. */
export function CardView({ card }: { card: Card }) {
  switch (card.kind) {
    case "fill":
      return (
        <Shell kind="fill" title="Filled">
          <p className="tabular">
            {sideWord(card.side)} {quantity(card.qty)} {card.symbol} at {price(card.price)}
          </p>
        </Shell>
      );
    case "approval":
      return (
        <Shell kind="approval" title="Needs your OK">
          <p className="tabular">
            {sideWord(card.side)} {quantity(card.qty)} {card.symbol}
            {card.limit ? ` at a limit of ${price(card.limit)}` : ""}
          </p>
          <Link href={`/approvals/${encodeURIComponent(card.approval_id)}`} className={cn(KEY, "w-full sm:w-auto")}>
            Review and answer
          </Link>
        </Shell>
      );
    case "proposal":
      return (
        <Shell kind="proposal" title="An idea">
          <p className="tabular">
            {sideWord(card.side)} {card.qty ? `${quantity(card.qty)} ` : ""}
            {card.symbol}
          </p>
          {card.reason ? <p className="text-sm text-pretty text-muted-foreground">{card.reason}</p> : null}
          <p className="text-caption text-muted-foreground">Your mandate&apos;s autonomy rules decide it; if it needs you, an approval comes next.</p>
        </Shell>
      );
    case "confirm_mandate":
      return <ConfirmCard card={card} />;
    default:
      return null;
  }
}
