"use client";

import { type FormEvent, type KeyboardEvent, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowUp } from "@phosphor-icons/react";
import { KEY } from "@/components/kumo/key";
import type { NewAgent } from "@/lib/fixture-journey";
import type { Deployment } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import { type Conversation, type Entry, currentSummary, nextStep } from "./conversation";
import { type Draft, MODELS, type ModelId } from "./draft";
import { StepHeading } from "./steps";
import { Summary, isConfirmed } from "./summary";

/** The sticky header's height plus a margin, matching the page's `scroll-padding-top` of 5rem. */
const HEADER_CLEARANCE_PX = 80;

/** The platform's side of the conversation: plain text, as in any chat. */
function Platform({ children, slot }: { children: ReactNode; slot?: string }) {
  return (
    <div data-slot={slot ?? "platform"} className="grid max-w-measure gap-2 text-pretty">
      <span className="sr-only">Owlhead: </span>
      {children}
    </div>
  );
}

function Owner({ text }: { text: string }) {
  return (
    <div data-slot="owner-message" className="ml-auto max-w-[85%] rounded-3xl bg-background px-4 py-2.5 text-pretty whitespace-pre-wrap wrap-anywhere">
      <span className="sr-only">You: </span>
      {text}
    </div>
  );
}

const CHOICE =
  "press grid content-start gap-0.5 rounded-2xl border border-foreground/25 bg-card px-4 py-3 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-55 min-h-11";

function ModelChoices({ busy, onChoose }: { busy: boolean; onChoose: (id: ModelId) => void }) {
  return (
    <div role="group" aria-label="Models" className="grid gap-2 sm:grid-cols-2" data-slot="model-choices">
      {MODELS.map((m) => (
        <button key={m.id} type="button" className={CHOICE} disabled={busy} onClick={() => onChoose(m.id)} data-slot="model-choice">
          <span className="font-semibold">{m.name}</span>
          <span className="text-sm text-pretty text-muted-foreground">{m.what}</span>
        </button>
      ))}
    </div>
  );
}

export interface ChatActions {
  onChoose: (id: ModelId) => void;
  onRetry: (messageId: string) => void;
  onWithoutModel: (messageId: string) => void;
  onSent: (deploymentId: string, revision: number) => void;
  onStartOver: () => void;
}

/** What the summary shown at a revision needs: the draft and request as they stand, while that revision is current or was sent. */
export interface Creation {
  draft: Draft | null;
  request: NewAgent | null;
  blocked: string | null;
  sent: { deployment: Deployment; revision: number } | null;
}

function Failed({ entry, last, busy, actions }: { entry: Extract<Entry, { kind: "failed" }>; last: boolean; busy: boolean; actions: ChatActions }) {
  return (
    <Platform slot="failed">
      <p data-reason={entry.reason}>
        {entry.reason === "unreachable" ? "The model didn't answer, so nothing was read." : "The model's answer didn't match the compiler's schema, so I used none of it."} Your message is kept.
      </p>
      {last ? (
        <div className="flex flex-wrap gap-3">
          <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onRetry(entry.messageId)}>
            Try again
          </Button>
          <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onWithoutModel(entry.messageId)}>
            Continue without the model
          </Button>
        </div>
      ) : null}
    </Platform>
  );
}

