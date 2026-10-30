"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import type { Card, ConfirmOrderCard } from "@/api/chat";
import { confirmStepUpDigest } from "@/api/confirm";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { price, quantity, usd } from "@/lib/format";
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

/** Where a tap on the order card stands: nothing yet, sent, recorded, refused, or dismissed here. */
type Tap = { phase: "idle" } | { phase: "sending" } | { phase: "confirmed" } | { phase: "refused"; reason: string; unknown: boolean } | { phase: "dismissed" };

/**
 * The owner's buy the chat prepared and did not send (`confirm_order`, DEC-2859): what would be
 * placed, and nothing placed until the owner taps Confirm, which submits the owner request under
 * their own session. The server reads the order from its own card and derives the request's id from
 * it, so a second tap submits nothing more. Dismiss only hides the buttons here; nothing was sent.
 * A refusal says why, and an answer that was lost is never shown as sent (API-13).
 */
export function ConfirmOrderView({ card, agentId, messageId }: { card: ConfirmOrderCard; agentId: string | null; messageId: string }) {
  const { chat } = useRuntime();
  const [tap, setTap] = useState<Tap>({ phase: "idle" });
  const confirm = () => {
    if (agentId === null || tap.phase === "sending" || tap.phase === "confirmed") return;
    setTap({ phase: "sending" });
    void chat.confirmOrder(agentId, messageId).then((answer) => setTap(answer.ok ? { phase: "confirmed" } : { phase: "refused", reason: answer.reason, unknown: answer.effect === "unknown" }));
  };
  const open = tap.phase === "idle" || (tap.phase === "refused" && tap.unknown);
  return (
    <Shell kind="confirm_order" title="Confirm your order">
      <p className="tabular">
        Buy {quantity(card.qty)} {card.symbol}
      </p>
      <dl className="grid gap-1">
        <div className="flex flex-wrap justify-between gap-x-4">
          <dt className="text-sm text-muted-foreground">Reference price</dt>
          <dd className="text-sm tabular">{card.reference_price ? `${price(card.reference_price)}, last daily close` : "No trusted price yet"}</dd>
        </div>
        <div className="flex flex-wrap justify-between gap-x-4">
          <dt className="text-sm text-muted-foreground">Estimated cost</dt>
          <dd className="text-sm tabular">{card.estimated_cost_usd ? usd(card.estimated_cost_usd) : "Not known yet"}</dd>
        </div>
      </dl>
      {card.note ? <p className="text-caption text-muted-foreground">{card.note}</p> : null}
      {open && agentId !== null ? (
        <div className="grid grid-cols-2 gap-2">
          <button type="button" className={DECISION_KEY} onClick={confirm}>
            Confirm
          </button>
          <button type="button" className={DECISION_KEY} onClick={() => setTap({ phase: "dismissed" })}>
            Dismiss
          </button>
        </div>
      ) : null}
      <p role="status" aria-live="polite" className="text-sm">
        {tap.phase === "sending" ? "Sending your order." : null}
        {tap.phase === "confirmed" ? "Confirmed. The platform sizes, checks and places it; watch for the order card." : null}
        {tap.phase === "refused" && !tap.unknown ? `Refused: ${tap.reason.replaceAll("_", " ")}. Nothing was placed.` : null}
        {tap.phase === "refused" && tap.unknown ? "Your deployment has not said whether it took this. Confirming again sends it at most once." : null}
        {tap.phase === "dismissed" ? "Dismissed. Nothing was sent." : null}
      </p>
    </Shell>
  );
}

/** One card, from the server's typed data; a kind this page does not know shows nothing. */
export function CardView({ card, agentId = null, messageId = "" }: { card: Card; agentId?: string | null; messageId?: string }) {
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
    case "confirm_order":
      return <ConfirmOrderView card={card} agentId={agentId} messageId={messageId} />;
    default:
      return null;
  }
}
