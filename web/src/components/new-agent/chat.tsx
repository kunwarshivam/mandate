"use client";

import { type ReactNode, useEffect, useRef } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Composer, type ComposerHandle } from "@/components/chat/composer";
import { Markdown } from "@/components/chat/markdown";
import { OwnerSaid } from "@/components/chat/record-reply";
import { KEY } from "@/components/kumo/key";
import type { NewAgent } from "@/lib/fixture-journey";
import { type SetUp, useRuntime } from "@/lib/mock-runtime";
import { type Conversation, type Entry, currentSummary, nextStep } from "./conversation";
import { type Draft, type ModelChoice, type ModelId } from "./draft";
import { StepHeading } from "./steps";
import { SetUpSteps, isBound } from "./set-up-steps";
import { Summary } from "./summary";

/** The sticky header's height plus a margin, matching the page's `scroll-padding-top` of 5rem. */
const HEADER_CLEARANCE_PX = 80;

/** The platform's side of the conversation: text, as in any chat, which may carry a model's Markdown. */
function Platform({ children, slot }: { children: ReactNode; slot?: string }) {
  return (
    <div data-slot={slot ?? "platform"} className="grid max-w-measure gap-2 text-pretty">
      <span className="sr-only">Owlhead: </span>
      {children}
    </div>
  );
}

const CHOICE =
  "press grid content-start gap-0.5 rounded-2xl border border-foreground/25 bg-card px-4 py-3 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-55 min-h-11";

function ModelChoices({ models, busy, onChoose }: { models: readonly ModelChoice[]; busy: boolean; onChoose: (id: ModelId) => void }) {
  if (models.length === 0) return <p className="text-sm text-muted-foreground">Your deployment lists no model this page can set up yet.</p>;
  return (
    <div role="group" aria-label="Models" className="grid gap-2 sm:grid-cols-2" data-slot="model-choices">
      {models.map((m) => (
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
  onCreated: (setUpId: string, revision: number) => void;
  onStartOver: () => void;
}

/** What the summary shown at a revision needs: the draft and request as they stand, and each revision's latest set-up. */
export interface Creation {
  draft: Draft | null;
  request: NewAgent | null;
  blocked: string | null;
  setUps: Readonly<Record<number, SetUp>>;
}

/** What an earlier summary's set-up recorded before a change replaced it. */
function replacedNote(setUp: SetUp | null): string {
  const replaced = "An earlier version of your agent, replaced after you changed it.";
  if (setUp?.created.phase !== "recorded") return replaced;
  if (setUp.confirmed?.phase !== "recorded") return `${replaced} Its version 1 was created and not confirmed; no agent exists for it.`;
  return `${replaced} Its version 1 was created and confirmed, and not deployed; no agent exists for it.`;
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
      return <OwnerSaid text={entry.text} />;
    case "said":
      return (
        <Platform slot="said">
          <Markdown text={entry.lines.join("\n\n")} links="show" />
          {last && entry.asks.kind === "model" && nextStep(c).kind === "model" ? <ModelChoices models={c.models} busy={busy} onChoose={actions.onChoose} /> : null}
        </Platform>
      );
    case "summary": {
      const current = currentSummary(c)?.id === entry.id;
      const setUp = creation.setUps[entry.revision] ?? null;
      if ((!current && !isBound(setUp)) || !creation.draft || !creation.request) {
        return (
          <Platform slot="replaced">
            <p className="text-sm text-muted-foreground">{replacedNote(setUp)}</p>
          </Platform>
        );
      }
      return (
        <div className="grid gap-4">
          <Summary draft={creation.draft} request={creation.request} setUp={setUp} />
          <SetUpSteps
            draft={creation.draft}
            request={creation.request}
            current={current}
            blocked={creation.blocked}
            setUp={setUp}
            onCreated={(id) => actions.onCreated(id, entry.revision)}
            onStartOver={actions.onStartOver}
          />
        </div>
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
 * Over the API no model reads the owner's words (the compiler is not served, spec §4.1), so the
 * figures-only reader does, in this page; the words reach the deployment only in the version created.
 */
export const API_NOTE = "Your deployment doesn't serve a model to read your words yet, so figures are read exactly as written, in this page. Nothing is sent until you create the version.";

/**
 * The conversation (brief A0 to A2 and A5, DEC-476, DEC-477): the log, then the composer. The
 * platform's replies are text, drawn from Markdown with their links shown but never opened, since a
 * model wrote part of them (DEC-481); only deterministic parts carry buttons (PX-18). The composer
 * keeps focus as the log grows, and the newest part of the log is brought into view above it.
 */
export function Chat({
  conversation,
  busy,
  onSend,
  creation,
  actions,
  initialText = null,
}: {
  conversation: Conversation;
  busy: boolean;
  onSend: (text: string) => void;
  creation: Creation;
  actions: ChatActions;
  initialText?: string | null;
}) {
  const { source } = useRuntime();
  const composer = useRef<ComposerHandle>(null);
  const log = useRef<HTMLDivElement>(null);
  const { entries } = conversation;
  const seen = useRef(entries.length);
  const done = Object.values(creation.setUps).some(isBound);

  useEffect(() => {
    const added = entries.length - seen.current;
    seen.current = entries.length;
    const box = log.current;
    if (added <= 0 || !box) return;
    const fresh = [...box.children].slice(-added) as HTMLElement[];
    const behavior: ScrollBehavior = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    const top = fresh[0].getBoundingClientRect().top;
    const bottom = fresh[fresh.length - 1].getBoundingClientRect().bottom;
    const visible = (composer.current?.top() ?? window.innerHeight) - HEADER_CLEARANCE_PX;
    if (bottom - top > visible) fresh[0].scrollIntoView({ block: "start", behavior });
    else fresh[fresh.length - 1].scrollIntoView({ block: "nearest", behavior });
  }, [entries.length]);

  const refocus =
    <A extends unknown[]>(act: (...args: A) => void) =>
    (...args: A) => {
      act(...args);
      composer.current?.focus();
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

      <Composer
        ref={composer}
        label="Your answer"
        labelShown
        initialText={initialText}
        busy={busy}
        disabled={done}
        status={busy ? "Reading your message…" : ""}
        onSend={onSend}
        className="sticky bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) bg-card px-(--page-x) pt-1 pb-3 lg:bottom-0 lg:mx-0 lg:px-0 lg:pb-[calc(var(--dock-clearance)+1rem)]"
        note={source === "api" ? API_NOTE : (
          <>
            {conversation.figuresOnly ? "Reading without the model now: figures only, exactly as written." : "A model reads your words; it can't set anything you didn't say."} Here a fixture stands
            in for it, and nothing leaves this page.
          </>
        )}
      />
    </div>
  );
}
