"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { ArrowRight } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { Deadline } from "@/components/approvals/deadline";
import type { Approval } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { clock, dateLabel, price, quantity } from "@/lib/format";
import { APPROVAL_STATUS_LABEL } from "@/lib/labels";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { Section, WorkspaceGate } from "./common";

/** Open requests are card fields you can act on; resolved ones recede to muted, with their status as a label. */
function Row({ approval, now, label, index }: { approval: Approval; now: string; label: string; index: number }) {
  const open = approval.status === "delivered";
  return (
    <li className="reveal grid" style={{ "--i": Math.min(index, 6) } as CSSProperties}>
      <Link
        href={`/approvals/${approval.approval_id}`}
        data-status={approval.status}
        className={cn("press grid gap-1.5 px-3 py-3 sm:px-4", open ? "bg-card hover:bg-muted" : "bg-muted text-muted-foreground hover:text-foreground")}
      >
        <span className={cn("flex justify-between gap-x-3 gap-y-1", open ? "items-start" : "flex-wrap items-center")}>
          <span className={cn("min-w-0", open ? "font-bold text-foreground" : "font-medium")}>
            {label}: buy <span className="font-mono tabular">{quantity(approval.bound.qty)}</span> {approval.bound.symbol} at a limit of{" "}
            <span className="font-mono tabular">{price(approval.bound.limit)}</span>
          </span>
          {open ? (
            <ArrowRight className="mt-1 size-4 shrink-0" aria-hidden />
          ) : (
            <span className="inline-flex h-6 items-center bg-card px-1.5 label-caps text-foreground">{APPROVAL_STATUS_LABEL[approval.status]}</span>
          )}
        </span>
        {open ? (
          <Deadline deadline={approval.deadline} now={now} className="text-muted-foreground" />
        ) : (
          <span className="text-sm">
            {approval.resolution ? `${approval.resolution.at.slice(0, 10) === now.slice(0, 10) ? clock(approval.resolution.at) : dateLabel(approval.resolution.at)}: ${approval.resolution.text}` : null}
          </span>
        )}
      </Link>
    </li>
  );
}

function Inbox() {
  const { ws, now } = useRuntime();
  const all = ws.approvals.map((a) => approvalAt(a, now));
  const open = all.filter((a) => a.status === "delivered").sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const resolved = all.filter((a) => a.status !== "delivered").sort((a, b) => Date.parse(b.resolution?.at ?? b.deadline) - Date.parse(a.resolution?.at ?? a.deadline));
  const label = (a: Approval) => findAgent(ws, a.agent_id)?.label ?? "An agent";

  return (
    <div className="grid max-w-4xl grid-cols-1 gap-(--section-gap)">
      <PageHeader title="Approvals" environment={ws.environment} description="Requests your rules sent to you. If you do nothing, a request is skipped at its deadline." className="mb-0" />
      <div className="grid grid-cols-1 gap-(--section-gap)">
        <Section title="Open, by deadline">
          {open.length === 0 ? (
            <p className="bg-muted px-3 py-3 text-muted-foreground sm:px-4">Nothing is waiting for you.</p>
          ) : (
            <ul className="grid gap-(--seam)">
              {open.map((a, i) => (
                <Row key={a.approval_id} approval={a} now={now} label={label(a)} index={i} />
              ))}
            </ul>
          )}
        </Section>
        <Section title="Resolved">
          {resolved.length === 0 ? (
            <p className="text-muted-foreground">No resolved requests yet.</p>
          ) : (
            <ul className="grid gap-(--seam)">
              {resolved.map((a, i) => (
                <Row key={a.approval_id} approval={a} now={now} label={label(a)} index={open.length + i} />
              ))}
            </ul>
          )}
        </Section>
      </div>
    </div>
  );
}

export function ApprovalsInboxScreen() {
  return (
    <WorkspaceGate>
      <Inbox />
    </WorkspaceGate>
  );
}
