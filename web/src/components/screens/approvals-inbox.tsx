"use client";

import Link from "next/link";
import { cn } from "@/lib/utils";
import { Deadline } from "@/components/approvals/deadline";
import type { Approval } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { clock, dateLabel, price, quantity } from "@/lib/format";
import { APPROVAL_STATUS_LABEL } from "@/lib/labels";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { PageHeader, Section, WorkspaceGate } from "./common";

function Row({ approval, now, label }: { approval: Approval; now: string; label: string }) {
  const open = approval.status === "delivered";
  return (
    <li>
      <Link
        href={`/approvals/${approval.approval_id}`}
        data-status={approval.status}
        className={cn("press grid gap-1.5 rounded-xl border bg-card p-4 hover:border-primary/50", open && "shadow-whisper")}
      >
        <span className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <span className="font-medium">
            {label}: buy <span className="font-mono tabular">{quantity(approval.bound.qty)}</span> {approval.bound.symbol} at a limit of{" "}
            <span className="font-mono tabular">{price(approval.bound.limit)}</span>
          </span>
          <span className="text-caption text-muted-foreground">{APPROVAL_STATUS_LABEL[approval.status]}</span>
        </span>
        {open ? (
          <Deadline deadline={approval.deadline} now={now} className="text-muted-foreground" />
        ) : (
          <span className="text-sm text-muted-foreground">
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
    <div className="grid gap-10">
      <PageHeader title="Approvals" lead="Requests your rules sent to you. If you do nothing, a request is skipped at its deadline." />
      <Section title="Open, by deadline">
        {open.length === 0 ? (
          <p className="text-muted-foreground">Nothing is waiting for you.</p>
        ) : (
          <ul className="grid gap-2">
            {open.map((a) => (
              <Row key={a.approval_id} approval={a} now={now} label={label(a)} />
            ))}
          </ul>
        )}
      </Section>
      <Section title="Resolved">
        {resolved.length === 0 ? (
          <p className="text-muted-foreground">No resolved requests yet.</p>
        ) : (
          <ul className="grid gap-2">
            {resolved.map((a) => (
              <Row key={a.approval_id} approval={a} now={now} label={label(a)} />
            ))}
          </ul>
        )}
      </Section>
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
