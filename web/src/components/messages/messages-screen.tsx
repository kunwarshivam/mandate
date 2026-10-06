"use client";

import Link from "next/link";
import { ArrowLeft } from "pixelarticons/react/ArrowLeft.js";
import { MessageText } from "pixelarticons/react/MessageText.js";
import { FixtureTag } from "@/components/domain/placeholders";
import { ModeBadge } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import { type PageTab, PageTabs } from "@/components/kumo/page-header/page-header";
import { AgentNotFound } from "@/components/screens/agent-frame";
import { EmptyBoard, WorkspaceGate } from "@/components/screens/common";
import { PAGE_FRAME } from "@/components/shell/frame";
import type { Agent } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { allFeedsOk } from "@/lib/feeds";
import { useRuntime } from "@/lib/mock-runtime";
import { agentHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { ThreadDesk } from "./desk";
import { ThreadList } from "./inbox";
import { ThreadRail } from "./rail";
import { ThreadChat } from "./thread";

export type ThreadView = "chat" | "desk";

/**
 * The panes fill the window under the header, edge to edge (DEC-481). On a desktop: the threads,
 * the open thread, and the agent beside it, divided by hairlines. On a phone: one of the first two.
 */
const MESSAGES_GRID =
  "grid min-h-0 flex-1 grid-cols-1 grid-rows-[minmax(0,1fr)] lg:grid-cols-[19rem_minmax(0,1fr)_17rem] lg:divide-x lg:divide-border/70 xl:grid-cols-[21rem_minmax(0,1fr)_19rem]";

/** A side pane scrolls on its own, and its end clears the dock. A pane is the containing block of what it scrolls, so nothing inside, such as a screen reader's label, lengthens the page. */
const SIDE_PANE = "relative min-h-0 min-w-0 overflow-y-auto overscroll-contain lg:pb-[calc(var(--dock-clearance)+1rem)]";

/** Chat and Desk are pages, each with its own address, so nothing the owner needs hides behind a tab. */
export function threadViews(agentId: string): PageTab[] {
  return [
    { href: `/messages/${agentId}`, label: "Chat" },
    { href: `/messages/${agentId}/desk`, label: "Desk" },
  ];
}

function Thread({ agent, view }: { agent: Agent; view: ThreadView }) {
  return (
    <section aria-labelledby="thread-title" data-slot="thread-pane" className="flex min-h-0 min-w-0 flex-col">
      <header className="flex shrink-0 flex-wrap items-end gap-x-6 border-b border-border/70 px-(--page-x) pt-1.5 lg:px-6 lg:pt-3">
        <div className="flex min-w-0 flex-1 items-center gap-2 pb-1.5 lg:gap-3 lg:pb-3">
          <Link
            href="/messages"
            aria-label="Back to Messages"
            className="-ml-2.5 inline-flex size-11 shrink-0 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring lg:hidden"
          >
            <ArrowLeft aria-hidden className="size-6" />
          </Link>
          <AgentOwl agent={agent} className="size-8 shrink-0 lg:hidden" />
          <div className="grid min-w-0 gap-0.5">
            <h2 id="thread-title" className="flex flex-wrap items-center gap-x-3 gap-y-1 text-h3">
              {agent.label}
              <ModeBadge mode={agent.mode} />
            </h2>
            <p className="truncate text-sm text-muted-foreground" translate="no">
              {agent.mandate.name}
            </p>
          </div>
          <Link
            href={agentHref(agent.agent_id, "overview")}
            className="ml-auto inline-flex h-11 shrink-0 items-center rounded-lg px-3 text-sm font-medium text-lapis outline-none hover:bg-lapis-soft focus-visible:ring-2 focus-visible:ring-ring lg:hidden"
          >
            Open agent
          </Link>
        </div>
        <PageTabs tabs={threadViews(agent.agent_id)} label="Thread views" className="mx-0 px-0 max-lg:basis-full [&>ul]:border-b-0" />
      </header>
      {view === "chat" ? (
        <ThreadChat agent={agent} />
      ) : (
        <div data-slot="desk-pane" className="relative min-h-0 flex-1 overflow-y-auto overscroll-contain px-(--page-x) pt-5 pb-8 lg:px-8 lg:pb-[calc(var(--dock-clearance)+1rem)]">
          <ThreadDesk agent={agent} />
        </div>
      )}
    </section>
  );
}

function NoThreadOpen() {
  return (
    <div data-slot="no-thread" className="grid place-content-center justify-items-center gap-3 px-8 pb-(--dock-clearance) text-center max-lg:hidden lg:col-span-2">
      <MessageText aria-hidden className="size-12 text-muted-foreground" />
      <p className="text-h3">Pick a thread</p>
      <p className="max-w-measure text-pretty text-muted-foreground">
        Each agent&apos;s thread is its journal: orders, requests and mode changes, at the time each was recorded. Ask about it in its own words; the answers come from the
        record.
      </p>
    </div>
  );
}

function Messages({ agentId, view }: { agentId: string | null; view: ThreadView }) {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) {
    return (
      <div className={PAGE_FRAME}>
        <EmptyBoard />
      </div>
    );
  }
  const agent = agentId ? findAgent(ws, agentId) : undefined;
  if (agentId && !agent) {
    return (
      <div className={PAGE_FRAME}>
        <AgentNotFound />
      </div>
    );
  }
  return (
    <div className={MESSAGES_GRID} data-slot="messages">
      {agent ? <h1 className="sr-only lg:hidden">Messages</h1> : null}
      <div data-slot="threads-pane" className={cn(SIDE_PANE, "grid content-start gap-5 px-(--page-x) pt-4 pb-6 lg:px-4", agent && "max-lg:hidden")}>
        <header className="grid gap-1">
          <h1 className="text-h2">Messages</h1>
          <p className="text-sm text-pretty text-muted-foreground">Every agent&apos;s thread. Requests waiting for you come first.</p>
        </header>
        <ThreadList ws={ws} now={now} current={agent?.agent_id ?? null} />
        <FixtureTag className={cn("w-fit", !allFeedsOk(ws) && "lg:hidden")} />
      </div>
      {agent ? <Thread agent={agent} view={view} /> : <NoThreadOpen />}
      {agent ? (
        <div data-slot="rail-pane" className={cn(SIDE_PANE, "px-5 pt-5 max-lg:hidden")}>
          <ThreadRail agent={agent} />
        </div>
      ) : null}
    </div>
  );
}

/** Messages (DEC-479, DEC-481): every agent's thread, read from the journal, with the record answering questions. */
export function MessagesScreen({ agentId = null, view = "chat" }: { agentId?: string | null; view?: ThreadView }) {
  const { ws } = useRuntime();
  if (ws.status !== "ready") {
    return (
      <div className={PAGE_FRAME}>
        <WorkspaceGate>{null}</WorkspaceGate>
      </div>
    );
  }
  return <Messages agentId={agentId} view={view} />;
}
