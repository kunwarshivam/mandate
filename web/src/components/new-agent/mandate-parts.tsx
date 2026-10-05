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

/**
 * The contract card (brief A2, DEC-182): the whole envelope in plain words on the mandate field,
 * the owner's answers quoted, the loss figures in dollars with who drafted each, and the unasked
 * dollars (DEC-189). A rendering of the fields, not a separate object.
 */
export function ContractCard({ draft, headingLevel = 2 }: { draft: Draft; headingLevel?: 2 | 3 }) {
  const t = draft.terms;
  const f = draft.figures;
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const nothingUnasked = f.unasked === ZERO;
  return (
    <section aria-labelledby="contract-title" data-slot="contract-card" className="grid gap-6 rounded-2xl bg-mandate px-5 py-6 text-mandate-foreground sm:px-7">
      <div className="grid gap-1.5">
        <Heading id="contract-title" className="text-h2 text-mandate-strong">
          Your mandate in plain words
        </Heading>
        <p className="max-w-measure text-sm text-mandate-muted">The fields below, read as sentences. Anything marked as proposed is not active until you confirm its section.</p>
      </div>

      <ul className="grid max-w-measure gap-2.5 text-base text-pretty" data-slot="contract-terms">
        <li>
          This agent may use <span className="font-mono font-medium tabular">{usd(t.allocationUsd)}</span> of simulated money, on paper.
        </li>
        <li>{goalWords(t)}</li>
        <li>
          If it loses <span className="font-mono font-medium tabular">{usd(t.maxLossUsd)}</span> in total, at{" "}
          <span className="font-mono tabular">{usd(sub(t.allocationUsd, t.maxLossUsd))}</span>, it closes everything and pauses until you change the mandate.
        </li>
        <li>It asks you before every buy. A buy you do not answer is skipped.</li>
        <li>{draft.symbols.length > 0 ? `It trades only ${draft.symbols.join(", ")}.` : "It trades only the instruments you list, and you have not listed any yet."}</li>
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
        <p className="text-label text-mandate-muted">Unasked dollars: what could trade without asking you once you confirm</p>
        <Money value={f.unasked} className="w-fit text-figure" />
        <p className="max-w-measure text-sm text-pretty">
          {nothingUnasked
            ? "No buy runs without your answer: nothing in this mandate is set to go ahead without asking, and the platform never proposes that. Only you can set it, later, and this figure would show how much it lets through."
            : "The most the agent could buy today without asking you, under the rules set to go ahead without an answer."}{" "}
          Selling to cut risk never waits for you.
        </p>
      </div>

      <div className="grid gap-1">
        <p className="text-label text-mandate-muted">Losses in dollars</p>
        <dl data-slot="loss-figures" className="grid">
          <FigureRow label="One full position stopped out" value={f.positionLossAtStop} provenance="platform_proposed" />
          <FigureRow label="Most it may lose in one day" value={f.dailyLossBudget} provenance="platform_proposed" />
          <FigureRow label="Fall at which it closes everything" value={f.flattenLoss} provenance="platform_proposed" />
          <FigureRow label="Most it may lose, in total" value={f.floorLoss} provenance="user_stated" />
        </dl>
        <p className="pt-1 text-caption text-pretty text-mandate-muted">Price gaps and exit prices can make any of these losses larger. You stated only the last one; the platform worked out the others from it.</p>
      </div>
    </section>
  );
}

/** Constraints in the owner's words that no mandate field can express (spec §7), listed apart from the fields. */
export function NotEnforcedList({ draft, headingLevel = 2 }: { draft: Draft; headingLevel?: 2 | 3 }) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return (
    <section aria-labelledby="not-enforced-title" data-slot="not-enforced" className="grid gap-(--block-gap)">
      <div className="grid gap-1.5">
        <Heading id="not-enforced-title" className="text-h2">
          Not enforced
        </Heading>
        <p className="max-w-measure text-muted-foreground">Things you said that no limit can check. They reach the agent&apos;s models only as description text.</p>
      </div>
      {draft.notEnforced.length === 0 ? (
        <p className="text-sm text-muted-foreground">Nothing. Everything you said is a field above.</p>
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
