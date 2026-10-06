"use client";

import { useState } from "react";
import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Pause } from "pixelarticons/react/Pause.js";
import { Headroom } from "@/components/domain/envelope";
import { ModeBadge } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import { askSentence } from "@/components/screens/approvals-inbox";
import { CommandEntry } from "@/components/stop/command-log";
import type { Agent } from "@/fixtures/types";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { agentHref } from "@/lib/screens";

const PAUSE_KEY =
  "press inline-flex h-11 items-center justify-center gap-1.5 rounded-xl bg-ink pr-4.5 pl-3 font-semibold text-ink-foreground outline-none hover:bg-ink/85 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2";
const OPEN_KEY =
  "press inline-flex h-11 items-center justify-center gap-1 rounded-xl border border-foreground/25 bg-card pr-3 pl-4 font-medium text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";

/** Pause sends at once, as in the Stop sheet: it lowers risk and needs no passkey. Never disabled. */
function PauseNow({ agent }: { agent: Agent }) {
  const { send, commands } = useRuntime();
  const [sent, setSent] = useState<string | null>(null);
  const command = sent ? commands.find((c) => c.id === sent) : undefined;
  const pausable = agent.mode === "normal" || agent.mode === "exits_only";
  return (
    <>
      {pausable && !command ? (
        <button type="button" className={PAUSE_KEY} onClick={() => setSent(send("pause", agent.agent_id).id)}>
          <Pause aria-hidden className="size-6" />
          Pause
        </button>
      ) : null}
      <div role="status" className="grid empty:hidden">
        {command ? <CommandEntry command={command} label={agent.label} /> : null}
      </div>
    </>
  );
}

/**
 * Beside a thread: the agent at a glance. Its limits as distances to each cap, never a result; what
 * sends the owner a request; and Pause, with the agent's own page one step away.
 */
export function ThreadRail({ agent }: { agent: Agent }) {
  const canPause = useCan("stop.pause");
  return (
    <div data-slot="thread-rail" className="grid min-w-0 content-start gap-(--section-gap)">
      <section aria-labelledby="rail-agent" className="grid justify-items-start gap-2">
        <AgentOwl agent={agent} className="size-12" />
        <h2 id="rail-agent" className="text-h3">
          {agent.label}
        </h2>
        <p className="text-sm text-muted-foreground" translate="no">
          {agent.mandate.name}
        </p>
        <ModeBadge mode={agent.mode} />
      </section>

      <Headroom agent={agent} />

      <section aria-labelledby="rail-asks" className="grid gap-1.5">
        <h2 id="rail-asks" className="text-label text-muted-foreground">
          What sends you a request
        </h2>
        <p className="text-sm text-pretty">{askSentence(agent)}</p>
      </section>

      <div className="grid gap-2">
        {canPause ? <PauseNow key={agent.agent_id} agent={agent} /> : null}
        <Link href={agentHref(agent.agent_id, "overview")} className={OPEN_KEY}>
          Open agent
          <ArrowRight aria-hidden className="size-6" />
        </Link>
      </div>
    </div>
  );
}
