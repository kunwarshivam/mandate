"use client";

import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { CheckCircle } from "@phosphor-icons/react";
import { FIELD } from "@/components/auth/buttons";
import { isPlatformAuthored } from "@/lib/labels";
import { type Draft, type DraftSection, MODELS, SECTION_KEYS, type SectionKey, type Strategy } from "./draft";
import { BackButton, StepHeading } from "./goal-steps";
import { ContractCard, DraftFields, NotEnforcedList } from "./mandate-parts";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

function SectionError({ id, error }: { id?: string; error: string | null }) {
  if (!error) return null;
  return (
    <p id={id} role="alert" data-slot="section-error" className="max-w-measure text-sm font-medium text-pretty">
      {error}
    </p>
  );
}

/**
 * One section of the draft with its own confirmation (brief A2: per section, never all at once,
 * PX-2). Focus follows the control that replaces the one pressed, so confirming never drops it. A
 * refused confirmation moves focus to the field that is wrong, or brings the reason into view when
 * no one field is, since the reason can sit far above the button that was pressed.
 */
function ReviewSection({
  section,
  confirmed,
  error,
  onConfirm,
  onUndo,
  children,
}: {
  section: DraftSection;
  confirmed: boolean;
  /** Why this section cannot be confirmed, when the owner tried and it cannot. */
  error?: string | null;
  /** False when the section cannot be confirmed yet. */
  onConfirm: () => boolean;
  onUndo: () => void;
  children?: ReactNode;
}) {
  const headingId = `review-${section.key}`;
  const root = useRef<HTMLElement>(null);
  const actions = useRef<HTMLDivElement>(null);
  const moveFocus = useRef(false);
  const [refusals, setRefusals] = useState(0);
  useEffect(() => {
    if (!moveFocus.current) return;
    moveFocus.current = false;
    actions.current?.querySelector("button")?.focus();
  }, [confirmed]);
  useEffect(() => {
    if (refusals === 0) return;
    const invalid = root.current?.querySelector<HTMLElement>("[aria-invalid=true]");
    if (invalid) invalid.focus();
    else root.current?.querySelector("[data-slot=section-error]")?.scrollIntoView({ block: "nearest" });
  }, [refusals]);
  const proposed = section.fields.filter((f) => f.provenance !== null && isPlatformAuthored(f.provenance)).length;

  return (
    <section ref={root} aria-labelledby={headingId} data-slot="review-section" data-section={section.key} data-confirmed={confirmed ? "true" : "false"} className="grid gap-(--block-gap)">
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
      <SectionError error={error ?? null} />
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
              const ok = onConfirm();
              moveFocus.current = ok;
              if (!ok) setRefusals((n) => n + 1);
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
        Separate them with commas or spaces. At most 20. One agent trades an instrument on an account.
      </p>
      <SectionError id={`${id}-error`} error={error} />
    </div>
  );
}

const MODEL_CHOICE =
  "press grid content-start gap-1 rounded-2xl border border-foreground/25 bg-card px-4 py-4 has-checked:border-foreground has-checked:bg-background has-focus-visible:ring-3 has-focus-visible:ring-ring";

/**
 * The model and its settings, the owner's to choose (brief A3): methodology only, listed in a fixed
 * order with no ranking and nothing recommended; nothing is preselected and every setting starts empty.
 */
