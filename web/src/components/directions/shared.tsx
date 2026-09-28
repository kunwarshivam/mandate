"use client";

import { createContext, useContext, useState, type CSSProperties, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";
import type { Approval } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { type Dec, dec, mul } from "@/lib/decimal";
import { clock, direction, directionWord, signedUsd, usd } from "@/lib/format";
import { APPROVAL_STATUS_LABEL } from "@/lib/labels";
import { type ApprovalResponse, approvalAt, useRuntime } from "@/lib/mock-runtime";
import type { Motion } from "./directions";

const MotionContext = createContext<Motion>("snap");

export function MotionPersonality({ motion: value, children }: { motion: Motion; children: ReactNode }) {
  return <MotionContext.Provider value={value}>{children}</MotionContext.Provider>;
}

/** `--i` for the reveal stagger. */
export function stagger(i: number): CSSProperties {
  return { "--i": i } as CSSProperties;
}

/**
 * A changed figure swaps whole; nothing counts through values that were never true. How it swaps is
 * the direction's motion personality: Vernier snaps and leaves a decaying mark, Placard rolls the
 * new figure up, Keel cross-softens.
 */
export function Figure({ value, className }: { value: string; className?: string }) {
  const personality = useContext(MotionContext);
  const [initial] = useState(value);
  if (personality === "snap") {
    return (
      <span className={cn("relative inline-block", className)}>
        {value}
        {value === initial ? null : <span key={value} aria-hidden className="d-snap-mark absolute inset-x-0 -bottom-0.5 h-0.5 bg-(--signal)" />}
      </span>
    );
  }
  const roll = personality === "roll";
  return (
    <span className={cn("relative inline-grid overflow-hidden", className)}>
      <span className="sr-only">{value}</span>
      <AnimatePresence initial={false} mode="popLayout">
        <motion.span
          key={value}
          aria-hidden
          className="[grid-area:1/1]"
          initial={roll ? { opacity: 0, transform: "translateY(60%)" } : { opacity: 0, filter: "blur(3px)" }}
          animate={roll ? { opacity: 1, transform: "translateY(0%)" } : { opacity: 1, filter: "blur(0px)" }}
          exit={roll ? { opacity: 0, transform: "translateY(-60%)" } : { opacity: 0, filter: "blur(3px)" }}
          transition={roll ? { duration: 0.2, ease: [0.23, 1, 0.32, 1] } : { type: "spring", duration: 0.25, bounce: 0 }}
        >
          {value}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

/** A gain or loss: sign, colour, and the word, so colour never carries the meaning alone. */
export function Signed({ value, className, word = true }: { value: string | Dec; className?: string; word?: boolean }) {
  const d = direction(value);
  return (
    <span data-direction-of-change={d} className={cn("inline-flex items-baseline gap-1.5", d === "gain" && "text-lagoon-text", d === "loss" && "text-rose-text", className)}>
      <Figure value={signedUsd(value)} className="font-mono tabular" />
      {word ? <span className="font-sans text-[max(0.8em,0.75rem)] font-medium">{directionWord(value)}</span> : <span className="sr-only">{directionWord(value)}</span>}
    </span>
  );
}

export function useDashboard() {
  const { ws, now } = useRuntime();
  const open = ws.approvals
    .map((a) => approvalAt(a, now))
    .filter((a) => a.status === "delivered")
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  return {
    ws,
    now,
    open,
    marketStale: ws.health.market_data.state !== "ok",
    running: ws.agents.filter((a) => a.mode !== "stopped").length,
    positions: ws.agents.flatMap((a) => a.positions.map((p) => ({ agent: a, p }))),
  };
}

export function useApproval(approvalId: string) {
  const { ws, now, responses, respond } = useRuntime();
  const raw = ws.approvals.find((a) => a.approval_id === approvalId);
  if (!raw) return null;
  const approval = approvalAt(raw, now);
  return {
    approval,
    now,
    agent: findAgent(ws, approval.agent_id),
    response: responses[approval.approval_id] as ApprovalResponse | undefined,
    respond: (r: "approve" | "skip") => respond(approval.approval_id, r),
    orderValue: usd(mul(dec(approval.bound.qty), dec(approval.bound.limit))),
    open: approval.status === "delivered",
  };
}

/** Nothing is optimistic: "sent" until the runtime records it, and the default still applies. */
export function ResponseText({ approval, response }: { approval: Approval; response: ApprovalResponse }) {
  if (response.phase === "sent") {
    return (
      <span data-phase="sent">
        {response.response === "approve" ? "Your approval was sent." : "Your skip was sent."} Not recorded yet; if the runtime does not record it before the deadline (
        {clock(approval.deadline)}), the action is skipped.
      </span>
    );
  }
  if (approval.status === "delivered") {
    return (
      <span data-phase="recorded">
        Recorded at {clock(response.recordedAt ?? approval.deadline)}: you approved.{" "}
        {approval.approvals_so_far.length < approval.approvers_required
          ? `Waiting for ${approval.approvers_required - approval.approvals_so_far.length} more approver before the deadline.`
          : "The gate is re-running its checks on the bound quantity, limit price, and mandate version."}
      </span>
    );
  }
  return null;
}

export function outcomeOf(approval: Approval): { title: string; text: string } | null {
  if (approval.status === "delivered" || !approval.resolution) return null;
  return { title: APPROVAL_STATUS_LABEL[approval.status], text: `${clock(approval.resolution.at)}: ${approval.resolution.text}` };
}

/** The response stages, in order; the current one is where the record stands. */
export const STAGES = ["Sent", "Recorded", "Gate re-run"] as const;

export function stageOf(approval: Approval, response: ApprovalResponse | undefined): number {
  if (!response) return -1;
  if (response.phase === "sent") return 0;
  return approval.status === "delivered" ? 1 : 2;
}
