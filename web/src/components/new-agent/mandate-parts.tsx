"use client";

import { Money } from "@/components/domain/money";
import { ProvenanceBadge } from "@/components/domain/provenance-badge";
import type { Provenance } from "@/fixtures/types";
import { type Dec, ZERO, sub } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { isPlatformAuthored } from "@/lib/labels";
import { cn } from "@/lib/utils";
import { type Draft, type DraftField, goalWords } from "./draft";

/** The owner's own words, quoted back exactly as written. */
export function QuotedSpan({ quote, className }: { quote: string; className?: string }) {
  return (
    <span data-slot="quoted-span" className={cn("text-caption text-pretty text-muted-foreground", className)}>
      You said “{quote}”
    </span>
  );
}

/**
 * A section's fields, each with who wrote it. A value the platform drafted stays muted, beside its
 * dashed badge and "Not active yet", until the owner confirms its section (brief P2, mandate §2.1).
 */
export function DraftFields({ fields, confirmed }: { fields: DraftField[]; confirmed: boolean }) {
  return (
    <dl className="grid">
      {fields.map((f) => {
        const inactive = f.provenance !== null && isPlatformAuthored(f.provenance) && !confirmed;
        return (
          <div
            key={f.path}
            data-slot="draft-field"
            data-path={f.path}
            data-provenance={f.provenance ?? undefined}
            data-active={inactive ? "false" : "true"}
            className="grid gap-1.5 border-b border-border/70 py-(--row-y) last:border-b-0 sm:grid-cols-[minmax(0,11rem)_minmax(0,1fr)] sm:gap-x-6"
          >
            <dt className="text-sm text-muted-foreground">{f.label}</dt>
            <dd className="grid min-w-0 justify-items-start gap-1.5 wrap-anywhere">
              <span className={cn("text-pretty tabular transition-colors duration-(--duration-hover)", inactive ? "text-muted-foreground" : "text-foreground")}>
                {f.value}
                {inactive ? <span className="sr-only"> (proposed, not active until you confirm this section)</span> : null}
              </span>
              {f.provenance ? (
                <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <ProvenanceBadge provenance={{ path: f.path, provenance: f.provenance, quote: f.quote }} />
                  {inactive ? (
                    <span aria-hidden className="text-caption text-muted-foreground">
                      Not active yet
                    </span>
                  ) : null}
                </span>
              ) : null}
              {f.quote ? <QuotedSpan quote={f.quote} /> : null}
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

function FigureRow({ label, value, provenance }: { label: string; value: Dec; provenance: Provenance }) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-4 gap-y-1 border-b border-mandate-strong/15 py-2.5 last:border-b-0">
      <dt className="text-sm">{label}</dt>
      <dd className="font-mono font-medium tabular">{usd(value)}</dd>
      <dd className="col-span-2">
        <ProvenanceBadge provenance={{ path: label, provenance }} />
      </dd>
    </div>
  );
}

/** The contract card's sentences, one per line, as the card shows them and the A5 record keeps them. */
export function contractTerms(draft: Draft): string[] {
  const t = draft.terms;
  const model = draft.strategy ? `It decides with ${draft.strategy.model.name.toLowerCase()} (${draft.strategy.model.id}), with the settings you chose.` : "It decides with the model you choose, and you have not chosen one yet.";
  return [
    `This agent may use ${usd(t.allocationUsd)} of simulated money, on paper.`,
    goalWords(t),
    `If it loses ${usd(t.maxLossUsd)} in total, at ${usd(sub(t.allocationUsd, t.maxLossUsd))}, it closes everything and pauses until you change the mandate.`,
    model,
    "It asks you before every buy. A buy you do not answer is skipped.",
    draft.symbols.length > 0 ? `It trades only ${draft.symbols.join(", ")}.` : "It trades only the instruments you list, and you have not listed any yet.",
  ];
}

/** The unasked-dollars note under the figure, as shown. */
export function unaskedNote(draft: Draft): string {
  const lead =
    draft.figures.unasked === ZERO
      ? "No buy runs without your answer: nothing in this mandate is set to go ahead without asking, and the platform never proposes that. Only you can set it, later, and this figure would show how much it lets through."
      : "The most the agent could buy today without asking you, under the rules set to go ahead without an answer.";
  return `${lead} Selling to cut risk never waits for you.`;
}

/** The loss figures in dollars, with who drafted each: the owner stated only the last. */
export function lossFigures(draft: Draft): Array<{ label: string; value: Dec; provenance: Provenance }> {
  const f = draft.figures;
  return [
    { label: "One full position stopped out", value: f.positionLossAtStop, provenance: "platform_proposed" },
    { label: "Most it may lose in one day", value: f.dailyLossBudget, provenance: "platform_proposed" },
    { label: "Fall at which it closes everything", value: f.flattenLoss, provenance: "platform_proposed" },
    { label: "Most it may lose, in total", value: f.floorLoss, provenance: "user_stated" },
  ];
}

export const CONTRACT_TITLE = "Your mandate in plain words";
export const CONTRACT_LEAD = "The fields below, read as sentences. Anything marked as proposed is not active until you confirm its section.";
export const CONTRACT_LEAD_CONFIRMED = "The fields below, read as sentences. You confirmed every section.";
export const UNASKED_LABEL = "Unasked dollars: what could trade without asking you once you confirm";
export const GAP_NOTE = "Price gaps and exit prices can make any of these losses larger. You stated only the last one; the platform worked out the others from it.";

/**
 * The contract card (brief A2, DEC-182): the whole envelope in plain words on the mandate field,
 * the owner's answers quoted, the loss figures in dollars with who drafted each, and the unasked
 * dollars (DEC-189). A rendering of the fields, not a separate object.
 */
export function ContractCard({ draft, confirmed = false }: { draft: Draft; confirmed?: boolean }) {
  return (
    <section aria-labelledby="contract-title" data-slot="contract-card" className="grid gap-6 rounded-2xl bg-mandate px-5 py-6 text-mandate-foreground sm:px-7">
      <div className="grid gap-1.5">
        <h2 id="contract-title" className="text-h2 text-mandate-strong">
          {CONTRACT_TITLE}
        </h2>
        <p className="max-w-measure text-sm text-mandate-muted">{confirmed ? CONTRACT_LEAD_CONFIRMED : CONTRACT_LEAD}</p>
      </div>

      <ul className="grid max-w-measure gap-2.5 text-base text-pretty tabular" data-slot="contract-terms">
        {contractTerms(draft).map((term) => (
          <li key={term}>{term}</li>
        ))}
      </ul>

      <div className="grid gap-2">
        <p className="text-label text-mandate-muted">{draft.answers.length === 1 ? "Your words" : "Your three answers"}</p>
        <ul className="grid gap-2">
          {draft.answers.map((a) => (
            <li key={a.label} className="grid gap-0.5 border-l-2 border-mandate-marker pl-3">
              <span className="text-caption text-mandate-muted">{a.label}</span>
              <span className="text-pretty wrap-anywhere">“{a.quote}”</span>
            </li>
          ))}
        </ul>
      </div>

      <div data-slot="unasked" className="grid gap-1.5 border-y border-mandate-strong/15 py-4">
        <p className="text-label text-mandate-muted">{UNASKED_LABEL}</p>
        <Money value={draft.figures.unasked} className="w-fit text-figure" />
        <p className="max-w-measure text-sm text-pretty">{unaskedNote(draft)}</p>
      </div>

      <div className="grid gap-1">
        <p className="text-label text-mandate-muted">Losses in dollars</p>
        <dl data-slot="loss-figures" className="grid">
          {lossFigures(draft).map((figure) => (
            <FigureRow key={figure.label} {...figure} />
          ))}
        </dl>
        <p className="pt-1 text-caption text-pretty text-mandate-muted">{GAP_NOTE}</p>
      </div>
    </section>
  );
}

export const NOT_ENFORCED_LEAD = "Things you said that no limit can check. They reach the agent's models only as description text.";
export const NOT_ENFORCED_EMPTY = "Nothing. Everything you said is a field above.";

/** Constraints in the owner's words that no mandate field can express (spec §7), listed apart from the fields. */
export function NotEnforcedList({ draft }: { draft: Draft }) {
  return (
    <section aria-labelledby="not-enforced-title" data-slot="not-enforced" className="grid gap-(--block-gap)">
      <div className="grid gap-1.5">
        <h2 id="not-enforced-title" className="text-h2">
          Not enforced
        </h2>
        <p className="max-w-measure text-muted-foreground">{NOT_ENFORCED_LEAD}</p>
      </div>
      {draft.notEnforced.length === 0 ? (
        <p className="text-sm text-muted-foreground">{NOT_ENFORCED_EMPTY}</p>
      ) : (
        <ul className="grid rounded-2xl bg-background px-5">
          {draft.notEnforced.map((n) => (
            <li key={n.quote} data-slot="not-enforced-item" className="grid gap-1 border-b border-border/70 py-(--row-y) last:border-b-0">
              <span className="text-pretty wrap-anywhere">“{n.quote}”</span>
              <span className="text-caption text-pretty text-muted-foreground">{n.why}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
