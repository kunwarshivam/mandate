"use client";

import { type ReactNode, useEffect, useId, useRef } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { CheckCircle } from "@phosphor-icons/react";
import { FIELD } from "@/components/auth/buttons";
import { isPlatformAuthored } from "@/lib/labels";
import { type Draft, type DraftSection, SECTION_KEYS, type SectionKey } from "./draft";
import { BackButton, PrototypeNote, StepHeading } from "./goal-steps";
import { ContractCard, DraftFields, NotEnforcedList } from "./mandate-parts";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

/**
 * One section of the draft with its own confirmation (brief A2: per section, never all at once,
 * PX-2). Focus follows the control that replaces the one pressed, so confirming never drops it.
 */
function ReviewSection({
  section,
  confirmed,
  onConfirm,
  onUndo,
  children,
}: {
  section: DraftSection;
  confirmed: boolean;
  /** False when the section cannot be confirmed yet. */
  onConfirm: () => boolean;
  onUndo: () => void;
  children?: ReactNode;
}) {
  const headingId = `review-${section.key}`;
  const actions = useRef<HTMLDivElement>(null);
  const moveFocus = useRef(false);
  useEffect(() => {
    if (!moveFocus.current) return;
    moveFocus.current = false;
    actions.current?.querySelector("button")?.focus();
  }, [confirmed]);
  const proposed = section.fields.filter((f) => f.provenance !== null && isPlatformAuthored(f.provenance)).length;

  return (
    <section aria-labelledby={headingId} data-slot="review-section" data-section={section.key} data-confirmed={confirmed ? "true" : "false"} className="grid gap-(--block-gap)">
      <div className="grid gap-1.5">
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
          <h2 id={headingId} className="text-h2">
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
        <p className="max-w-measure text-pretty text-muted-foreground">{section.lead}</p>
      </div>
      {children}
      <DraftFields fields={section.fields} confirmed={confirmed} />
      <div ref={actions} className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {confirmed ? (
          <>
            <p className="text-sm">You confirmed this section.</p>
            <Button
              type="button"
              variant="secondary"
              size="lg"
              className={KEY}
              onClick={() => {
                moveFocus.current = true;
                onUndo();
              }}
            >
              Undo<span className="sr-only">: {section.title}</span>
            </Button>
          </>
        ) : (
          <Button
            type="button"
            variant="secondary"
            size="lg"
            className={KEY}
            onClick={() => {
              moveFocus.current = onConfirm();
            }}
          >
            Confirm section<span className="sr-only">: {section.title}</span>
          </Button>
        )}
      </div>
    </section>
  );
}

/** The owner's own instrument list, the one field on this screen only they may fill in (V-038). */
function SymbolsField({ value, error, onChange }: { value: string; error: string | null; onChange: (value: string) => void }) {
  const id = useId();
  return (
    <div className="grid gap-2">
      <label htmlFor={`${id}-symbols`} className="field-label">
        Symbols it may trade
      </label>
      <input
        id={`${id}-symbols`}
        type="text"
        value={value}
        autoComplete="off"
        autoCapitalize="characters"
        spellCheck={false}
        aria-describedby={error ? `${id}-error ${id}-hint` : `${id}-hint`}
        aria-invalid={error ? true : undefined}
        onChange={(e) => onChange(e.target.value)}
        className={FIELD}
      />
      <p id={`${id}-hint`} className="text-sm text-muted-foreground">
        Separate them with commas or spaces. At most 20.
      </p>
      {error ? (
        <p id={`${id}-error`} role="alert" data-slot="section-error" className="text-sm font-medium">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/**
 * A2, the compiled review: the contract card on top, every field grouped by section with its
 * provenance, what is not enforced listed apart, and one primary action, enabled only once every
 * section is confirmed.
 */
export function CompiledReview({
  draft,
  confirmed,
  symbolsText,
  symbolsError,
  onSymbols,
  onConfirm,
  onUndo,
  onBack,
  onDeploy,
}: {
  draft: Draft;
  confirmed: Record<SectionKey, boolean>;
  symbolsText: string;
  /** Why the instrument list cannot be confirmed yet, if it cannot. */
  symbolsError: string | null;
  onSymbols: (value: string) => void;
  onConfirm: (key: SectionKey) => boolean;
  onUndo: (key: SectionKey) => void;
  onBack: () => void;
  onDeploy: () => void;
}) {
  const done = SECTION_KEYS.filter((k) => confirmed[k]).length;
  const ready = done === SECTION_KEYS.length;
  return (
    <div className="grid gap-(--section-gap)">
      <div className="grid gap-3">
        <PrototypeNote />
        <p className="text-label text-muted-foreground">Review</p>
        <StepHeading>Check your mandate</StepHeading>
        <p className="max-w-measure text-pretty text-muted-foreground">
          Drafted from your words. Each field says who wrote it. What the platform proposed stays inactive until you confirm its section; change your answers if something reads
          wrong.
        </p>
      </div>

      <ContractCard draft={draft} />

      {draft.sections.map((section) => (
        <ReviewSection
          key={section.key}
          section={section}
          confirmed={confirmed[section.key]}
          onConfirm={() => onConfirm(section.key)}
          onUndo={() => onUndo(section.key)}
        >
          {section.key === "universe" ? <SymbolsField value={symbolsText} error={symbolsError} onChange={onSymbols} /> : null}
        </ReviewSection>
      ))}

      <NotEnforcedList draft={draft} />

      <section aria-labelledby="deploy-title" data-slot="deploy" className="grid gap-4 rounded-2xl bg-background px-5 py-5 sm:px-6">
        <div className="grid gap-1.5">
          <h2 id="deploy-title" className="text-h3">
            Deploy to paper
          </h2>
          <p className="max-w-measure text-sm text-pretty">It would trade simulated funds on your paper account. No real money moves.</p>
          <p role="status" className="text-sm text-muted-foreground" data-slot="confirmed-count">
            <span className="tabular">{done}</span> of <span className="tabular">{SECTION_KEYS.length}</span> sections confirmed.{" "}
            {ready ? "Every section is confirmed." : "Confirm each section to continue."}
          </p>
        </div>
        <div className="flex flex-col-reverse gap-3 sm:flex-row sm:items-center sm:justify-between">
          <BackButton onClick={onBack}>Change your answers</BackButton>
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} disabled={!ready} onClick={onDeploy}>
            Confirm and deploy to paper
          </Button>
        </div>
        <p className="text-caption text-pretty text-muted-foreground">In the product a passkey check comes next, and the journal keeps this screen as you saw it. This prototype skips both.</p>
      </section>
    </div>
  );
}