function StrategyField({ strategy, error, onChange }: { strategy: Strategy; error: string | null; onChange: (next: Strategy) => void }) {
  const id = useId();
  const chosen = MODELS.find((m) => m.id === strategy.model);
  return (
    <div className="grid gap-5">
      <fieldset className="grid gap-2" aria-describedby={`${id}-models-hint`}>
        <legend className="field-label">Model</legend>
        <p id={`${id}-models-hint`} className="pb-1 text-sm text-pretty text-muted-foreground">
          In alphabetical order. The platform ranks none and recommends none.
        </p>
        <div className="grid gap-3 sm:grid-cols-2" data-slot="model-choices">
          {MODELS.map((m) => (
            <label key={m.id} className={MODEL_CHOICE} data-slot="model-choice">
              <span className="flex items-center gap-2.5">
                <input
                  type="radio"
                  name={`${id}-model`}
                  value={m.id}
                  checked={strategy.model === m.id}
                  onChange={() => onChange({ model: m.id, params: Object.fromEntries(m.params.map((p) => [p.key, strategy.params[p.key] ?? ""])) })}
                  className="size-4 accent-foreground outline-none"
                />
                <span className="text-base font-semibold">{m.name}</span>
              </span>
              <span className="text-sm text-pretty text-muted-foreground">{m.what}</span>
              <span className="font-mono text-caption text-muted-foreground" translate="no">
                {m.id} {m.version}
              </span>
            </label>
          ))}
        </div>
      </fieldset>
      {chosen
        ? chosen.params.map((p) => (
            <div key={p.key} className="grid gap-2">
              <label htmlFor={`${id}-${p.key}`} className="field-label">
                {p.label}
              </label>
              <input
                id={`${id}-${p.key}`}
                type="text"
                inputMode="decimal"
                autoComplete="off"
                spellCheck={false}
                value={strategy.params[p.key] ?? ""}
                aria-describedby={`${id}-${p.key}-hint`}
                onChange={(e) => onChange({ ...strategy, params: { ...strategy.params, [p.key]: e.target.value } })}
                className={cn(FIELD, "max-w-48")}
              />
              <p id={`${id}-${p.key}-hint`} className="max-w-measure text-sm text-pretty text-muted-foreground">
                {p.hint}
              </p>
            </div>
          ))
        : null}
      <SectionError error={error} />
    </div>
  );
}

export type SectionErrors = Partial<Record<SectionKey, string | null>>;

/**
 * A2, the compiled review: the contract card on top, every field grouped by section with its
 * provenance, what is not enforced listed apart, and one primary action, enabled only once every
 * section is confirmed, that opens the confirmation record (A5).
 */
export function CompiledReview({
  draft,
  confirmed,
  errors,
  symbolsText,
  strategy,
  onSymbols,
  onStrategy,
  onConfirm,
  onUndo,
  onBack,
  onContinue,
}: {
  draft: Draft;
  confirmed: Record<SectionKey, boolean>;
  errors: SectionErrors;
  symbolsText: string;
  strategy: Strategy;
  onSymbols: (value: string) => void;
  onStrategy: (next: Strategy) => void;
  onConfirm: (key: SectionKey) => boolean;
  onUndo: (key: SectionKey) => void;
  onBack: () => void;
  onContinue: () => void;
}) {
  const done = SECTION_KEYS.filter((k) => confirmed[k]).length;
  const ready = done === SECTION_KEYS.length;
  const extra = (key: SectionKey): ReactNode => {
    switch (key) {
      case "universe":
        return <SymbolsField value={symbolsText} error={errors.universe ?? null} onChange={onSymbols} />;
      case "strategy":
        return <StrategyField strategy={strategy} error={errors.strategy ?? null} onChange={onStrategy} />;
      case "money":
      case "limits":
      case "autonomy":
        return null;
      default: {
        const unhandled: never = key;
        throw new Error(`unhandled section ${String(unhandled)}`);
      }
    }
  };
  return (
    <div className="grid gap-(--section-gap)">
      <div className="grid gap-3">
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
          error={section.key === "universe" || section.key === "strategy" ? null : errors[section.key]}
          onConfirm={() => onConfirm(section.key)}
          onUndo={() => onUndo(section.key)}
        >
          {extra(section.key)}
        </ReviewSection>
      ))}

      <NotEnforcedList draft={draft} />

      <section aria-labelledby="deploy-title" data-slot="deploy" className="grid gap-4 rounded-2xl bg-background px-5 py-5 sm:px-6">
        <div className="grid gap-1.5">
          <h2 id="deploy-title" className="text-h3">
            Deploy to paper
          </h2>
          <p className="max-w-measure text-sm text-pretty">
            Next you read the whole mandate once more, exactly as the journal will keep it, and confirm it with your passkey. Then it trades simulated funds on your paper account.
          </p>
          <p role="status" className="text-sm text-muted-foreground" data-slot="confirmed-count">
            <span className="tabular">{done}</span> of <span className="tabular">{SECTION_KEYS.length}</span> sections confirmed.{" "}
            {ready ? "Every section is confirmed." : "Confirm each section to continue."}
          </p>
        </div>
        <div className="flex flex-col-reverse gap-3 sm:flex-row sm:items-center sm:justify-between">
          <BackButton onClick={onBack}>Change your answers</BackButton>
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} disabled={!ready} onClick={onContinue}>
            Continue to confirm
          </Button>
        </div>
      </section>
    </div>
  );
}
