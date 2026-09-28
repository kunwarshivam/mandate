"use client";

import { cn } from "@/lib/utils";
import type { HealthState, Workspace } from "@/fixtures/types";
import { ago, clock } from "@/lib/format";
import { FixtureTag } from "@/components/domain/placeholders";

interface Item {
  key: string;
  state: HealthState | "loading";
  text: string;
}

function items(ws: Workspace, now: string): Item[] {
  const h = ws.health;
  if (ws.status === "loading") {
    return [{ key: "loading", state: "loading", text: "Connecting to your deployment" }];
  }
  const md: Item =
    h.market_data.state === "ok"
      ? { key: "market", state: "ok", text: `Market data as of ${clock(h.market_data.as_of)}` }
      : { key: "market", state: h.market_data.state, text: `Market data stale: as of ${clock(h.market_data.as_of)}, ${ago(h.market_data.as_of, now)}` };
  const deployment: Item =
    h.deployment.state === "ok"
      ? { key: "deployment", state: "ok", text: "Deployment reachable" }
      : { key: "deployment", state: "down", text: `Deployment unreachable since ${clock(h.deployment.as_of)}; agent data hidden` };
  const broker: Item =
    h.broker.state === "ok"
      ? { key: "broker", state: "ok", text: "Broker connected" }
      : { key: "broker", state: h.broker.state, text: `Broker last seen ${clock(h.broker.as_of)}` };
  const relay: Item =
    h.relay.state === "ok"
      ? { key: "relay", state: "ok", text: "Push relay working" }
      : { key: "relay", state: h.relay.state, text: "Push relay down: no notifications; this screen still updates" };
  return [md, deployment, broker, relay];
}

const DOT: Record<Item["state"], string> = {
  ok: "bg-lagoon",
  stale: "bg-persimmon",
  down: "bg-rose",
  loading: "bg-muted-foreground/40",
};

export function StatusStrip({ ws, now, className }: { ws: Workspace; now: string; className?: string }) {
  const list = items(ws, now);
  const degraded = list.filter((i) => i.state === "stale" || i.state === "down").length;
  return (
    <div
      role="region"
      aria-label="System status"
      data-slot="status-strip"
      className={cn("flex items-center gap-x-5 gap-y-1 overflow-x-auto px-4 py-2 text-caption whitespace-nowrap text-muted-foreground [scrollbar-width:none]", className)}
    >
      {degraded > 0 ? (
        <span className="font-semibold text-foreground" data-slot="degraded-count">
          {degraded} degraded
        </span>
      ) : null}
      {list.map((i) => (
        <span key={i.key} data-state={i.state} className={cn("inline-flex items-center gap-1.5", i.state !== "ok" && i.state !== "loading" && "text-foreground")}>
          <span className={cn("size-1.5 shrink-0 rounded-full", DOT[i.state])} aria-hidden />
          {i.text}
        </span>
      ))}
      <FixtureTag className="ml-auto" />
    </div>
  );
}
