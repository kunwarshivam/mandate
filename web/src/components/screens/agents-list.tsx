"use client";

import { LinkButton } from "@cloudflare/kumo/components/button";
import { useRuntime } from "@/lib/mock-runtime";
import { BEVEL } from "@/components/kumo/bevel";
import { AgentCard, PaperPnlNote } from "./agent-card";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { EmptyBoard, WorkspaceGate } from "./common";

function Agents() {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) return <EmptyBoard />;
  const marketStale = ws.health.market_data.state !== "ok";
  return (
    <div className="grid">
      <PageHeader
        title="Agents"
        environment={ws.environment}
        description="Each agent trades on paper within its own confirmed mandate."
        actions={
          <LinkButton href="/agents/new" variant="outline" size="lg" className={`h-11 px-5 ${BEVEL}`}>
            Describe an agent
          </LinkButton>
        }
      />
      <PaperPnlNote className="pb-2" />
      <ul className="grid">
        {ws.agents.map((agent, i) => (
          <li key={agent.agent_id} className="grid">
            <AgentCard agent={agent} now={now} marketStale={marketStale} index={i} heading="h2" />
          </li>
        ))}
      </ul>
    </div>
  );
}

export function AgentsListScreen() {
  return (
    <WorkspaceGate>
      <Agents />
    </WorkspaceGate>
  );
}
