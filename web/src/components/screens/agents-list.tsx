"use client";

import Link from "next/link";
import { Button } from "@/components/ui/button";
import { useRuntime } from "@/lib/mock-runtime";
import { AgentCard } from "./agent-card";
import { PageHeader, WorkspaceGate } from "./common";

function Agents() {
  const { ws, now } = useRuntime();
  const marketStale = ws.health.market_data.state !== "ok";
  return (
    <div className="grid gap-6">
      <PageHeader title="Agents" lead="Each agent trades on paper within its own confirmed mandate.">
        <Button asChild variant="outline" size="lg" className="press h-10 px-4">
          <Link href="/agents/new">Describe an agent</Link>
        </Button>
      </PageHeader>
      {ws.agents.length === 0 ? (
        <p className="text-muted-foreground">No agents yet.</p>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {ws.agents.map((agent) => (
            <AgentCard key={agent.agent_id} agent={agent} now={now} marketStale={marketStale} />
          ))}
        </div>
      )}
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
