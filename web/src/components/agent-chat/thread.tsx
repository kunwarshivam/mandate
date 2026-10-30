"use client";

import { type ReactNode, useRef } from "react";
import type { ChatMessage, ChatTarget, Source } from "@/api/chat";
import { Composer, type ComposerHandle } from "@/components/chat/composer";
import { JumpToLatest, useFollow } from "@/components/chat/follow";
import { Markdown } from "@/components/chat/markdown";
import { clockShort } from "@/lib/format";
import { cn } from "@/lib/utils";
import { CardView } from "./cards";
import { useThread } from "./use-thread";

const THREAD_X = "px-(--page-x) lg:px-8";
const COMPOSER_DOCK = `relative shrink-0 bg-card pt-2 pb-2 ${THREAD_X} lg:pb-[calc(var(--dock-clearance)+0.5rem)]`;

/** What the owner sent, as they formatted it: Markdown, with every line break they typed. */
function OwnerSaid({ text }: { text: string }) {
  return (
    <div data-slot="owner-message" className="ml-auto max-w-[min(85%,44rem)] min-w-0 rounded-3xl bg-background px-4 py-2.5">
      <span className="sr-only">You: </span>
      <Markdown text={text} />
    </div>
  );
}

/** A source as a chip: a URL opens outside, a record or item is named, never followed as model text. */
function Sources({ sources }: { sources: Source[] }) {
  const shown = sources.filter((s) => !s.ref.startsWith("brief:"));
  if (shown.length === 0) return null;
  return (
    <ul aria-label="Sources" className="flex flex-wrap gap-2 pt-1">
      {shown.map((s) => (
        <li key={`${s.ref}:${s.label}`} className="inline-flex min-h-8 items-center rounded-full border border-border bg-card px-3 text-caption text-muted-foreground">
          {/^https:\/\//.test(s.ref) ? (
            <a href={s.ref} target="_blank" rel="noreferrer noopener" className="underline-offset-4 hover:underline">
              {s.label}
            </a>
          ) : (
            <span title={s.ref}>{s.label}</span>
          )}
        </li>
      ))}
    </ul>
  );
}

/** The agent's message: its words as quoted text (links only to `https:`), then its card, then its sources. */
function AgentSaid({ message }: { message: ChatMessage }) {
  return (
    <div data-slot="agent-message" data-proactive={message.proactive || undefined} className="grid max-w-[min(90%,44rem)] min-w-0 justify-items-start gap-1.5">
      <p className="px-1 text-caption text-muted-foreground">
        <time dateTime={message.at}>{clockShort(message.at)}</time>
        {message.proactive ? <span> · first to say</span> : null}
      </p>
      {message.text ? (
        <div className="rounded-2xl rounded-tl-md bg-background px-3.5 py-2 text-pretty lg:px-4 lg:py-2.5">
          <span className="sr-only">Agent: </span>
          <Markdown text={message.text} links="show" />
        </div>
      ) : null}
      {message.card ? <CardView card={message.card} /> : null}
      <Sources sources={message.sources} />
    </div>
  );
}

/**
 * One thread with an agent, or with a setup for a new one: the messages as the journal holds them,
 * newest at the bottom, a composer, and Stop while the agent replies. New agent messages are
 * announced through the log's live region.
 */
export function ChatThread({
  target,
  label,
  placeholder,
  empty,
  disabledNote,
}: {
  target: ChatTarget;
  label: string;
  placeholder: string;
  empty: ReactNode;
  /** Shown under the composer: what a message can do here. */
  disabledNote?: ReactNode;
}) {
  const thread = useThread(target);
  const key = "agent" in target ? `agent:${target.agent}` : `setup:${thread.setupId ?? "new"}`;
  const { scroller, content, away, jump, pin } = useFollow(key);
  const composer = useRef<ComposerHandle>(null);
  const send = (text: string) => {
    pin();
    thread.send(text);
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="chat-thread">
      <div ref={scroller} data-slot="thread-scroll" className="relative min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div ref={content} role="log" aria-live="polite" aria-label={label} className={cn("grid grid-cols-[minmax(0,1fr)] gap-4 pt-4 pb-3 lg:gap-5 lg:pt-6 lg:pb-4", THREAD_X)} data-slot="thread">
          {thread.loaded && thread.messages.length === 0 ? <div className="text-muted-foreground">{empty}</div> : null}
          {thread.messages.map((m) => (m.from === "owner" ? <OwnerSaid key={m.id} text={m.text ?? ""} /> : <AgentSaid key={m.id} message={m} />))}
          {thread.running ? (
            <p className="px-1 text-sm text-muted-foreground" data-slot="agent-working">
              Working on it…
            </p>
          ) : null}
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
        {thread.problem ? (
          <p role="alert" className="pb-2 text-sm text-pretty" data-slot="thread-problem">
            {thread.problem}
          </p>
        ) : null}
        <div className="flex items-end gap-2">
          <Composer ref={composer} label={label} placeholder={placeholder} onSend={send} busy={thread.running} note={disabledNote} className="min-w-0 flex-1" />
          {thread.running && "agent" in target ? (
            <button type="button" onClick={thread.stop} data-slot="stop-reply" className="press mb-1.5 inline-flex h-11 items-center rounded-xl border border-foreground/25 bg-card px-4 font-medium text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring">
              Stop
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
