"use client";

import { type ReactNode, useId } from "react";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { ContentRef, Provenance } from "@/fixtures/types";
import { type Dec, sub } from "@/lib/decimal";
import { type NewAgent, mandateVersion } from "@/lib/fixture-journey";
import { PROVENANCE_LABEL } from "@/lib/labels";
import { usd } from "@/lib/format";
import type { SetUp } from "@/lib/mock-runtime";
import { type Draft, goalWords } from "./draft";

/**
 * The whole agent, shown once in the conversation when nothing is missing (DEC-477). It is the
 * confirmation record (brief A5, mandate §10): every value in compact form, nothing collapsed (PX-1),
 * what the platform drafted marked as such, and the version its confirmation binds. Under it, three
 * steps set it up, each recorded on its own (DEC-2401): create the version, confirm it with a passkey
 * (G3), whose record keeps every line shown, and deploy it to paper with a passkey.
 */

export const SUMMARY_TITLE = "Your agent";
export const LEGEND = "Marked Proposed or Default: set by Owlhead to fit inside the loss you gave. Everything else came from you.";
export const GAP_NOTE = "Price gaps and exit prices can make any of these losses larger. You gave the last one; Owlhead worked out the others from it.";
const NOT_ENFORCED_WHY = "No limit can check this, so it isn't enforced. The agent gets it as a note.";

/** The same words as the mandate page's provenance badges (DEC-513), on what the owner did not say. */
const TAG: Record<Provenance, string | null> = {
  user_stated: null,
  user_entered: null,
  template_structure: PROVENANCE_LABEL.template_structure,
  platform_proposed: PROVENANCE_LABEL.platform_proposed,
  platform_default: PROVENANCE_LABEL.platform_default,
};

/** The agent in plain sentences, one per line. */
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

interface Row {
  label: string;
  value: string;
  provenance: Provenance | null;
}

/** The losses in dollars, with who drafted each, and what could buy without asking (DEC-189). */
function dollarRows(draft: Draft): Row[] {
  const f = draft.figures;
  const row = (label: string, value: Dec, provenance: Provenance): Row => ({ label, value: usd(value), provenance });
  return [
    row("One full position stopped out", f.positionLossAtStop, "platform_proposed"),
    row("Most it may lose in one day", f.dailyLossBudget, "platform_proposed"),
    row("Fall at which it closes everything", f.flattenLoss, "platform_proposed"),
    row("Most it may lose, in total", f.floorLoss, "user_stated"),
    { label: "Could buy without asking you", value: usd(f.unasked), provenance: null },
  ];
}

const rowLine = (r: Row) => `${r.label}: ${r.value}${r.provenance && TAG[r.provenance] ? ` (${TAG[r.provenance]})` : ""}`;

/** Every line the summary shows, in order, as the journal keeps it with `MandateConfirmed` (brief §4.1, mandate §10). */
export function summaryLines(draft: Draft, version: ContentRef): string[] {
  return [
    SUMMARY_TITLE,
    "PAPER: simulated funds",
    LEGEND,
    ...contractTerms(draft),
    "Losses in dollars",
    ...dollarRows(draft).map(rowLine),
    GAP_NOTE,
    "Every setting",
    ...draft.sections.flatMap((s) => [s.title, ...s.fields.map((f) => rowLine(f))]),
    ...(draft.notEnforced.length === 0 ? [] : ["Not enforced", ...draft.notEnforced.flatMap((n) => [`“${n.quote}”`, NOT_ENFORCED_WHY])]),
    `Version 1: ${version}`,
  ];
}

/** Every path the summary shows, each one the owner confirms (journal rule 18), in order and once. */
export function confirmedPaths(draft: Draft): string[] {
  return [...new Set(draft.sections.flatMap((s) => s.fields.map((f) => f.path)))];
}

