"use client";

import { type FormEvent, type ReactNode, useId } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowLeft, ChatText, ListNumbers } from "@phosphor-icons/react";
import { FIELD } from "@/components/auth/buttons";
import { cn } from "@/lib/utils";
import { KEY } from "@/components/kumo/key";

/** The id of every step's heading, so the flow can move focus to it when the step changes. */
export const STEP_HEADING = "new-agent-step";

/** A step's heading, the one focus target when the step changes. */
export function StepHeading({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h1 id={STEP_HEADING} tabIndex={-1} className={cn("text-h1 text-balance outline-none", className)}>
      {children}
    </h1>
  );
}

export function BackButton({ onClick, children = "Back" }: { onClick: () => void; children?: ReactNode }) {
  return (
    <Button type="button" variant="secondary" size="lg" className={cn("w-fit", KEY)} onClick={onClick}>
      <ArrowLeft className="size-4" aria-hidden />
      {children}
    </Button>
  );
}

const CHOICE =
  "press grid w-full content-start gap-2 rounded-2xl border border-foreground/25 bg-card px-5 py-5 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring min-h-11";

/**
 * A0's way in. The three questions and "Describe it yourself" are equal choices (brief A0): the same
 * shape, size and weight, side by side in a fixed order, with nothing preselected or focused.
 */
export function StartStep({ onQuestions, onDescribe }: { onQuestions: () => void; onDescribe: () => void }) {
  return (
    <div className="grid gap-8">
      <div className="grid gap-3">
        <StepHeading>Set up an agent</StepHeading>
        <p className="max-w-measure text-pretty text-muted-foreground">
          Say what this agent is for and the platform drafts its mandate: the limits it must stay inside. You then check every field, see where each came from, and confirm it
          section by section. Nothing is active until you do.
        </p>
      </div>
      <div role="group" aria-label="How to start" className="grid gap-3 sm:grid-cols-2" data-slot="start-choices">
        <button type="button" className={CHOICE} onClick={onQuestions} data-slot="start-choice">
          <ListNumbers className="size-6" aria-hidden />
          <span className="text-h3">Answer three questions</span>
          <span className="text-sm text-pretty text-muted-foreground">How much money, what the goal is, and how much loss you can stand. One at a time.</span>
        </button>
        <button type="button" className={CHOICE} onClick={onDescribe} data-slot="start-choice">
          <ChatText className="size-6" aria-hidden />
          <span className="text-h3">Describe it yourself</span>
          <span className="text-sm text-pretty text-muted-foreground">Write it in your own words, as much or as little as you like.</span>
        </button>
      </div>
    </div>
  );
}

/** One of A0's three questions: the question is the heading, the answer is the owner's words or numbers. */
export function QuestionStep({
  index,
  question,
  label,
  hint,
  value,
  error,
  multiline = false,
  onChange,
  onBack,
  onContinue,
}: {
  index: number;
  question: string;
  label: string;
  hint: ReactNode;
  value: string;
  error: string | null;
  multiline?: boolean;
  onChange: (value: string) => void;
  onBack: () => void;
  onContinue: () => void;
}) {
  const id = useId();
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onContinue();
  };
  const field = {
    id: `${id}-answer`,
    value,
    "aria-labelledby": `${STEP_HEADING} ${id}-label`,
    "aria-describedby": error ? `${id}-error ${id}-hint` : `${id}-hint`,
    "aria-invalid": error ? true : undefined,
    autoComplete: "off",
    spellCheck: multiline,
  } as const;
  return (
    <form onSubmit={submit} noValidate className="grid gap-8">
      <div className="grid gap-3">
        <p className="text-label text-muted-foreground">
          Question <span className="tabular">{index}</span> of <span className="tabular">3</span>
        </p>
        <StepHeading>{question}</StepHeading>
      </div>
      <div className="grid gap-2">
        <label id={`${id}-label`} htmlFor={field.id} className="field-label">
          {label}
        </label>
        {multiline ? (
          <textarea {...field} rows={4} onChange={(e) => onChange(e.target.value)} className={cn(FIELD, "h-auto min-h-32 resize-y py-3 leading-normal")} />
        ) : (
          <input {...field} type="text" onChange={(e) => onChange(e.target.value)} className={FIELD} />
        )}
        <p id={`${id}-hint`} className="max-w-measure text-sm text-pretty text-muted-foreground">
          {hint}
        </p>
        {error ? (
          <p id={`${id}-error`} role="alert" data-slot="step-error" className="max-w-measure text-sm font-medium text-pretty">
            {error}
          </p>
        ) : null}
      </div>
      <div className="flex flex-col-reverse gap-3 sm:flex-row sm:items-center sm:justify-between">
        <BackButton onClick={onBack} />
        <Button type="submit" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")}>
          Continue
        </Button>
      </div>
    </form>
  );
}

/** A1: the owner writes the mandate in their own words; the draft quotes them back. */
export function DescribeStep({
  value,
  error,
  onChange,
  onBack,
  onContinue,
}: {
  value: string;
  error: string | null;
  onChange: (value: string) => void;
  onBack: () => void;
  onContinue: () => void;
}) {
  const id = useId();
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onContinue();
  };
  return (
    <form onSubmit={submit} noValidate className="grid gap-8">
      <div className="grid gap-3">
        <StepHeading>Describe it yourself</StepHeading>
        <p className="max-w-measure text-pretty text-muted-foreground">
          A mandate is the set of limits your agent must stay inside. Say how much money it may use, what it is for, and how much it may lose, in your own words. The draft quotes them
          back to you.
        </p>
      </div>
      <div className="grid gap-2">
        <label htmlFor={`${id}-text`} className="field-label">
          Your description
        </label>
        <textarea
          id={`${id}-text`}
          value={value}
          rows={7}
          spellCheck
          aria-describedby={error ? `${id}-error ${id}-hint` : `${id}-hint`}
          aria-invalid={error ? true : undefined}
          onChange={(e) => onChange(e.target.value)}
          className={cn(FIELD, "h-auto min-h-44 resize-y py-3 leading-normal")}
        />
        <p id={`${id}-hint`} className="max-w-measure text-sm text-pretty text-muted-foreground">
          Write amounts in figures. Anything a limit cannot check is kept as a note and marked not enforced.
        </p>
        {error ? (
          <p id={`${id}-error`} role="alert" data-slot="step-error" className="max-w-measure text-sm font-medium text-pretty">
            {error}
          </p>
        ) : null}
      </div>
      <div className="flex flex-col-reverse gap-3 sm:flex-row sm:items-center sm:justify-between">
        <BackButton onClick={onBack} />
        <Button type="submit" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")}>
          Continue
        </Button>
      </div>
    </form>
  );
}
