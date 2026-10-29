"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import type { ConfirmedVersion, MandateVersions, RejectionReason } from "@/api/mandate-versions";
import { ClassTag } from "@/components/screens/mandate-versions";
import { Section, WorkspaceGate } from "@/components/screens/common";
import { clock } from "@/lib/format";
import { type VersionsAnswer, useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";

/** How often the history is read again while the screen is open: an application lands on its own. */
export const VERSIONS_POLL_MS = 10_000;
const NOT_REPORTED = "text-muted-foreground";
const TAG = "inline-flex h-6 w-fit shrink-0 items-center rounded-md px-2.5 text-label whitespace-nowrap";

/** Mandate spec §5.1 and §5.7's reasons, in words: why the executor refused a version. */
export const REJECTED: Readonly<Record<RejectionReason, string>> = {
  increase_blocked_while_latched: "it added risk while a limit was latched",
  equity_below_exposure: "the agent's equity after the change would have been below what it holds and has working",
  would_trigger_limit: "applying it would have triggered a limit at once",
  not_loosening: "it did not loosen the lifetime loss floor, which only a loosening version lifts",
  waiting_period: "a single-user workspace waits a full risk day after the confirmation day before loosening a latched floor",
  still_below_new_floor: "the agent's equity would still have been at or below the new floor",
};

/** A journaled time with its date, in UTC. */
const dated = (at: string) => `${at.slice(0, 10)} ${clock(at)} UTC`;

function Hash({ version }: { version: string }) {
  return <span className="font-mono">{version.slice(7, 19)}</span>;
}

function Times({ created, confirmed }: { created: string | null; confirmed: string | null }) {
  return (
    <p className="text-caption text-muted-foreground">
      {created ? `Created ${dated(created)}.` : "Creation not reported."} {confirmed ? `Confirmed by you ${dated(confirmed)}.` : "Confirmation not reported."}
    </p>
  );
}

/** What the executor did with a later version, or why it cannot be told. */
function applicationWords(v: ConfirmedVersion, history: MandateVersions): string {
  const a = v.application;
  if (a?.result === "applied") return `Applied ${dated(a.recorded_at)}.`;
  if (a?.result === "rejected") return `Not applied: the agent refused it ${dated(a.recorded_at)}, because ${REJECTED[a.reason]}. The version before it stayed in effect.`;
  if (history.account_ref === null) return "Whether it applied is not reported: your deployment did not find the account's record.";
  if (history.stopped_at !== null) return "Not applied before the agent stopped.";
  return v.classification === "risk_increasing"
    ? "Not applied yet: a risk-increasing version applies at the agent's next check with no order in an unknown state."
    : "Not applied yet.";
}

/**
 * The agent's mandate versions as its deployment serves them (§4.7, DEC-2147), newest first: each
 * with its classification as the server computed it, its times, and what the executor did with it.
 * Only the executor's application puts a version in effect, so the tag follows `in_effect` alone,
 * on the latest entry naming it; nothing here works a classification or a state out.
 */
export function VersionHistory({ history }: { history: MandateVersions }) {
  const entries = [history.deployed, ...history.later];
  const current = history.in_effect === null ? -1 : entries.findLastIndex((v) => v.mandate_version === history.in_effect && (v.kind === "deployed" || v.application?.result === "applied"));
  return (
    <div className="grid max-w-3xl gap-(--block-gap)">
      {history.stopped_at ? (
        <p className="text-sm font-medium" data-slot="versions-stopped">
          The agent stopped {dated(history.stopped_at)}: no version is in effect.
        </p>
      ) : history.account_ref === null ? (
        <p className="text-sm font-medium" data-slot="versions-no-account">
          Which version is in effect is not reported: your deployment did not find the account&rsquo;s record.
        </p>
      ) : null}
      <ol data-slot="versions" className="grid divide-y divide-border/70">
        {entries
          .map((v, index) => ({ v, index }))
          .reverse()
          .map(({ v, index }) => (
            <li key={`${index}:${v.mandate_version}`} data-slot="version" data-current={index === current || undefined} className="grid gap-2 py-5 first:pt-2">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                <h3 className="text-h3">{v.kind === "deployed" ? "Deployed version" : `Change ${index}`}</h3>
                {index === current ? <span className={cn(TAG, "bg-mandate text-mandate-strong ring-1 ring-mandate-edge ring-inset")}>In effect</span> : null}
                {v.kind === "confirmed" && v.application?.result === "rejected" ? <span className={cn(TAG, "bg-background text-foreground")}>Not applied</span> : null}
                {v.kind === "confirmed" ? v.classification ? <ClassTag value={v.classification} /> : <span className={cn("text-sm", NOT_REPORTED)}>Classification not reported</span> : null}
              </div>
              <p className="text-caption text-muted-foreground">
                Hash <Hash version={v.mandate_version} />
                {v.kind === "confirmed" ? (
                  <>
                    , over <Hash version={v.base_version} />
                  </>
                ) : null}
              </p>
              <Times created={v.created_at} confirmed={v.confirmed_at} />
              <p className="max-w-measure text-sm" data-slot="version-application">
                {v.kind === "deployed" ? `Deployed ${dated(v.deployed_at)}.` : applicationWords(v, history)}
              </p>
            </li>
          ))}
      </ol>
    </div>
  );
}

/** The versions screen over the API: the history read on open and every ten seconds, "not reported" when the read fails. */
export function ApiMandateVersions({ agentId }: { agentId: string }) {
  const { mandateVersions } = useRuntime();
  const [answer, setAnswer] = useState<VersionsAnswer | null>(null);
  useEffect(() => {
    if (!mandateVersions) return;
    let live = true;
    const load = () => void mandateVersions(agentId).then((next) => live && setAnswer(next));
    load();
    const poll = window.setInterval(load, VERSIONS_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [mandateVersions, agentId]);
  return (
    <WorkspaceGate api>
      <article className="grid content-start gap-(--section-gap)">
        <Link href={`/agents/${agentId}`} className="text-sm underline-offset-4 hover:underline">
          Back to the agent
        </Link>
        <Section title="Versions">
          {answer === null ? (
            <p className={NOT_REPORTED}>Reading the versions.</p>
          ) : !answer.ok ? (
            <p className={NOT_REPORTED} data-reason={answer.reason}>
              {answer.reason === "not_found" ? "Your deployment reports no such agent." : "The versions are not reported."}
            </p>
          ) : (
            <VersionHistory history={answer.value} />
          )}
        </Section>
      </article>
    </WorkspaceGate>
  );
}