/** The confirmation the first passkey check authorizes, in words. */
export function confirmAction(draft: Draft): string {
  return `Confirm version 1 of this agent: ${usd(draft.terms.allocationUsd)} of simulated money on paper, trading ${draft.symbols.join(", ")}.`;
}

/** The deployment the second passkey check authorizes, in words. */
export function deployAction(draft: Draft): string {
  return `Deploy version 1 to paper with ${usd(draft.terms.allocationUsd)} of simulated money, trading ${draft.symbols.join(", ")}.`;
}

function Rows({ rows }: { rows: Row[] }) {
  return (
    <dl className="grid">
      {rows.map((r) => {
        const tag = r.provenance ? TAG[r.provenance] : null;
        return (
          <div key={r.label} data-slot="summary-row" data-provenance={r.provenance ?? undefined} className="grid gap-x-4 gap-y-0.5 border-b border-mandate-strong/10 py-2 last:border-b-0 sm:grid-cols-[minmax(0,12rem)_minmax(0,1fr)]">
            <dt className="text-sm text-mandate-muted">{r.label}</dt>
            <dd className="min-w-0 text-sm text-pretty tabular wrap-anywhere">
              {r.value}
              {tag ? (
                <span data-slot="tag" className="ml-2 inline-block rounded-sm border border-dashed border-mandate-strong/40 px-1.5 text-caption text-mandate-muted">
                  {tag}
                </span>
              ) : null}
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

function Part({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="grid gap-1">
      <h3 className="text-label text-mandate-muted">{title}</h3>
      {children}
    </div>
  );
}

/** The summary card: the record the confirmation keeps, bound to the version it shows. */
export function Summary({ draft, request, setUp }: { draft: Draft; request: NewAgent; setUp: SetUp | null }) {
  const headingId = useId();
  const version = setUp?.version ?? mandateVersion(request.mandate);

  return (
    <section aria-labelledby={headingId} data-slot="summary" className="grid gap-5 rounded-2xl bg-mandate px-5 py-5 text-mandate-foreground sm:px-6">
      <div className="grid gap-1">
        <div className="flex flex-wrap items-center gap-3">
          <h2 id={headingId} className="text-h3 text-mandate-strong">
            {SUMMARY_TITLE}
          </h2>
          <EnvironmentBadge environment="paper" />
        </div>
        <p className="text-caption text-pretty text-mandate-muted">{LEGEND}</p>
      </div>

      <ul className="grid gap-1.5 text-pretty tabular" data-slot="terms">
        {contractTerms(draft).map((term) => (
          <li key={term}>{term}</li>
        ))}
      </ul>

      <Part title="Losses in dollars">
        <Rows rows={dollarRows(draft)} />
        <p className="pt-1 text-caption text-pretty text-mandate-muted">{GAP_NOTE}</p>
      </Part>

      <Part title="Every setting">
        <div className="grid gap-3">
          {draft.sections.map((s) => (
            <div key={s.key} data-slot="summary-section" data-section={s.key} className="grid">
              <h4 className="pt-1 text-sm font-semibold">{s.title}</h4>
              <Rows rows={s.fields} />
            </div>
          ))}
        </div>
      </Part>

      {draft.notEnforced.length > 0 ? (
        <Part title="Not enforced">
          <ul className="grid gap-2" data-slot="not-enforced">
            {draft.notEnforced.map((n) => (
              <li key={n.quote} data-slot="not-enforced-item" className="grid gap-0.5">
                <span className="text-sm text-pretty wrap-anywhere">“{n.quote}”</span>
                <span className="text-caption text-pretty text-mandate-muted">{NOT_ENFORCED_WHY}</span>
              </li>
            ))}
          </ul>
        </Part>
      ) : null}

      <p className="text-caption text-mandate-muted">
        Version 1:{" "}
        <span className="font-mono wrap-anywhere" translate="no" data-slot="mandate-version">
          {version}
        </span>
      </p>
    </section>
  );
}
