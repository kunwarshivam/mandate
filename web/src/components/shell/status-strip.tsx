"use client";

import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { useReducedMotion } from "motion/react";
import { cn } from "@/lib/utils";
import type { HealthState, Workspace } from "@/fixtures/types";
import { clock } from "@/lib/format";
import { Age, StateChip } from "@/components/domain/as-of";
import { FixtureTag } from "@/components/domain/placeholders";

interface Item {
  key: string;
  state: HealthState | "loading";
  text: ReactNode;
}

/** The four feeds, each saying when it was last true: the strip's and the banner's items. */
export function healthItems(ws: Workspace, now: string): Item[] {
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
 * true, and a degraded one is labelled with the chart footer's outlined chip, never an ink fill.
 */
const STATE_LABEL: Record<Item["state"], string | null> = {
  ok: null,
  loading: null,
  stale: "Stale",
  down: "Down",
};

/** How many items end past the strip's visible right edge, the half pixel absorbing subpixel layout. */
export function hiddenToTheRight(itemRights: readonly number[], visibleRight: number): number {
  return itemRights.filter((right) => right > visibleRight + 0.5).length;
}

const isDegraded = (i: Item) => i.state === "stale" || i.state === "down";

/**
 * One line at a fixed height whatever the text says: the market age changes every few seconds, and
 * a strip that rewrapped would move the whole page. Phones scroll it sideways, with a flat "+N" cue
 * at the right edge while items lie past it; wider screens clip each item with an ellipsis, healthy
 * ones first. The cue overlays the strip, so it appearing or going moves nothing.
 *
 * The `banner` variant is the phone's (DEC-207): the same row, words and height, listing only the
 * feeds that are stale or down, and rendered only while one is.
 */
export function StatusStrip({
  ws,
  now,
  className,
  variant = "strip",
}: {
  ws: Workspace;
  now: string;
  className?: string;
  variant?: "strip" | "banner";
}) {
  const banner = variant === "banner";
  const all = healthItems(ws, now);
  const list = banner ? all.filter(isDegraded) : all;
  const degraded = all.filter(isDegraded).length;
  const strip = useRef<HTMLDivElement>(null);
  const [hidden, setHidden] = useState(0);
  const reduceMotion = useReducedMotion();

  useLayoutEffect(() => {
    const el = strip.current;
    if (!el) return;
    const measure = () =>
      setHidden(hiddenToTheRight(Array.from(el.children, (c) => c.getBoundingClientRect().right), el.getBoundingClientRect().right));
    measure();
    const resize = new ResizeObserver(measure);
    for (const node of [el, ...el.children]) resize.observe(node);
    el.addEventListener("scroll", measure, { passive: true });
    return () => {
      resize.disconnect();
      el.removeEventListener("scroll", measure);
    };
  }, [list.length, degraded]);

  return (
    <div className={cn("relative shrink-0", degraded > 0 && "bg-background")}>
      <div
        ref={strip}
        role="region"
        aria-label={banner ? "Feed warning" : "System status"}
        data-slot={banner ? "feed-banner" : "status-strip"}
        data-degraded={degraded > 0 ? "" : undefined}
        tabIndex={0}
        className={cn(
          "flex h-(--status-row) items-center gap-x-5 overflow-x-auto overflow-y-hidden text-caption whitespace-nowrap text-muted-foreground tabular outline-none [scrollbar-width:none] focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset sm:overflow-x-hidden",
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
              {label ? <StateChip className="mr-1.5">{label}</StateChip> : null}
              {i.text}
            </span>
          );
        })}
        {banner ? null : <FixtureTag className="ml-auto" />}
      </div>
      {hidden > 0 ? (
        <button
          type="button"
          tabIndex={-1}
          aria-hidden
          data-slot="status-more"
          onClick={() => strip.current?.scrollBy({ left: strip.current.clientWidth * 0.8, behavior: reduceMotion ? "auto" : "smooth" })}
          className={cn(
            "absolute inset-y-0 right-0 flex min-w-11 items-center justify-center gap-0.5 border-l border-border px-2 text-caption font-semibold text-foreground tabular sm:hidden",
            degraded > 0 ? "bg-background" : "bg-card",
          )}
        >
          +{hidden}
          <ChevronRight className="size-6" />
        </button>
      ) : null}
    </div>
  );
}
