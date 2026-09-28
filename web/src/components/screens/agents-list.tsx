"use client";

import Link from "next/link";
import { Button } from "@/components/ui/button";
import { useRuntime } from "@/lib/mock-runtime";
import { AgentCard } from "./agent-card";
import { EmptyBoard, PageHeader, WorkspaceGate } from "./common";

function Agents() {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) return <EmptyBoard />;
  const marketStale = ws.health.market_data.state !== "ok";
  return (
    <div className="grid">
      <PageHeader title="Agents" lead="Each agent trades on paper within its own confirmed mandate.">
        <Button asChild variant="outline" size="lg">
          <Link href="/agents/new">Describe an agent</Link>
        </Button>
      </PageHeader>
      <ul className="grid gap-(--block-gap)">
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
