"use client";

import type { ReactNode } from "react";
import { Octagon } from "@phosphor-icons/react";
import { LinkButton } from "@cloudflare/kumo/components/button";
import { ModeBanner } from "@/components/domain/mode";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { OPEN_STOP_EVENT } from "@/components/shell/stop-control";
import type { Agent } from "@/fixtures/types";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { AGENT_SECTIONS, agentHref } from "@/lib/screens";

export function AgentNotFound() {
  return (
    <section aria-labelledby="missing-title" className="reveal grid max-w-2xl gap-4 pt-6 sm:pt-12">
      <h1 id="missing-title" className="text-h1">
        No agent with this ID
      </h1>
      <p className="max-w-prose text-muted-foreground">This workspace has no agent with that ID. It may belong to another workspace.</p>
      <LinkButton href="/agents" variant="outline" size="lg" className="h-11 w-fit rounded-full px-5">
        See all agents
      </LinkButton>
    </section>
  );
}

/** A record the agent does not have, with the way back to the list it would be in. */
export function RecordNotFound({ title, text, back }: { title: string; text: string; back: { href: string; label: string } }) {
  return (
    <section aria-labelledby="record-missing-title" data-slot="record-missing" className="reveal grid max-w-2xl gap-4 pt-4 sm:pt-8">
      <h2 id="record-missing-title" className="text-h1">
        {title}
      </h2>
      <p className="max-w-prose text-muted-foreground">{text}</p>
      <LinkButton href={back.href} variant="outline" size="lg" className="h-11 w-fit rounded-full px-5">
        {back.label}
      </LinkButton>
    </section>
  );
}

export function useAgent(agentId: string): Agent | undefined {
  return useRuntime().ws.agents.find((a) => a.agent_id === agentId);
}

const TOP_SECTIONS = AGENT_SECTIONS.filter((s) => !s.parent);

/**
 * Every agent screen shares this frame: a title, the paper badge beside it, route tabs, and a Stop
 * scoped to this agent. Sections title it with the owner's label for the agent; a record passes its
 * own title and names the agent underneath. The document title stays generic either way.
 */
export function AgentFrame({ agent, title, description, children }: { agent: Agent; title?: string; description?: ReactNode; children: ReactNode }) {
  const canStop = useCan("stop.open");
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <PageHeader
        title={title ?? agent.label}
        environment={useRuntime().ws.environment}
        description={description ?? agent.mandate.name}
        tabs={TOP_SECTIONS.map((s) => ({ href: agentHref(agent.agent_id, s.key), label: s.label }))}
        tabsLabel="Agent sections"
        className="mb-0"
        actions={
          canStop ? (
            <button
              type="button"
              onClick={() => window.dispatchEvent(new Event(OPEN_STOP_EVENT))}
              aria-haspopup="dialog"
              className="press inline-flex h-11 items-center gap-2 rounded-full border border-ink bg-card pr-5 pl-4 text-sm font-semibold text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              <Octagon className="size-4.5" weight="fill" aria-hidden />
              Stop this agent…
            </button>
          ) : null
        }
      >
        <ModeBanner mode={agent.mode} restrictions={agent.restrictions} showMode={false} />
      </PageHeader>
      {children}
    </div>
  );
}
