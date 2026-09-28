"use client";

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import type { HealthState, Workspace } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { Age } from "@/components/domain/as-of";
import { FixtureTag } from "@/components/domain/placeholders";

interface Item {
  key: string;
  state: HealthState | "loading";
  text: ReactNode;
}

function items(ws: Workspace, now: string): Item[] {
  const h = ws.health;
  if (ws.status === "loading") {
    return [{ key: "loading", state: "loading", text: "Connecting to your deployment…" }];
  }
  const md: Item =
    h.market_data.state === "ok"
      ? { key: "market", state: "ok", text: `Market data as of ${clock(h.market_data.as_of)}` }
      : {
          key: "market",
          state: h.market_data.state,
          text: (
            <>
              Market data stale: as of {clock(h.market_data.as_of)}, <Age at={h.market_data.as_of} now={now} />
            </>
          ),
        };
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

/**
 * One line at a fixed height whatever the text says: the market age changes every few seconds, and
 * a strip that rewrapped would move the whole page. Phones scroll it sideways; wider screens clip
 * each item with an ellipsis, healthy ones first.
 */
export function StatusStrip({ ws, now, className }: { ws: Workspace; now: string; className?: string }) {
  const list = items(ws, now);
  const degraded = list.filter((i) => i.state === "stale" || i.state === "down").length;
  return (
    <div
      role="region"
      aria-label="System status"
      data-slot="status-strip"
      data-degraded={degraded > 0 ? "" : undefined}
      tabIndex={0}
      className={cn(
        "flex h-9 shrink-0 items-center gap-x-5 overflow-x-auto overflow-y-hidden text-caption whitespace-nowrap text-muted-foreground tabular outline-none [scrollbar-width:none] focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset sm:overflow-x-hidden",
        degraded > 0 && "bg-background",
        className,
      )}
    >
      {degraded > 0 ? (
        <span className="shrink-0 font-semibold text-foreground" data-slot="degraded-count">
          {degraded} degraded
        </span>
      ) : null}
      {list.map((i) => {
        const label = STATE_LABEL[i.state];
        return (
          <span
            key={i.key}
            data-state={i.state}
            className={cn("shrink-0 sm:min-w-0 sm:truncate", label ? "font-medium text-foreground sm:shrink" : "sm:shrink-[4]")}
          >
            {label ? (
              <span className="mr-1.5 rounded-sm bg-foreground px-1.5 text-xs font-semibold text-background" aria-hidden>
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
