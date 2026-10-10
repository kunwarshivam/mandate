"use client";

import { type FormEvent, type KeyboardEvent, type ReactNode, type Ref, useId, useImperativeHandle, useRef, useState } from "react";
import { ArrowUp } from "pixelarticons/react/ArrowUp.js";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

export interface ComposerHandle {
  focus: () => void;
  /** Where the composer starts on screen, so a log can keep its newest part above it. */
  top: () => number;
}

/**
 * The one message field: Enter sends, Shift+Enter breaks the line, and the field keeps focus after
 * sending. `status` sits above it for what is happening now; `note`, when there is one, sits below it
 * for what a message can and cannot do. Where it sticks is the caller's, through `className`. The
 * label names the field; with `labelShown` it is also written above it, where a screen asks a
 * question and the field must say "type here" (critique C-4). The send button is flat, with an even
 * edge: a heavier foot on a round button draws an arc that reads as a loading spinner.
 */
export function Composer({
  label,
  labelShown = false,
  note,
  status,
  disabled = false,
  busy = false,
  placeholder,
  onSend,
  className,
  ref,
  slot = "composer",
  initialText = null,
}: {
  label: string;
  labelShown?: boolean;
  note?: ReactNode;
  status?: ReactNode;
  disabled?: boolean;
  busy?: boolean;
  placeholder?: string;
  onSend: (text: string) => void;
  className?: string;
  ref?: Ref<ComposerHandle>;
  slot?: string;
  /** Words carried in from elsewhere, waiting to be sent; the field takes focus when there are some. */
  initialText?: string | null;
}) {
  const id = useId();
  const [text, setText] = useState(initialText ?? "");
  const field = useRef<HTMLTextAreaElement>(null);
  const form = useRef<HTMLFormElement>(null);
  useImperativeHandle(ref, () => ({ focus: () => field.current?.focus(), top: () => form.current?.getBoundingClientRect().top ?? window.innerHeight }), []);

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    const said = text.trim();
    if (said === "" || busy || disabled) return;
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

  return (
    <form ref={form} onSubmit={submit} noValidate data-slot={slot} className={cn("grid gap-1.5", className)}>
      {status !== undefined ? (
        <p role="status" className="min-h-5 text-sm leading-5 text-muted-foreground" data-slot="thinking">
          {status}
        </p>
      ) : null}
      {labelShown ? (
        <label htmlFor={`${id}-message`} className="px-4 text-label text-foreground">
          {label}
        </label>
      ) : null}
      <div className="flex items-end gap-2 rounded-3xl border border-border bg-card py-1.5 pr-1.5 pl-4 focus-within:ring-3 focus-within:ring-ring">
        {labelShown ? null : (
          <label htmlFor={`${id}-message`} className="sr-only">
            {label}
          </label>
        )}
        <textarea
          ref={field}
          id={`${id}-message`}
          rows={1}
          value={text}
          spellCheck
          disabled={disabled}
          autoFocus={initialText !== null && initialText !== ""}
          placeholder={placeholder}
          aria-describedby={note === undefined ? undefined : `${id}-note`}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={keys}
          className="field-sizing-content max-h-48 min-h-11 flex-1 resize-none bg-transparent py-2.5 text-base leading-normal text-foreground outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed"
        />
        <button type="submit" aria-label="Send" className={cn(KEY, "size-11 shrink-0 rounded-full border-b border-b-foreground/45 px-0 active:not-disabled:pt-0")} disabled={busy || disabled || text.trim() === ""}>
          <ArrowUp className="size-6" aria-hidden />
        </button>
      </div>
      {note === undefined ? null : (
        <p id={`${id}-note`} className="px-4 text-caption text-pretty text-muted-foreground" data-slot="model-note">
          {note}
        </p>
      )}
    </form>
  );
}
