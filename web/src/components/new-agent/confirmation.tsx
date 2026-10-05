"use client";

import type { ReactNode } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowCounterClockwise } from "@phosphor-icons/react";
import { Owl } from "@/components/domain/owl";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { clock, dateLabel, zoneLabel } from "@/lib/format";
import { type Draft, SECTION_KEYS, draftDigest } from "./draft";
import { PrototypeNote, STEP_HEADING, StepHeading } from "./goal-steps";
import { ContractCard, DraftFields, NotEnforcedList } from "./mandate-parts";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

/** The agent the prototype would have created: a fixed fixture ID, so its owl is always the same one. */
export const PROTOTYPE_AGENT_ID = "agt_01JB4N3W5Q8RZ2X6C9V7B1T3MK";

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="grid gap-0.5 border-b border-border/70 py-3 last:border-b-0 sm:grid-cols-[minmax(0,11rem)_minmax(0,1fr)] sm:gap-x-6">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="min-w-0 wrap-anywhere">{children}</dd>
    </div>
  );
}

/**
 * A5, as the record the owner would keep (brief §4.1): what they confirmed, when, under which
 * version, everything expanded. The agent's owl appears here for the first time. The prototype says
 * plainly that it saved nothing, and offers a way to start over.
 */
export function Confirmation({ draft, confirmedAt, onStartOver }: { draft: Draft; confirmedAt: string; onStartOver: () => void }) {
  return (
    <article aria-labelledby={STEP_HEADING} data-slot="new-agent-record" className="grid gap-(--section-gap)">
      <header className="grid gap-5">
        <PrototypeNote />
        <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
          <Owl seed={PROTOTYPE_AGENT_ID} mood="awake" className="size-16 sm:size-20" />
          <div className="grid min-w-0 gap-1.5">
            <StepHeading className="flex flex-wrap items-center gap-3">
              Mandate confirmed
              <EnvironmentBadge environment="paper" />
            </StepHeading>
            <p className="text-pretty text-muted-foreground">This is your agent&apos;s owl. Every agent gets its own, drawn from its ID.</p>
          </div>
        </div>
        <p role="note" data-slot="nothing-saved" className="max-w-measure rounded-2xl bg-background px-5 py-4 font-medium text-pretty">
          This prototype saved nothing. No agent exists, no mandate version was created, nothing was sent to a broker and nothing will trade. Leaving this page forgets your
          answers.
        </p>
      </header>

      <section aria-labelledby="record-facts" className="grid gap-(--block-gap)">
        <h2 id="record-facts" className="text-h2">
          The record
        </h2>
        <dl className="grid">
          <Fact term="Confirmed">
            <span className="tabular">
              {dateLabel(confirmedAt)}, {clock(confirmedAt)} {zoneLabel(confirmedAt)}
            </span>
          </Fact>
          <Fact term="Version">
            <span className="grid gap-0.5">
              <span>
                Version <span className="tabular">1</span>, draft{" "}
                <span className="font-mono tabular" translate="no">
                  {draftDigest(draft)}
                </span>
              </span>
              <span className="text-caption text-muted-foreground">A name for this draft on this screen only. No version was stored.</span>
            </span>
          </Fact>
          <Fact term="Environment">Paper: simulated funds, no real money.</Fact>
          <Fact term="Sections confirmed">
            <span className="tabular">
              {SECTION_KEYS.length} of {SECTION_KEYS.length}
            </span>
            , each by you, one at a time.
          </Fact>
        </dl>
      </section>

      <ContractCard draft={draft} />

      <section aria-labelledby="record-fields" className="grid gap-(--section-gap)">
        <div className="grid gap-1.5">
          <h2 id="record-fields" className="text-h2">
            What you confirmed
          </h2>
          <p className="max-w-measure text-muted-foreground">Every field, with who wrote it, exactly as you saw it.</p>
        </div>
        {draft.sections.map((section) => (
          <section key={section.key} aria-labelledby={`record-${section.key}`} data-slot="record-section" className="grid gap-2">
            <h3 id={`record-${section.key}`} className="text-h3">
              {section.title}
            </h3>
            <DraftFields fields={section.fields} confirmed />
          </section>
        ))}
      </section>

      <NotEnforcedList draft={draft} />

      <div className="grid justify-items-start gap-3 border-t border-border/70 pt-6">
        <p className="max-w-measure text-sm text-pretty text-muted-foreground">Start over to try other answers. Nothing from this run is kept.</p>
        <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} onClick={onStartOver}>
          <ArrowCounterClockwise className="size-4" aria-hidden />
          Start over
        </Button>
      </div>
    </article>
  );
}
