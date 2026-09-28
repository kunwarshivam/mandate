"use client";

import Link from "next/link";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { FixtureTag } from "@/components/domain/placeholders";
import { PositionsTable } from "@/components/domain/positions";
import { quantity } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { positionHref } from "@/lib/screens";
import { EmptyBoard, Panel, Section, WorkspaceGate } from "./common";

function Positions() {
  const { ws, now } = useRuntime();
  if (ws.agents.length === 0) return <EmptyBoard />;
  const marketStale = ws.health.market_data.state !== "ok";
  const holding = ws.agents.filter((a) => a.positions.length > 0);
  const flat = ws.agents.filter((a) => a.positions.length === 0);
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      <PageHeader title="Positions" environment={ws.environment} description="Everything your agents hold on this paper account. Each row opens the position." className="mb-0" />
      {holding.map((agent) => {
        const staleSymbols = marketStale
          ? new Set(agent.positions.map((p) => p.instrument.symbol))
          : new Set(agent.restrictions.filter((r) => r.code === "stale_mark").map((r) => r.symbol ?? ""));
        return (
          <Section
            key={agent.agent_id}
            id={`positions-${agent.agent_id}`}
            title={agent.label}
            action={
              <Link href={`/agents/${agent.agent_id}`} className="text-sm font-semibold text-lapis underline underline-offset-4 hover:decoration-2">
                Open agent
              </Link>
            }
          >
            <Panel>
              <PositionsTable positions={agent.positions} now={now} staleSymbols={staleSymbols} hrefFor={(p) => positionHref(agent.agent_id, p.instrument.asset_id)} />
            </Panel>
          </Section>
        );
      })}
      {flat.length > 0 ? (
        <p className="text-sm text-muted-foreground">
          Holding nothing: {flat.map((a) => a.label).join(", ")}.
        </p>
      ) : null}
      {ws.external_positions.length > 0 ? (
        <Section title="Your own holdings">
          <Panel className="grid gap-2">
            <p className="text-sm text-muted-foreground">No agent manages these. Stop choices leave them alone, except Close everything on this account.</p>
            <ul className="grid gap-1">
              {ws.external_positions.map((p) => (
                <li key={p.instrument.asset_id} className="flex items-baseline gap-2 font-semibold">
                  <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                </li>
              ))}
            </ul>
          </Panel>
        </Section>
      ) : null}
      <FixtureTag className="w-fit" />
    </div>
  );
}

export function PositionsScreen() {
  return (
    <WorkspaceGate>
      <Positions />
    </WorkspaceGate>
  );
}
