"use client";

import { type CSSProperties, type FocusEvent, useRef, useState } from "react";
import Link from "next/link";
import { Popover } from "@cloudflare/kumo/components/popover";
import { Check, Clock, HandPalm, type Icon as PhosphorIcon, MinusCircle, PauseCircle, Prohibit, Tray } from "@phosphor-icons/react";
import type { Workspace } from "@/fixtures/types";
import { age, clock } from "@/lib/format";
import { oldestFeed } from "@/lib/feeds";
import { cn } from "@/lib/utils";
import { WIRE_STATE_LABEL, type WireItem, type WireState } from "@/lib/wire";
import { FixtureTag } from "@/components/domain/placeholders";
import { healthItems } from "./status-strip";

const STATE_ICON: Record<WireState, PhosphorIcon> = {
  waiting_for_you: Tray,
  blocked: MinusCircle,
  held: HandPalm,
  waiting: Clock,
  done: Check,
  paused: PauseCircle,
  stopped: Prohibit,
};

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

function Row({ items, copy }: { items: WireItem[]; copy: boolean }) {
  return (
    <ul data-copy={copy ? "" : undefined} aria-hidden={copy || undefined} inert={copy || undefined} className="flex shrink-0 items-stretch gap-2 px-2">
      {items.map((item) => {
        const Icon = STATE_ICON[item.state];
        return (
          <li key={item.key} data-slot="wire-item" data-state={item.state} className="flex shrink-0">
            <Link
              href={item.href}
              className="flex items-center gap-2 rounded-md px-2 whitespace-nowrap outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            >
              <time dateTime={item.at} className="font-mono text-muted-foreground tabular">
                {clock(item.at).slice(0, 5)}
              </time>
              <span className="text-foreground">
                <span className="font-semibold">{item.agent}</span> {item.phrase}
              </span>
              <span data-slot="wire-state" className="inline-flex items-center gap-1 text-muted-foreground">
                <span className="sr-only">, </span>
                <Icon className="size-3.5 shrink-0" aria-hidden />
                {WIRE_STATE_LABEL[item.state]}
              </span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

/**
 * The agent wire under the header, while every feed answers: what the agents are doing, requests
 * waiting for you first. Ink and muted only, words beside every glyph, and no amount won or lost. It
 * drifts left and holds still under the pointer or focus; with reduced motion it stands still and
 * scrolls by hand. It is the status strip's height, so the strip taking its place moves nothing.
 */
export function Wire({ ws, now, items, className }: { ws: Workspace; now: string; items: WireItem[]; className?: string }) {
  return (
    <div data-slot="wire" className={cn("glass sticky top-[calc(4rem+1px)] z-20 h-(--status-row) shrink-0 items-stretch border-b text-caption", className)}>
      <Freshness ws={ws} now={now} />
      <div
        role="region"
        aria-label="Agent activity"
        data-slot="wire-tape"
        style={{ "--wire-items": items.length } as CSSProperties}
        className="flex min-w-0 flex-1 overflow-hidden text-muted-foreground [scrollbar-width:none]"
      >
        {items.length === 0 ? (
          <p data-slot="wire-empty" className="flex items-center px-4 text-muted-foreground">
            No agent activity yet today
          </p>
        ) : (
          <div data-slot="wire-track" className="flex w-max shrink-0 py-1">
            <Row items={items} copy={false} />
            <Row items={items} copy />
          </div>
        )}
      </div>
      <div className="flex shrink-0 items-center border-l border-border px-(--page-x)">
        <FixtureTag />
      </div>
    </div>
  );
}
