"use client";

import { useEffect, useState } from "react";
import { type AdmissionReason, type Thesis, thesesOf } from "@/api/theses";
import { Section } from "@/components/screens/common";
import { clock } from "@/lib/format";
import { type RecordsAnswer, useRuntime } from "@/lib/mock-runtime";

/** How many of the agent stream's latest records the view reads: the page's most (§4.8.1). */
export const THESIS_RECORDS = 1000;
/** How often the theses are read again while the screen is open. */
const THESES_POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";

/** Mandate spec §8.5's first failing check, in words: why the admission refused a thesis. */
const REFUSED: Readonly<Record<AdmissionReason, string>> = {
  direction_not_allowed: "it was not long, the only direction allowed",
  horizon_mismatch: "its expiry did not match its horizon",
  revision_without_predecessor: "it named a thesis to revise that did not match its revision",
  research_disabled: "research is off in the mandate",
  universe_pinned: "the mandate pins the instruments",
  admission_denied: "the mandate denies new instruments",
  cost_cap_reached: "the day's research spend cap was reached",
  not_in_data_universe: "the instrument is outside the data universe",
  operator_halt: "the platform halted theses on this instrument",
  not_allowed_asset_class: "the mandate does not allow its asset class",
  leveraged_etp_not_enabled: "it is a leveraged or inverse ETP, which the mandate does not enable",
  eligibility_floor: "the instrument is below the eligibility floor",
  instrument_group_claimed: "another agent on the account trades this instrument group",
  source_not_allowlisted: "it cited a source not on the allowlist",
  no_corroboration: "no independent source or market data corroborated it",
  lineage_retired: "its lineage is retired",
  universe_full: "the working universe is full",
};

/** A journaled time with its date. */
const dated = (at: string) => `${at.slice(0, 10)} ${clock(at)} UTC`;

/** Where a thesis stands, in words, never by its code. */
export function thesisWords(t: Thesis): string {
  switch (t.state) {
    case "current":
      return `Admitted. Current until ${dated(t.expires_at)}.`;
    case "rejected":
      return `Rejected: ${t.reason ? REFUSED[t.reason] : "reason not reported"}.`;
    case "invalidated":
      return `Invalidated ${dated(t.ended_at ?? t.recorded_at)}: ${t.cause === "source_revoked" ? "a source it cites was revoked" : "the research agent judged its invalidation condition met"}.`;
    case "expired":
      return `Expired ${dated(t.ended_at ?? t.expires_at)}.`;
    case "lineage_retired":
      return `Ended ${dated(t.ended_at ?? t.recorded_at)}: its lineage was retired.`;
    case "replaced":
      return `Replaced ${dated(t.ended_at ?? t.recorded_at)} by a later thesis on the instrument.`;
  }
}

/**
 * The research agent's theses (journal spec §9.4, §9.22; DEC-2391 item 2), from its stream's latest
 * records with their bodies, each checked against its hash. An agent whose records hold no thesis
 * shows nothing here; a record that could not be verified is counted, never shown as a thesis.
 */
export function AgentTheses({ agentId }: { agentId: string }) {
  const { streamRecords, workspaceId, now } = useRuntime();
  const [answer, setAnswer] = useState<RecordsAnswer | null>(null);
  useEffect(() => {
    if (!streamRecords || !workspaceId) return;
    let live = true;
    const load = () => void streamRecords(`agent:${workspaceId}:${agentId}`, THESIS_RECORDS).then((next) => live && setAnswer(next));
    load();
    const poll = window.setInterval(load, THESES_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [streamRecords, workspaceId, agentId]);
  if (!answer) return null;
  if (!answer.ok)
    return answer.reason === "forbidden" ? null : (
      <Section title="Theses">
        <p className={NOT_REPORTED}>Theses not reported.</p>
      </Section>
    );
  const view = thesesOf(answer.items, now);
  if (view.theses.length === 0 && view.unverified === 0) return null;
  return (
    <Section title="Theses">
      <p className="text-sm text-muted-foreground">What the research agent proposed and the admission&rsquo;s verdict on each, newest first, from the latest records on its stream.</p>
      {view.unverified > 0 ? (
        <p className="text-sm font-medium" data-slot="theses-unverified">
          {view.unverified === 1 ? "1 thesis record" : `${view.unverified} thesis records`} could not be verified against the journal and {view.unverified === 1 ? "is" : "are"} not shown.
        </p>
      ) : null}
      {view.theses.length > 0 ? (
        <ol className="grid" aria-label="Theses">
          {view.theses.map((t) => (
            <li key={t.thesis_id} className="grid gap-1 border-b border-border/70 py-3.5 last:border-b-0" data-state={t.state}>
              <span>
                <span className="font-medium">{t.instrument_id}</span>, {t.direction}
                {t.revision > 0 ? `, revision ${t.revision}` : ""}
              </span>
              <span className="text-sm">{thesisWords(t)}</span>
              <span className="text-sm text-muted-foreground">Judged {dated(t.recorded_at)}</span>
            </li>
          ))}
        </ol>
      ) : null}
    </Section>
  );
}
