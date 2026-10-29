"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { FixtureTag } from "@/components/domain/placeholders";
import type { HomeAgent } from "@/api/chat";
import { ModeBadge } from "@/components/domain/mode";
import { openStop } from "@/components/shell/stop-control";
import { useRuntime } from "@/lib/mock-runtime";
import type { AgentMode } from "@/fixtures/types";
import { DetailsDrawer } from "./details";
import { ChatThread } from "./thread";

const QUIET_KEY =
  "press inline-flex h-11 items-center rounded-xl border border-foreground/25 bg-card px-4 font-medium text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";
const PAUSE_KEY =
  "press inline-flex h-11 items-center rounded-xl bg-ink px-4 font-semibold text-ink-foreground outline-none hover:bg-ink/85 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2";

/** A mode as the chat routes write it, as the badge's: `active` is `normal`. */
export function badgeMode(mode: string): AgentMode | null {
  if (mode === "active" || mode === "normal") return "normal";
  return mode === "paused" || mode === "stopped" || mode === "exits_only" ? mode : null;
}

/** This agent's row on Home: its name and mode, read with the thread. */
function useAgentRow(agentId: string): HomeAgent | null | "absent" {
  const { chat } = useRuntime();
  const [row, setRow] = useState<HomeAgent | null | "absent">(null);
  useEffect(() => {
    let live = true;
    void chat.home().then((answer) => {
      if (!live || !answer.ok) return;
      setRow(answer.value.find((a) => a.agent_id === agentId) ?? "absent");
    });
    return () => {
      live = false;
    };
  }, [chat, agentId]);
  return row;
}

/**
 * The agent (DEC-2293): one thread, with a header that always holds Stop's controls, so pausing and
 * the kill switch never depend on the chat or the model. Details opens the read-only drawer.
 */
export function AgentScreen({ agentId }: { agentId: string }) {
  const row = useAgentRow(agentId);
  const [details, setDetails] = useState(false);
  if (row === "absent") return <p className="px-(--page-x) py-6 text-muted-foreground">This agent isn&apos;t in your workspace.</p>;
  const name = row?.name ?? "Your agent";
  const mode = row ? badgeMode(row.mode) : null;
  const stopped = mode === "stopped";
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="agent-screen">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border/70 px-(--page-x) py-3 lg:px-8" data-slot="agent-header">
        <Link href="/" className="text-sm text-muted-foreground underline-offset-4 hover:underline">
          Home
        </Link>
        <h1 className="min-w-0 text-h2 wrap-anywhere">{name}</h1>
        <FixtureTag scope="workspace" />
        {mode ? <ModeBadge mode={mode} /> : null}
        {stopped ? <span className="text-sm text-muted-foreground">Stopped: questions only</span> : null}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <button type="button" className={QUIET_KEY} onClick={() => setDetails(true)}>
            Details
          </button>
          {stopped ? null : (
            <button type="button" className={PAUSE_KEY} onClick={openStop} data-slot="agent-stop-controls">
              {mode === "paused" ? "Resume or stop" : "Pause or stop"}
            </button>
          )}
        </div>
      </header>
      <ChatThread
        target={{ agent: agentId }}
        label={`Message ${name}`}
        placeholder={stopped ? "Ask a question" : "Ask anything, or tell it what to do"}
        empty={<p>Say hello, ask how it is doing, or tell it what to do. It answers from its own records and acts only within its mandate.</p>}
        disabledNote={stopped ? "Stopped agents answer questions but take no actions." : undefined}
      />
      <DetailsDrawer agentId={agentId} open={details} onOpenChange={setDetails} />
    </div>
  );
}
