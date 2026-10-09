"use client";

import type { CSSProperties } from "react";
import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Inbox as InboxIcon } from "pixelarticons/react/Inbox.js";
import { cn } from "@/lib/utils";
import { Deadline } from "@/components/approvals/deadline";
import type { Agent, Approval, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { clock, dateLabel, price, quantity, seconds } from "@/lib/format";
import { APPROVAL_STATUS_LABEL, ruleSentence } from "@/lib/labels";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { agentHref } from "@/lib/screens";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { PAGE_GRID, Section, WorkspaceGate } from "./common";
import { SideRail } from "./side-rail";

/**
 * Open requests sit on the account's pale tint you can act on; resolved ones recede to muted type, with their
 * status as a label. On a phone every request is one hairline row, the open ones marked by the tray.
 */
function Row({ approval, now, label, index }: { approval: Approval; now: string; label: string; index: number }) {
  const open = approval.status === "delivered";
  return (
    <li className={cn("reveal grid", open && "max-lg:border-b max-lg:border-border/70")} style={{ "--i": Math.min(index, 6) } as CSSProperties}>
      <Link
        href={`/approvals/${approval.approval_id}`}
        data-status={approval.status}
        className={cn(
          "press group grid gap-1.5 rounded-2xl px-4 py-4 outline-none focus-visible:ring-3 focus-visible:ring-ring",
          open
            ? "bg-lapis-soft hover:bg-lapis-soft/70 max-lg:-mx-2 max-lg:grid-cols-[1.5rem_minmax(0,1fr)] max-lg:gap-x-3 max-lg:rounded-xl max-lg:bg-transparent max-lg:px-2 max-lg:py-3 max-lg:hover:bg-background"
            : "-mx-4 text-muted-foreground hover:bg-background hover:text-foreground max-lg:-mx-2 max-lg:rounded-xl max-lg:px-2 max-lg:py-3",
        )}
      >
        {open ? <InboxIcon aria-hidden className="row-span-2 size-6 text-lapis lg:hidden" /> : null}
        <span className={cn("flex justify-between gap-x-3 gap-y-1", open ? "items-start" : "flex-wrap items-center")}>
          <span className={cn("min-w-0 font-medium", open && "text-foreground")}>
            {label}: buy <span className="font-mono tabular">{quantity(approval.bound.qty)}</span> {approval.bound.symbol} at a limit of{" "}
            <span className="font-mono tabular">{price(approval.bound.limit)}</span>
          </span>
          {open ? (
            <ArrowRight className="size-6 shrink-0 text-lapis transition-transform duration-(--duration-hover) motion-safe:group-hover:translate-x-0.5" aria-hidden />
          ) : (
            <span className="inline-flex h-6 items-center rounded-md bg-background px-2.5 text-label text-foreground">{APPROVAL_STATUS_LABEL[approval.status]}</span>
          )}
        </span>
        {open ? (
          <Deadline deadline={approval.deadline} now={now} staticOnPhone className="text-muted-foreground" />
        ) : (
          <span className="text-sm">
            {approval.resolution ? `${approval.resolution.at.slice(0, 10) === now.slice(0, 10) ? clock(approval.resolution.at) : dateLabel(approval.resolution.at)}: ${approval.resolution.text}` : null}
          </span>
        )}
      </Link>
    </li>
  );
}

/**
 * Which of an agent's rules send the owner a request, each by its sentence and never its id (critique
 * C-6), and how long a request waits, in words.
 */
export function askSentence(agent: Agent): string {
  const { rules, default: otherwise, approval } = agent.mandate.autonomy;
  const asks = rules.filter((r) => r.then === "ask").map(ruleSentence);
  const which = asks.length === 0 ? "No rule of yours asks you." : `${asks.length === 1 ? "Your rule" : "Your rules"}: ${asks.join("; ")}.`;
  const rest = otherwise === "ask" ? ` Anything no rule covers asks you${asks.length === 0 ? "" : " too"}.` : "";
  return `${which}${rest} A request waits ${seconds(approval.timeout_s)}, then is skipped.`;
}

/**
 * Beside the requests: why they come. Each agent's rules that ask the owner, and the window a request
 * waits, with a way to the mandate that holds them. The rules change only in a new mandate version.
 */
function WhyAsked({ ws }: { ws: Workspace }) {
  return (
    <section aria-labelledby="why-asked-title" data-slot="why-asked" className="grid content-start gap-2">
      <h2 id="why-asked-title" className="text-h3">
        What sends you a request
      </h2>
      <ul className="grid">
        {ws.agents.map((agent) => (
          <li key={agent.agent_id} className="grid gap-1 border-b border-border/70 py-3 last:border-b-0">
            <Link href={agentHref(agent.agent_id, "mandate")} className="w-fit font-semibold underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
              {agent.label}
            </Link>
            <p className="text-sm text-pretty text-muted-foreground">{askSentence(agent)}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Inbox() {
  const { ws, now } = useRuntime();
  const all = ws.approvals.map((a) => approvalAt(a, now));
  const open = all.filter((a) => a.status === "delivered").sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const resolved = all.filter((a) => a.status !== "delivered").sort((a, b) => Date.parse(b.resolution?.at ?? b.deadline) - Date.parse(a.resolution?.at ?? a.deadline));
  const label = (a: Approval) => findAgent(ws, a.agent_id)?.label ?? "An agent";

  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <PageHeader title="Approvals" description="Requests your rules sent to you. If you do nothing, a request is skipped at its deadline." className="mb-0" />
      <div className={PAGE_GRID}>
        <div data-layout="main" className="grid min-w-0 grid-cols-1 content-start gap-(--section-gap)">
          <Section title="Open, by deadline">
            {open.length === 0 ? (
              <p className="text-muted-foreground">Nothing is waiting for you.</p>
            ) : (
              <ul className="grid gap-2 max-lg:gap-0">
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
              <ul className="grid divide-y divide-border/70">
                {resolved.map((a, i) => (
                  <Row key={a.approval_id} approval={a} now={now} label={label(a)} index={open.length + i} />
                ))}
              </ul>
            )}
          </Section>
        </div>
        {ws.agents.length > 0 ? (
          <SideRail className="max-lg:hidden">
            <WhyAsked ws={ws} />
          </SideRail>
        ) : null}
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
