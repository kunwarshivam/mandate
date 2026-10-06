"use client";

import { Fragment, useEffect, useRef } from "react";
import { Composer, type ComposerHandle } from "@/components/chat/composer";
import { AskChips, ONE_ROW, OwnerSaid, RecordReply } from "@/components/chat/record-reply";
import { KIND_LABEL } from "@/components/domain/timeline";
import type { Agent, Iso, TimelineEvent } from "@/fixtures/types";
import { STARTERS, interpret } from "@/lib/ask-record";
import { useStartSetup } from "@/lib/handoff";
import { type ThreadItem, byDay, stamp, threadItems } from "@/lib/messages";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { type Turn, useTurns } from "./conversations";
import { PinnedRequest, RequestCard } from "./request-card";

/** The tab bar's height on a phone and the dock's clearance on a desktop, under the composer. */
export const COMPOSER_DOCK =
  "sticky bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) bg-card px-(--page-x) pt-2 pb-3 lg:bottom-0 lg:mx-0 lg:px-0 lg:pb-[calc(var(--dock-clearance)+1rem)]";

const COMPOSER_NOTE = "Owlhead answers from your record. Nothing happens until you press.";

/** A journal entry in the thread: written by code at the time shown, never in a model's voice. */
function JournalLine({ event, now }: { event: TimelineEvent; now: Iso }) {
  return (
    <div data-slot="journal-line" data-kind={event.kind} className="grid max-w-measure justify-items-start gap-1">
      <p className="flex flex-wrap items-baseline gap-x-2 px-1 text-caption text-muted-foreground">
        <span className="text-label text-foreground">{KIND_LABEL[event.kind]}</span>
        <time dateTime={event.at} className="font-mono tabular">
          {stamp(event.at, now)}
        </time>
      </p>
      <p className="rounded-2xl rounded-tl-md bg-background px-4 py-2.5 text-pretty">{event.text}</p>
    </div>
  );
}

type Entry = { kind: "item"; at: Iso; item: ThreadItem } | { kind: "turn"; at: Iso; turn: Turn };

function entriesOf(items: ThreadItem[], turns: Turn[]): Entry[] {
  const all: Entry[] = [...items.map((item): Entry => ({ kind: "item", at: item.at, item })), ...turns.map((turn): Entry => ({ kind: "turn", at: turn.at, turn }))];
  return all.sort((a, b) => Date.parse(a.at) - Date.parse(b.at) || (a.kind === b.kind ? 0 : a.kind === "item" ? -1 : 1));
}

/**
 * An agent's thread (DEC-476): its journal, oldest first, its requests as cards that open the
 * request, and below them what the owner asked and the record's answers. Asking writes nothing to
 * the journal; only a Pause the owner presses does, and its entry then appears here like any other.
 */
export function ThreadChat({ agent }: { agent: Agent }) {
  const { ws, now } = useRuntime();
  const create = useStartSetup();
  const canPause = useCan("stop.pause");
  const [turns, addTurn] = useTurns(agent.agent_id);
  const composer = useRef<ComposerHandle>(null);
  const end = useRef<HTMLDivElement>(null);
  const counted = useRef(turns.length);

  const items = threadItems(ws, agent.agent_id, now);
  const entries = entriesOf(items, turns);
  const groups = byDay(entries, now);
  const waiting = items
    .flatMap((i) => (i.kind === "request" && i.approval.status === "delivered" ? [i.approval] : []))
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline))[0];
  const pausable = canPause && (agent.mode === "normal" || agent.mode === "exits_only");
  const starters = pausable ? [...STARTERS, `Pause ${agent.label}`] : STARTERS;

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [agent.agent_id]);
  useEffect(() => {
    if (turns.length === counted.current) return;
    counted.current = turns.length;
    const behavior: ScrollBehavior = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    end.current?.scrollIntoView({ block: "end", behavior });
  }, [turns.length]);

  const ask = (said: string) => {
    addTurn({ id: `turn-${agent.agent_id}-${turns.length}`, at: now, said, reply: interpret(said, { ws, now, agentId: agent.agent_id }) });
    composer.current?.focus();
  };

  return (
    <div className="grid grid-cols-[minmax(0,1fr)] content-start gap-6" data-slot="thread-chat">
      {waiting ? <PinnedRequest approval={waiting} /> : null}
      <div role="log" aria-label={`${agent.label}'s thread`} className="grid gap-5" data-slot="thread">
        {items.length === 0 && turns.length === 0 ? <p className="text-muted-foreground">Nothing is recorded for {agent.label} yet. Its orders, requests and mode changes appear here.</p> : null}
        {groups.map((group) => (
          <section key={group.day} aria-label={group.label} className="grid gap-5">
            <h3 className="justify-self-center text-label text-muted-foreground">{group.label}</h3>
            {group.items.map((entry) => {
              if (entry.kind === "turn") {
                return (
                  <Fragment key={entry.turn.id}>
                    <OwnerSaid text={entry.turn.said} />
                    <RecordReply reply={entry.turn.reply} onAsk={ask} onCreate={create} />
                  </Fragment>
                );
              }
              const { item } = entry;
              return item.kind === "event" ? <JournalLine key={item.id} event={item.event} now={now} /> : <RequestCard key={item.id} approval={item.approval} now={now} />;
            })}
          </section>
        ))}
        <div ref={end} aria-hidden />
      </div>

      <div className={COMPOSER_DOCK}>
        {turns.length === 0 ? <AskChips asks={starters} onAsk={ask} label="Ask about this agent" className={ONE_ROW} /> : null}
        <Composer
          ref={composer}
          label={`Ask about ${agent.label}`}
          placeholder={`Ask about ${agent.label}, or say pause`}
          onSend={ask}
          className={turns.length === 0 ? "pt-3" : undefined}
          note={COMPOSER_NOTE}
        />
      </div>
    </div>
  );
}