function EntryView({ entry, c, last, busy, creation, actions }: { entry: Entry; c: Conversation; last: boolean; busy: boolean; creation: Creation; actions: ChatActions }) {
  switch (entry.kind) {
    case "owner":
      return <Owner text={entry.text} />;
    case "said":
      return (
        <Platform slot="said">
          {entry.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
          {last && entry.asks.kind === "model" && nextStep(c).kind === "model" ? <ModelChoices busy={busy} onChoose={actions.onChoose} /> : null}
        </Platform>
      );
    case "summary": {
      const current = currentSummary(c)?.id === entry.id;
      const sent = creation.sent?.revision === entry.revision ? creation.sent.deployment : null;
      if ((!current && !sent) || !creation.draft || !creation.request) {
        return (
          <Platform slot="replaced">
            <p className="text-sm text-muted-foreground">An earlier version of your agent, replaced after you changed it.</p>
          </Platform>
        );
      }
      return (
        <Summary
          draft={creation.draft}
          request={creation.request}
          current={current}
          blocked={creation.blocked}
          deployment={sent}
          onSent={(id) => actions.onSent(id, entry.revision)}
          onStartOver={actions.onStartOver}
        />
      );
    }
    case "failed":
      return <Failed entry={entry} last={last} busy={busy} actions={actions} />;
    default: {
      const unhandled: never = entry;
      throw new Error(`unhandled entry ${JSON.stringify(unhandled)}`);
    }
  }
}

/**
 * The conversation (brief A0 to A2 and A5, DEC-473, DEC-474): the log, then the composer. The
 * platform's replies are plain text; only deterministic parts carry buttons (PX-18). The composer
 * keeps focus as the log grows, and the newest part of the log is brought into view above it.
 */
export function Chat({ conversation, busy, onSend, creation, actions }: { conversation: Conversation; busy: boolean; onSend: (text: string) => void; creation: Creation; actions: ChatActions }) {
  const id = useId();
  const [text, setText] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const log = useRef<HTMLDivElement>(null);
  const { entries } = conversation;
  const seen = useRef(entries.length);
  const done = isConfirmed(creation.sent?.deployment ?? null);

  useEffect(() => {
    const added = entries.length - seen.current;
    seen.current = entries.length;
    const box = log.current;
    if (added <= 0 || !box) return;
    const fresh = [...box.children].slice(-added) as HTMLElement[];
    const behavior: ScrollBehavior = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    const top = fresh[0].getBoundingClientRect().top;
    const bottom = fresh[fresh.length - 1].getBoundingClientRect().bottom;
    const visible = (field.current?.closest("form")?.getBoundingClientRect().top ?? window.innerHeight) - HEADER_CLEARANCE_PX;
    if (bottom - top > visible) fresh[0].scrollIntoView({ block: "start", behavior });
    else fresh[fresh.length - 1].scrollIntoView({ block: "nearest", behavior });
  }, [entries.length]);

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    const said = text.trim();
    if (said === "" || busy || done) return;
    setText("");
    onSend(said);
    field.current?.focus();
  };
  const keys = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submit();
    }
  };
  const refocus =
    <A extends unknown[]>(act: (...args: A) => void) =>
    (...args: A) => {
      act(...args);
      field.current?.focus();
    };
  const wrapped: ChatActions = { ...actions, onChoose: refocus(actions.onChoose), onRetry: refocus(actions.onRetry), onWithoutModel: refocus(actions.onWithoutModel) };

  return (
    <div className="grid gap-(--section-gap)">
      <StepHeading className="text-h2">Set up an agent</StepHeading>

      <div ref={log} role="log" aria-label="Conversation" className="grid gap-6" data-slot="conversation">
        {entries.map((entry, i) => (
          <div key={entry.id} data-kind={entry.kind} className="grid">
            <EntryView entry={entry} c={conversation} last={i === entries.length - 1} busy={busy} creation={creation} actions={wrapped} />
          </div>
        ))}
      </div>

      <form
        onSubmit={submit}
        noValidate
        data-slot="composer"
        className="sticky bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) grid gap-1.5 bg-card px-(--page-x) pt-1 pb-3 lg:bottom-0 lg:mx-0 lg:px-0 lg:pb-[calc(var(--dock-clearance)+1rem)]"
      >
        <p role="status" className="min-h-5 text-sm leading-5 text-muted-foreground" data-slot="thinking">
          {busy ? "Reading your message…" : ""}
        </p>
        <div className="flex items-end gap-2 rounded-3xl border border-border bg-card py-1.5 pr-1.5 pl-4 focus-within:ring-3 focus-within:ring-ring">
          <label htmlFor={`${id}-message`} className="sr-only">
            Your message
          </label>
          <textarea
            ref={field}
            id={`${id}-message`}
            rows={1}
            value={text}
            spellCheck
            disabled={done}
            aria-describedby={`${id}-note`}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={keys}
            className="field-sizing-content max-h-48 min-h-11 flex-1 resize-none bg-transparent py-2.5 text-base leading-normal text-foreground outline-none disabled:cursor-not-allowed"
          />
          <button type="submit" aria-label="Send" className={cn(KEY, "size-11 shrink-0 rounded-full px-0")} disabled={busy || done || text.trim() === ""}>
            <ArrowUp className="size-5" weight="bold" aria-hidden />
          </button>
        </div>
        <p id={`${id}-note`} className="px-4 text-caption text-pretty text-muted-foreground" data-slot="model-note">
          {conversation.figuresOnly ? "Reading without the model now: figures only, exactly as written." : "A model reads your words; it can't set anything you didn't say."} Here a fixture stands in for it, and
          nothing leaves this page.
        </p>
      </form>
    </div>
  );
}
