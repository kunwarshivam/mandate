"use client";

import { type CSSProperties, type FocusEvent, useRef, useState } from "react";
import { Popover } from "@cloudflare/kumo/components/popover";
import type { Workspace } from "@/fixtures/types";
import { type Direction, age, price } from "@/lib/format";
import { type Quote, oldestFeed } from "@/lib/quotes";
import { cn } from "@/lib/utils";
import { FixtureTag } from "@/components/domain/placeholders";
import { healthItems } from "./status-strip";

const MOVE_WORD: Record<Direction, string> = { gain: "up", loss: "down", flat: "unchanged" };

/**
 * How fresh the screen is: the age of the oldest feed. Hover or keyboard focus opens the four feeds.
 * Focus moves into the list when it opens, so it is not reopened as focus comes back from it.
 */
function Freshness({ ws, now }: { ws: Workspace; now: string }) {
  const [open, setOpen] = useState(false);
  const returning = useRef(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const feeds = useRef<HTMLUListElement>(null);
  const oldest = oldestFeed(ws);
  const feedAge = age(oldest.as_of, now);

  const onFocus = (e: FocusEvent<HTMLButtonElement>) => {
    if (!returning.current && e.currentTarget.matches(":focus-visible")) setOpen(true);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        const active = document.activeElement;
        if (!next) returning.current = active === trigger.current || (feeds.current?.contains(active) ?? false);
        setOpen(next);
      }}
    >
      <Popover.Trigger
        openOnHover
        delay={150}
        render={<button type="button" ref={trigger} />}
        onFocus={onFocus}
        onBlur={() => {
          returning.current = false;
        }}
        data-slot="freshness"
        aria-label={`Live: ${feedAge} since the oldest feed answered`}
        className="flex w-36 shrink-0 items-center gap-1.5 border-r border-border px-(--page-x) text-left font-medium whitespace-nowrap text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        Live<span aria-hidden>·</span>
        <span data-slot="freshness-age" className="tabular">
          {feedAge}
        </span>
      </Popover.Trigger>
      <Popover.Content side="bottom" align="start" sideOffset={6} positionMethod="fixed" className="w-80 max-w-[calc(100vw-2rem)]">
        <Popover.Title className="text-sm font-semibold">Feeds</Popover.Title>
        <ul ref={feeds} data-slot="feeds" className="mt-2 grid gap-1.5 text-sm text-muted-foreground tabular">
          {healthItems(ws, now).map((i) => (
            <li key={i.key} data-feed={i.key} className={cn(i.key === oldest.key && "text-foreground")}>
              {i.text}
            </li>
          ))}
        </ul>
      </Popover.Content>
    </Popover>
  );
}

function Row({ quotes, copy }: { quotes: Quote[]; copy: boolean }) {
  return (
    <ul data-copy={copy ? "" : undefined} aria-hidden={copy || undefined} inert={copy || undefined} className="flex shrink-0 items-center justify-around gap-8 px-4">
      {quotes.map((q) => (
        <li key={q.symbol} data-slot="quote" className="flex items-baseline gap-2 whitespace-nowrap">
          <span className="font-semibold text-foreground">{q.symbol}</span>
          <span className="font-mono text-foreground tabular">{price(q.last)}</span>
          {q.change ? (
            <span data-move={q.direction} className={cn("font-mono font-medium tabular", q.direction === "gain" && "text-gain", q.direction === "loss" && "text-loss")}>
              {q.change}
              <span className="sr-only"> {MOVE_WORD[q.direction]} today</span>
            </span>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

/**
 * The ticker tape under the header, while every feed answers: the last price of each held instrument
 * and its change on the day, never an amount won or lost. It drifts left and holds still under the
 * pointer or focus; with reduced motion it stands still and scrolls by hand. It is the status strip's
 * height, so the strip taking its place when a feed goes stale moves nothing.
 */
export function Ticker({ ws, now, quotes, className }: { ws: Workspace; now: string; quotes: Quote[]; className?: string }) {
  return (
    <div data-slot="ticker" className={cn("glass sticky top-[calc(4rem+1px)] z-20 h-(--status-row) shrink-0 border-b text-caption", className)}>
      {/* Gains and losses need 4.5:1 over anything scrolling under: the veil takes the glass from 72% card to 83%. */}
      <div data-slot="ticker-veil" className="flex min-w-0 flex-1 items-stretch bg-card/40">
        <Freshness ws={ws} now={now} />
        <div
          role="region"
          aria-label="Held instruments, last price and change today"
          tabIndex={0}
          data-slot="ticker-tape"
          style={{ "--ticker-items": quotes.length } as CSSProperties}
          className="flex min-w-0 flex-1 overflow-hidden text-muted-foreground outline-none [scrollbar-width:none] focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        >
          <div data-slot="ticker-track" className="flex w-max shrink-0">
            <Row quotes={quotes} copy={false} />
            <Row quotes={quotes} copy />
          </div>
        </div>
        <div className="flex shrink-0 items-center border-l border-border px-(--page-x)">
          <FixtureTag />
        </div>
      </div>
    </div>
  );
}
