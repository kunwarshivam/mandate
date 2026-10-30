"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import type { HomeAgent } from "@/api/chat";
import { KEY } from "@/components/kumo/key";
import { ModeBadge } from "@/components/domain/mode";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import { badgeMode } from "./agent-screen";
import { POLL_IDLE_MS } from "./use-thread";

/** Home's agents, read again every [`POLL_IDLE_MS`]; `null` until the first read, a string when it failed. */
function useHome(): HomeAgent[] | string | null {
  const { chat } = useRuntime();
  const [agents, setAgents] = useState<HomeAgent[] | string | null>(null);
  useEffect(() => {
    let live = true;
    const read = () =>
      void chat.home().then((answer) => {
        if (!live) return;
        setAgents(answer.ok ? answer.value : "Your deployment didn't report your agents. Home reads them again in a moment.");
      });
    read();
    const timer = window.setInterval(read, POLL_IDLE_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [chat]);
  return agents;
}

function AgentRow({ agent }: { agent: HomeAgent }) {
  const mode = badgeMode(agent.mode);
  const last = agent.last_message;
  return (
    <li>
      <Link
        href={`/agents/${agent.agent_id}`}
        data-slot="home-agent"
        className="press grid gap-1 rounded-2xl border border-border bg-card px-4 py-3 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring"
      >
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="min-w-0 text-h3 wrap-anywhere">{agent.name}</span>
          {mode ? <ModeBadge mode={mode} /> : null}
          {agent.needs_you ? (
            <span data-slot="needs-you" className="inline-flex h-6 items-center rounded-md bg-lapis-soft px-2 text-label text-lapis">
              Needs you
            </span>
          ) : null}
          {agent.unread > 0 ? (
            <span className="ml-auto inline-flex h-6 min-w-6 items-center justify-center rounded-full bg-ink px-2 text-label text-ink-foreground" aria-label={`${agent.unread} unread`}>
              {agent.unread}
            </span>
          ) : null}
        </span>
        <span className="line-clamp-1 text-sm text-muted-foreground">{last ? `${last.from === "owner" ? "You: " : ""}${last.text ?? "Sent you a card"}` : "No messages yet"}</span>
      </Link>
    </li>
  );
}

/**
 * Home (DEC-2293): your agents as conversations, each with its last message, its mode, and whether
 * it needs you. An approval is never a screen of its own: the agent that needs you says so here and
 * asks in its thread.
 */
export function HomeScreen() {
  const agents = useHome();
  return (
    <div className="mx-auto grid w-full max-w-2xl gap-5" data-slot="home">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-h1">Your agents</h1>
        <Link href="/new" className={cn(KEY, "w-full sm:w-auto")}>
          New agent
        </Link>
      </div>
      {agents === null ? <p className="text-muted-foreground">Reading your agents…</p> : null}
      {typeof agents === "string" ? <p role="alert">{agents}</p> : null}
      {Array.isArray(agents) && agents.length === 0 ? (
        <p className="text-pretty text-muted-foreground">
          No agents yet. Describe one in your own words and it will ask you what it needs.{" "}
          <Link href="/new" className="text-foreground underline underline-offset-4">
            Set one up
          </Link>
          .
        </p>
      ) : null}
      {Array.isArray(agents) && agents.length > 0 ? (
        <ul className="grid gap-3" aria-label="Agents">
          {agents.map((a) => (
            <AgentRow key={a.agent_id} agent={a} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}
