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
    return [{ key: "loading", state: "loading", text: "Connecting to your deployment…" }];
  }
  const md: Item =
    h.market_data.state === "ok"
      ? { key: "market", state: "ok", text: `Market data as of ${clock(h.market_data.as_of)}` }
      : { key: "market", state: h.market_data.state, text: `Market data stale: as of ${clock(h.market_data.as_of)}, ${ago(h.market_data.as_of, now)}` };
  const deployment: Item =
    h.deployment.state === "ok"
      ? { key: "deployment", state: "ok", text: `Deployment answered at ${clock(h.deployment.as_of)}` }
      : { key: "deployment", state: "down", text: `Deployment unreachable since ${clock(h.deployment.as_of)}; agent data hidden` };
  const broker: Item =
    h.broker.state === "ok"
      ? { key: "broker", state: "ok", text: `Broker as of ${clock(h.broker.as_of)}` }
      : { key: "broker", state: h.broker.state, text: `Broker last seen ${clock(h.broker.as_of)}` };
  const relay: Item =
    h.relay.state === "ok"
      ? { key: "relay", state: "ok", text: `Push relay as of ${clock(h.relay.as_of)}` }
      : { key: "relay", state: h.relay.state, text: "Push relay down: no notifications; this screen still updates" };
  return [md, deployment, broker, relay];
}

/**
 * System health carries no meaning colour and no "healthy" dot: every line says when it was last
 * true, and a degraded one is labelled.
 */
const STATE_LABEL: Record<Item["state"], string | null> = {
  ok: null,
  loading: null,
  stale: "Stale",
  down: "Down",
};

export function StatusStrip({ ws, now, className }: { ws: Workspace; now: string; className?: string }) {
  const list = items(ws, now);
  const degraded = list.filter((i) => i.state === "stale" || i.state === "down").length;
  return (
    <div
      role="region"
      aria-label="System status"
      data-slot="status-strip"
      data-degraded={degraded > 0 ? "" : undefined}
      className={cn(
        "flex items-center gap-x-5 gap-y-1 overflow-x-auto py-2 text-caption whitespace-nowrap text-muted-foreground [scrollbar-width:none] sm:flex-wrap sm:overflow-x-visible",
        degraded > 0 && "bg-background",
        className,
      )}
    >
      {degraded > 0 ? (
        <span className="font-semibold text-foreground" data-slot="degraded-count">
          {degraded} degraded
        </span>
      ) : null}
      {list.map((i) => {
        const label = STATE_LABEL[i.state];
        return (
          <span key={i.key} data-state={i.state} className={cn("inline-flex items-center gap-1.5", label && "font-medium text-foreground")}>
            {label ? (
              <span className="rounded-sm bg-foreground px-1.5 text-xs font-semibold text-background" aria-hidden>
                {label}
              </span>
            ) : null}
            {i.text}
          </span>
        );
      })}
      <FixtureTag className="ml-auto" />
    </div>
  );
}
