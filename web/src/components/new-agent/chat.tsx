"use client";

import { type FormEvent, type KeyboardEvent, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { CheckCircle, PaperPlaneRight } from "@phosphor-icons/react";
import { FIELD } from "@/components/auth/buttons";
import { KEY } from "@/components/kumo/key";
import { type Dec } from "@/lib/decimal";
import { percent, usd } from "@/lib/format";
import { isPlatformAuthored } from "@/lib/labels";
import { cn } from "@/lib/utils";
import { type Conversation, type Entry, type NotedItem, type Pending, type Step, activeAsk, draftOf, lossWords } from "./conversation";
import { type Draft, type DraftSection, LOSS_CEILING, MODELS, type ModelId, type SectionKey } from "./draft";
import { ContractCard, DraftFields, NotEnforcedList, QuotedSpan } from "./mandate-parts";
import { StepHeading } from "./steps";

export const INTRO =
  "Tell me about this agent in your own words: how much money it may use, what it is for, and how much it could lose. Say it all at once, or answer one question at a time. I draft its mandate, the limits it must stay inside, and you confirm every part. Nothing is active until you do.";

const QUESTION: Record<Exclude<Step["kind"], "param" | "section" | "check" | "ready">, { text: string; hint: string }> = {
  money: { text: "How much money may this agent use?", hint: "In dollars. Simulated money: it trades on paper only, and never touches more than this." },
  goal: {
    text: "What is it for?",
    hint: "In your own words. If it should stop at a level, give the level in figures. Anything a limit cannot check is kept as a note and marked not enforced.",
  },
  loss: { text: "How much could you stand to lose, in total?", hint: "In dollars, or as a percentage of the money. When it has lost this much it closes everything and pauses." },
  symbols: { text: "Which stocks or ETFs may it trade?", hint: "Write their symbols. Only you choose them; the platform suggests none. One agent trades an instrument on an account." },
  model: { text: "How should it decide?", hint: "Choose a model, or name it. They are in alphabetical order; the platform ranks none and recommends none." },
};

/** The sticky header's height plus a margin, matching the page's `scroll-padding-top` of 5rem. */
const HEADER_CLEARANCE_PX = 80;

const READY_TEXT = "Every section is confirmed. Next you read the whole mandate once more, exactly as the journal will keep it, and confirm it with your passkey.";

/** A message from the platform: deterministic text, never the model's. */
function Platform({ children, slot }: { children: ReactNode; slot?: string }) {
  return (
    <div data-slot={slot ?? "platform"} className="grid max-w-measure gap-1.5 text-pretty">
      <span className="sr-only">Owlhead: </span>
      {children}
    </div>
  );
}

function Owner({ text }: { text: string }) {
  return (
    <div data-slot="owner-message" className="ml-auto max-w-[85%] rounded-2xl bg-background px-4 py-2.5 text-pretty whitespace-pre-wrap wrap-anywhere">
      <span className="sr-only">You: </span>
      {text}
    </div>
  );
}

function Noted({ items }: { items: NotedItem[] }) {
  return (
    <div data-slot="noted" className="grid max-w-measure gap-1 rounded-2xl bg-mandate px-5 py-4 text-mandate-foreground">
      <p className="text-label text-mandate-muted">Noted from your words</p>
      <dl className="grid">
        {items.map((item) => (
          <div key={`${item.label}-${item.quote}`} data-slot="noted-item" className="grid gap-1 border-b border-mandate-strong/15 py-2.5 last:border-b-0 sm:grid-cols-[minmax(0,10rem)_minmax(0,1fr)] sm:gap-x-5">
            <dt className="text-sm text-mandate-muted">{item.label}</dt>
            <dd className="grid min-w-0 justify-items-start gap-1 wrap-anywhere">
              <span className="font-medium text-pretty tabular">{item.value}</span>
              <QuotedSpan quote={item.quote} className="text-mandate-muted" />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/** The model's words, set apart as a quotation: they inform and set nothing (PX-18). */
function ModelReply({ text }: { text: string }) {
  return (
    <figure data-slot="model-reply" className="grid max-w-measure gap-1">
      <figcaption className="text-caption text-muted-foreground">The model said</figcaption>
      <blockquote className="border-l-2 border-border pl-3 text-pretty">{text}</blockquote>
    </figure>
  );
}

const CHOICE =
  "press grid w-full content-start gap-1 rounded-2xl border border-foreground/25 bg-card px-4 py-4 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-55 min-h-11";

function ModelChoices({ busy, onChoose }: { busy: boolean; onChoose: (id: ModelId) => void }) {
  return (
    <div role="group" aria-label="Models" className="grid gap-3 sm:grid-cols-2" data-slot="model-choices">
      {MODELS.map((m) => (
        <button key={m.id} type="button" className={CHOICE} disabled={busy} onClick={() => onChoose(m.id)} data-slot="model-choice">
          <span className="text-base font-semibold">{m.name}</span>
          <span className="text-sm text-pretty text-muted-foreground">{m.what}</span>
          <span className="font-mono text-caption text-muted-foreground" translate="no">
            {m.id} {m.version}
          </span>
        </button>
      ))}
    </div>
  );
}

function pendingWords(p: Pending, money: Dec | null): { value: string; field: string } {
  if (p.field === "money") return { value: usd(p.value), field: "the money it may use" };
  return { value: lossWords(p.loss, money), field: "the most it may lose, in total" };
}

function SectionCard({ section, confirmed, active, busy, onConfirm }: { section: DraftSection; confirmed: boolean; active: boolean; busy: boolean; onConfirm?: () => void }) {
  const headingId = useId();
  const proposed = section.fields.filter((f) => f.provenance !== null && isPlatformAuthored(f.provenance)).length;
  return (
    <section aria-labelledby={headingId} data-slot="review-section" data-section={section.key} data-confirmed={confirmed ? "true" : "false"} className="grid gap-3 rounded-2xl bg-background px-5 py-4 sm:px-6">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 id={headingId} className="text-h3">
          {section.title}
        </h2>
        <span className="inline-flex items-center gap-1.5 text-sm text-muted-foreground" data-slot="section-state">
          {confirmed ? (
            <>
              <CheckCircle weight="fill" className="size-4 text-foreground" aria-hidden />
              <span className="text-foreground">Confirmed</span>
            </>
          ) : proposed > 0 ? (
            <span>
              <span className="tabular">{proposed}</span> proposed, not active yet
            </span>
          ) : (
            "Not confirmed yet"
          )}
        </span>
      </div>
      <p className="max-w-measure text-sm text-pretty text-muted-foreground">{section.lead}</p>
      <DraftFields fields={section.fields} confirmed={confirmed} />
      {active && onConfirm ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={onConfirm}>
            Confirm section<span className="sr-only">: {section.title}</span>
          </Button>
          <p className="max-w-measure text-sm text-pretty text-muted-foreground">Or say what to change.</p>
        </div>
      ) : null}
    </section>
  );
}

export interface ChatActions {
  onChoose: (id: ModelId) => void;
  onCheck: (yes: boolean) => void;
  onConfirm: (key: SectionKey) => void;
  onReady: () => void;
  onRetry: (messageId: string) => void;
  onWithoutModel: (messageId: string) => void;
}

function Ask({
  entry,
  draft,
  money,
  active,
  busy,
  actions,
}: {
  entry: Extract<Entry, { kind: "ask" }>;
  draft: Draft | null;
  money: Dec | null;
  active: boolean;
  busy: boolean;
  actions: ChatActions;
}) {
  const { step } = entry;
  switch (step.kind) {
    case "money":
    case "goal":
    case "symbols":
    case "loss":
    case "model": {
      const q = QUESTION[step.kind];
      return (
        <div className="grid gap-3" data-slot="ask" data-step={step.kind}>
          <Platform>
            <p className="font-semibold">{q.text}</p>
            <p className="text-sm text-muted-foreground">
              {q.hint}
              {step.kind === "loss" && entry.ceiling !== undefined ? (
                <>
                  {" "}
                  This workspace&apos;s limit: at most {percent(LOSS_CEILING, 0)} of the money, <span className="font-mono tabular">{usd(entry.ceiling)}</span>.
                </>
              ) : null}
            </p>
          </Platform>
          {step.kind === "model" && active ? <ModelChoices busy={busy} onChoose={actions.onChoose} /> : null}
        </div>
      );
    }
    case "param": {
      const param = MODELS.flatMap((m) => m.params).find((p) => p.key === step.key);
      return (
        <div data-slot="ask" data-step="param">
          <Platform>
            <p className="font-semibold">{param?.question}</p>
            <p className="text-sm text-muted-foreground">{param?.hint}</p>
          </Platform>
        </div>
      );
    }
    case "check": {
      const pending = entry.pending;
      if (!pending) return null;
      const { value, field } = pendingWords(pending, money);
      return (
        <div className="grid gap-3" data-slot="ask" data-step="check">
          <Platform>
            <p className="font-semibold">
              We read “{pending.quote}” as {value}, {field}. Is that right?
            </p>
            <p className="text-sm text-muted-foreground">It was written in words, so the model read it. Nothing is kept until you say so.</p>
          </Platform>
          {active ? (
            <div className="flex flex-wrap gap-3">
              <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onCheck(true)}>
                Yes, {value}
              </Button>
              <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onCheck(false)}>
                No
              </Button>
            </div>
          ) : null}
        </div>
      );
    }
    case "section": {
      const live = active ? draft?.sections.find((s) => s.key === step.key) : undefined;
      const section = live ?? entry.snapshot;
      if (!section) {
        const title = draft?.sections.find((s) => s.key === step.key)?.title ?? "This section";
        return (
          <Platform slot="replaced">
            <p className="text-sm text-muted-foreground">{title}: not confirmed. Your answers changed, so it was drafted again below.</p>
          </Platform>
        );
      }
      return <SectionCard section={section} confirmed={!live} active={active} busy={busy} onConfirm={() => actions.onConfirm(step.key)} />;
    }
    case "ready":
      return (
        <div className="grid gap-3" data-slot="ask" data-step="ready">
          <Platform>
            <p>{READY_TEXT}</p>
          </Platform>
          {active ? (
            <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-fit")} disabled={busy} onClick={actions.onReady}>
              Review and confirm
            </Button>
          ) : null}
        </div>
      );
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

function Failed({ entry, last, busy, actions }: { entry: Extract<Entry, { kind: "failed" }>; last: boolean; busy: boolean; actions: ChatActions }) {
  return (
    <div data-slot="failed" data-reason={entry.reason} className="grid max-w-measure gap-3 rounded-2xl bg-background px-5 py-4">
      <p className="font-semibold">{entry.reason === "unreachable" ? "The model did not answer." : "We could not compile this."}</p>
      <p className="text-sm text-pretty">
        {entry.reason === "unreachable" ? "Nothing was read and nothing changed." : "The model's answer did not match the compiler's schema, so none of it was used."} Your words are kept. Try
        again, or read them without the model: figures only.
      </p>
      {last ? (
        <div className="flex flex-wrap gap-3">
          <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onRetry(entry.messageId)}>
            Try again
          </Button>
          <Button type="button" variant="secondary" size="lg" className={KEY} disabled={busy} onClick={() => actions.onWithoutModel(entry.messageId)}>
            Read it without the model
          </Button>
        </div>
      ) : null}
    </div>
  );
}

function EntryView({ entry, c, draft, last, latestDraft, busy, actions }: { entry: Entry; c: Conversation; draft: Draft | null; last: boolean; latestDraft: boolean; busy: boolean; actions: ChatActions }) {
  switch (entry.kind) {
    case "intro":
      return (
        <Platform slot="intro">
          <p>{INTRO}</p>
        </Platform>
      );
    case "ask":
      return <Ask entry={entry} draft={draft} money={c.money?.value ?? null} active={activeAsk(c)?.id === entry.id} busy={busy} actions={actions} />;
    case "owner":
      return <Owner text={entry.text} />;
    case "reply":
      return <ModelReply text={entry.text} />;
    case "withheld":
      return (
        <Platform slot="withheld">
          <p className="text-sm text-muted-foreground">The model&apos;s reply was withheld: it read as advice, and the platform gives none. Nothing changed.</p>
        </Platform>
      );
    case "noted":
      return <Noted items={entry.items} />;
    case "refused":
      return (
        <Platform slot="refused">
          <p className="font-medium">{entry.text}</p>
        </Platform>
      );
    case "unread":
      return (
        <Platform slot="unread">
          <p>{entry.text}</p>
        </Platform>
      );
    case "draft":
      if (!latestDraft || !draft) {
        return (
          <Platform slot="earlier-draft">
            <p className="text-sm text-muted-foreground">An earlier draft, replaced when your answers changed.</p>
          </Platform>
        );
      }
      return (
        <div data-slot="draft" className="grid gap-(--block-gap)">
          <Platform>
            <p>Here is the mandate drafted from your words. Check it, then confirm it one section at a time. Say what to change at any point.</p>
          </Platform>
          <ContractCard draft={draft} />
          <NotEnforcedList draft={draft} />
        </div>
      );
    case "failed":
      return <Failed entry={entry} last={last} busy={busy} actions={actions} />;
    default: {
      const unhandled: never = entry;
      throw new Error(`unhandled entry ${JSON.stringify(unhandled)}`);
    }
  }
}

/**
 * The conversation (brief A0 to A2, DEC-473): the log, then the composer. Only deterministic cards
 * carry buttons; the model's replies are quotations. The composer keeps focus as the log grows, and
 * the newest part of the log is brought into view above it.
 */
export function Chat({ conversation, busy, onSend, actions }: { conversation: Conversation; busy: boolean; onSend: (text: string) => void; actions: ChatActions }) {
  const id = useId();
  const [text, setText] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const log = useRef<HTMLDivElement>(null);
  const draft = draftOf(conversation);
  const { entries } = conversation;
  const latestDraftId = [...entries].reverse().find((e) => e.kind === "draft")?.id;
  const seen = useRef(entries.length);

  useEffect(() => {
    const added = entries.length - seen.current;
    seen.current = entries.length;
    const box = log.current;
    if (added <= 0 || !box) return;
    const fresh = [...box.children].slice(-added) as HTMLElement[];
    const behavior: ScrollBehavior = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    const draft = fresh.find((e) => e.dataset.kind === "draft");
    if (draft) {
      draft.scrollIntoView({ block: "start", behavior });
      return;
    }
    const top = fresh[0].getBoundingClientRect().top;
    const bottom = fresh[fresh.length - 1].getBoundingClientRect().bottom;
    const visible = (field.current?.closest("form")?.getBoundingClientRect().top ?? window.innerHeight) - HEADER_CLEARANCE_PX;
    if (bottom - top > visible) fresh[0].scrollIntoView({ block: "start", behavior });
    else fresh[fresh.length - 1].scrollIntoView({ block: "nearest", behavior });
  }, [entries.length]);

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    const said = text.trim();
    if (said === "" || busy) return;
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
  const after = (act: () => void) => () => {
    act();
    field.current?.focus();
  };
  const wrapped: ChatActions = {
    onChoose: (m) => after(() => actions.onChoose(m))(),
    onCheck: (yes) => after(() => actions.onCheck(yes))(),
    onConfirm: (key) => after(() => actions.onConfirm(key))(),
    onReady: actions.onReady,
    onRetry: (m) => after(() => actions.onRetry(m))(),
    onWithoutModel: (m) => after(() => actions.onWithoutModel(m))(),
  };

  return (
    <div className="grid gap-(--section-gap)">
      <header className="grid gap-3">
        <StepHeading>Set up an agent</StepHeading>
        <p className="max-w-measure text-pretty text-muted-foreground">A conversation that drafts this agent&apos;s mandate from your words. You confirm every part of it.</p>
      </header>

      <div ref={log} role="log" aria-label="Conversation" className="grid gap-5" data-slot="conversation">
        {entries.map((entry, i) => (
          <div key={entry.id} data-kind={entry.kind} className="grid">
            <EntryView entry={entry} c={conversation} draft={draft} last={i === entries.length - 1} latestDraft={entry.id === latestDraftId} busy={busy} actions={wrapped} />
          </div>
        ))}
      </div>

      <form
        onSubmit={submit}
        noValidate
        data-slot="composer"
        className="sticky bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))] z-10 -mx-(--page-x) grid gap-2 border-t border-border/70 bg-card px-(--page-x) pt-2 pb-4 lg:bottom-0 lg:mx-0 lg:px-0 lg:pb-[calc(var(--dock-clearance)+1rem)]"
      >
        <p role="status" className="min-h-5 text-sm leading-5 text-muted-foreground" data-slot="thinking">
          {busy ? "Reading your words…" : ""}
        </p>
        <label htmlFor={`${id}-message`} className="sr-only">
          Your message
        </label>
        <textarea
          ref={field}
          id={`${id}-message`}
          rows={2}
          value={text}
          spellCheck
          aria-describedby={`${id}-note`}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={keys}
          className={cn(FIELD, "h-auto min-h-14 resize-y py-3 leading-normal")}
        />
        <div className="flex items-start justify-between gap-3">
          <p id={`${id}-note`} className="max-w-measure text-caption text-pretty text-muted-foreground" data-slot="model-note">
            {conversation.figuresOnly ? "Reading without the model now: figures only, read exactly as written." : "A model reads your words and quotes back what it read; it sets nothing."}
            {" Here a fixture stands in for it, and nothing leaves this page."}
          </p>
          <Button type="submit" variant="secondary" size="lg" className={cn(KEY, "shrink-0")} disabled={busy || text.trim() === ""}>
            Send
            <PaperPlaneRight className="size-4" aria-hidden />
          </Button>
        </div>
      </form>
    </div>
  );
}
