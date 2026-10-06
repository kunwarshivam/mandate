"use client";

import Link from "next/link";
import { ArrowLeft } from "pixelarticons/react/ArrowLeft.js";
import { MessageText } from "pixelarticons/react/MessageText.js";
import { ModeBadge } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import { PageHeader, type PageTab, PageTabs } from "@/components/kumo/page-header/page-header";
import { AgentNotFound } from "@/components/screens/agent-frame";
import { EmptyBoard, WorkspaceGate } from "@/components/screens/common";
import type { Agent } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { useRuntime } from "@/lib/mock-runtime";
import { agentHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { ThreadDesk } from "./desk";
import { ThreadList } from "./inbox";
import { ThreadRail } from "./rail";
import { ThreadChat } from "./thread";

export type ThreadView = "chat" | "desk";

/** On a desktop: the threads, the open thread, and the agent beside it. On a phone: one of the first two. */
const MESSAGES_GRID = "grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[17rem_minmax(0,1fr)_16rem] lg:gap-x-10 xl:grid-cols-[19rem_minmax(0,1fr)_18rem] xl:gap-x-14";

/** The threads and the rail stay in view while a thread scrolls, each scrolling on its own when taller than the window. */
const SIDE_COLUMN = "min-w-0 lg:sticky lg:top-24 lg:max-h-[calc(100dvh-var(--dock-clearance)-8rem)] lg:self-start lg:overflow-y-auto lg:px-2";

/** Chat and Desk are pages, each with its own address, so nothing the owner needs hides behind a tab. */
export function threadViews(agentId: string): PageTab[] {
  return [
    { href: `/messages/${agentId}`, label: "Chat" },
    { href: `/messages/${agentId}/desk`, label: "Desk" },
  ];
}

function Thread({ agent, view }: { agent: Agent; view: ThreadView }) {
  return (
    <section aria-labelledby="thread-title" data-slot="thread-pane" className="grid min-w-0 content-start gap-5">
      <header className="grid gap-4">
        <Link
          href="/messages"
          className="-ml-2 inline-flex h-11 w-fit items-center gap-1.5 rounded-lg px-2 text-sm text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring lg:hidden"
        >
          <ArrowLeft aria-hidden className="size-6" />
          Messages
        </Link>
        <div className="flex min-w-0 items-center gap-3">
          <AgentOwl agent={agent} className="size-12 lg:hidden" />
          <div className="grid min-w-0 gap-0.5">
            <h2 id="thread-title" className="flex flex-wrap items-center gap-x-3 gap-y-1 text-h2">
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
        <PageTabs tabs={threadViews(agent.agent_id)} label="Thread views" />
      </header>
      {view === "chat" ? <ThreadChat agent={agent} /> : <ThreadDesk agent={agent} />}
    </section>
  );
}

function NoThreadOpen() {
  return (
    <div data-slot="no-thread" className="grid content-start justify-items-start gap-3 pt-2 max-lg:hidden">
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
  if (ws.agents.length === 0) return <EmptyBoard />;
  const agent = agentId ? findAgent(ws, agentId) : undefined;
  if (agentId && !agent) return <AgentNotFound />;
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <PageHeader
        title="Messages"
        environment={ws.environment}
        description="Each agent's thread, from the journal. Requests waiting for you come first."
        className={cn("mb-0", agent && "max-lg:sr-only")}
      />
      <div className={MESSAGES_GRID} data-slot="messages">
        <div className={cn(SIDE_COLUMN, agent && "max-lg:hidden")}>
          <ThreadList ws={ws} now={now} current={agent?.agent_id ?? null} />
        </div>
        {agent ? <Thread agent={agent} view={view} /> : <NoThreadOpen />}
        {agent ? (
          <div className={cn(SIDE_COLUMN, "max-lg:hidden")}>
            <ThreadRail agent={agent} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** Messages (DEC-479): every agent's thread, read from the journal, with the record answering questions. */
export function MessagesScreen({ agentId = null, view = "chat" }: { agentId?: string | null; view?: ThreadView }) {
  return (
    <WorkspaceGate>
      <Messages agentId={agentId} view={view} />
    </WorkspaceGate>
  );
}
