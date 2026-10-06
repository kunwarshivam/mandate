"use client";

import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { useCopilot } from "@/components/copilot/copilot";
import { ModeBadge } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import type { Iso, Workspace } from "@/fixtures/types";
import { type InboxRow, inboxRows, itemLine, listStamp } from "@/lib/messages";
import { cn } from "@/lib/utils";

function Row({ row, now, current }: { row: InboxRow; now: Iso; current: boolean }) {
  const { agent, open, last } = row;
  return (
    <li>
      <Link
        href={`/messages/${agent.agent_id}`}
        aria-current={current ? "page" : undefined}
        data-slot="thread-row"
        data-open={open.length > 0 ? "" : undefined}
        className={cn(
          "press -mx-2 grid grid-cols-[3rem_minmax(0,1fr)] gap-x-3 rounded-2xl px-2 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring",
          current && "bg-background",
        )}
      >
        <AgentOwl agent={agent} still className="row-span-2 size-12" />
        <span className="flex min-w-0 items-baseline justify-between gap-2">
          <span className={cn("truncate", current ? "font-semibold" : "font-medium")}>{agent.label}</span>
          {last ? (
            <time dateTime={last.at} className="shrink-0 font-mono text-caption text-muted-foreground tabular">
              {listStamp(last.at, now)}
            </time>
          ) : null}
        </span>
        <span className="grid min-w-0 gap-1.5">
          <span className="line-clamp-2 text-sm text-pretty text-muted-foreground">{last ? itemLine(last) : "Nothing recorded yet."}</span>
          <span className="flex flex-wrap items-center gap-1.5">
            {open.length > 0 ? (
              <span className="inline-flex h-6 items-center rounded-md bg-lapis-soft px-2.5 text-label text-lapis">
                {open.length === 1 ? "1 request" : `${open.length} requests`}
              </span>
            ) : null}
            {agent.mode === "normal" ? null : <ModeBadge mode={agent.mode} />}
          </span>
        </span>
      </Link>
    </li>
  );
}

/**
 * The threads (DEC-476): agents with a request waiting first, by deadline, then the rest by their
 * latest entry. A row names the agent by its opaque label and quotes no model output.
 */
export function ThreadList({ ws, now, current }: { ws: Workspace; now: Iso; current: string | null }) {
  const { needsYou, earlier } = inboxRows(ws, now);
  const copilot = useCopilot();
  return (
    <nav aria-label="Threads" data-slot="thread-list" className="grid content-start gap-6">
      <button
        type="button"
        onClick={copilot.show}
        aria-expanded={copilot.open}
        aria-controls="owlhead-copilot"
        data-slot="copilot-row"
        className="press -mx-2 grid grid-cols-[3rem_minmax(0,1fr)] items-center gap-x-3 rounded-2xl px-2 py-3 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring"
      >
        <BrandOwl still className="size-12" />
        <span className="grid min-w-0 gap-0.5">
          <span className="font-medium">Owlhead</span>
          <span className="text-sm text-pretty text-muted-foreground">Ask about all your agents, from the record.</span>
        </span>
      </button>
      {needsYou.length > 0 ? (
        <section aria-labelledby="threads-needs-you" className="grid gap-1">
          <h2 id="threads-needs-you" className="text-label text-muted-foreground">
            Needs you
          </h2>
          <ul className="grid">
            {needsYou.map((row) => (
              <Row key={row.agent.agent_id} row={row} now={now} current={row.agent.agent_id === current} />
            ))}
          </ul>
        </section>
      ) : null}
      {earlier.length > 0 ? (
        <section aria-labelledby="threads-earlier" className="grid gap-1">
          <h2 id="threads-earlier" className="text-label text-muted-foreground">
            {needsYou.length > 0 ? "Earlier" : "Threads"}
          </h2>
          <ul className="grid">
            {earlier.map((row) => (
              <Row key={row.agent.agent_id} row={row} now={now} current={row.agent.agent_id === current} />
            ))}
          </ul>
        </section>
      ) : null}
    </nav>
  );
}
