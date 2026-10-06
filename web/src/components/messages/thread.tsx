"use client";

import { Fragment, useRef } from "react";
import { Composer, type ComposerHandle } from "@/components/chat/composer";
import { JumpToLatest, useFollow } from "@/components/chat/follow";
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

/** The thread's gutter, matched by the composer under it so the two share one edge. */
const THREAD_X = "px-(--page-x) lg:px-8";

/** Under the composer: a sliver on a phone, where the tab bar sits below the pane; the dock's clearance on a desktop. */
const COMPOSER_DOCK = `relative shrink-0 bg-card pt-2 pb-2 ${THREAD_X} lg:pb-[calc(var(--dock-clearance)+0.5rem)]`;

/** Between entries: tighter on a phone, where the screen is the conversation's (DEC-482). */
const ENTRY_GAP = "gap-4 lg:gap-5";

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
      <p className="rounded-2xl rounded-tl-md bg-background px-3.5 py-2 text-pretty lg:px-4 lg:py-2.5">{event.text}</p>
    </div>
  );
}

type Entry = { kind: "item"; at: Iso; item: ThreadItem } | { kind: "turn"; at: Iso; turn: Turn };

function entriesOf(items: ThreadItem[], turns: Turn[]): Entry[] {
  const all: Entry[] = [...items.map((item): Entry => ({ kind: "item", at: item.at, item })), ...turns.map((turn): Entry => ({ kind: "turn", at: turn.at, turn }))];
  return all.sort((a, b) => Date.parse(a.at) - Date.parse(b.at) || (a.kind === b.kind ? 0 : a.kind === "item" ? -1 : 1));
}

/**
 * An agent's thread (DEC-479): its journal, oldest first, its requests as cards that open the
 * request, and below them what the owner asked and the record's answers. Asking writes nothing to
 * the journal; only a Pause the owner presses does, and its entry then appears here like any other.
 * The log scrolls on its own above the composer (DEC-481): it opens at the latest entry, follows new
 * ones while the owner is there, stays put once they scroll up to read, and comes back down when
 * they send.
 */
export function ThreadChat({ agent }: { agent: Agent }) {
  const { ws, now } = useRuntime();
  const create = useStartSetup();
  const canPause = useCan("stop.pause");
  const [turns, addTurn] = useTurns(agent.agent_id);
  const composer = useRef<ComposerHandle>(null);
  const { scroller, content, away, jump, pin } = useFollow(agent.agent_id);

  const items = threadItems(ws, agent.agent_id, now);
  const entries = entriesOf(items, turns);
  const groups = byDay(entries, now);
  const waiting = items
    .flatMap((i) => (i.kind === "request" && i.approval.status === "delivered" ? [i.approval] : []))
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline))[0];
  const pausable = canPause && (agent.mode === "normal" || agent.mode === "exits_only");
  const starters = pausable ? [...STARTERS, `Pause ${agent.label}`] : STARTERS;

  const ask = (said: string) => {
    pin();
    addTurn({ id: `turn-${agent.agent_id}-${turns.length}`, at: now, said, reply: interpret(said, { ws, now, agentId: agent.agent_id }) });
    composer.current?.focus();
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="thread-chat">
      {waiting ? <PinnedRequest approval={waiting} /> : null}
      <div ref={scroller} data-slot="thread-scroll" className="relative min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div ref={content} role="log" aria-label={`${agent.label}'s thread`} className={`grid grid-cols-[minmax(0,1fr)] ${ENTRY_GAP} pt-4 pb-3 lg:pt-6 lg:pb-4 ${THREAD_X}`} data-slot="thread">
          {items.length === 0 && turns.length === 0 ? <p className="text-muted-foreground">Nothing is recorded for {agent.label} yet. Its orders, requests and mode changes appear here.</p> : null}
          {groups.map((group) => (
            <section key={group.day} aria-label={group.label} className={`grid grid-cols-[minmax(0,1fr)] ${ENTRY_GAP}`}>
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
        </div>
      </div>

      <div className={COMPOSER_DOCK} data-slot="composer-dock">
        <JumpToLatest
          away={away}
          onJump={() => {
            jump();
            composer.current?.focus();
          }}
        />
        {turns.length === 0 ? <AskChips asks={starters} onAsk={ask} label="Ask about this agent" className={ONE_ROW} /> : null}
        <Composer
          ref={composer}
          label={`Ask about ${agent.label}`}
          placeholder={`Ask about ${agent.label}, or say pause`}
          onSend={ask}
          className={turns.length === 0 ? "pt-2 lg:pt-3" : undefined}
        />
      </div>
    </div>
  );
}
