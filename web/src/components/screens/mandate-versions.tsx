import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import type { Agent, ChangeClass, MandateVersionRecord } from "@/fixtures/types";
import { clock, dateLabel, zoneLabel } from "@/lib/format";
import { CHANGE_CLASS_LABEL } from "@/lib/labels";
import { changeLabel, changeValue } from "@/lib/mandate-paths";
import { cn } from "@/lib/utils";

const TAG = "inline-flex h-6 w-fit shrink-0 items-center rounded-md px-2.5 text-label whitespace-nowrap";

/** Risk-increasing is outlined in ink; reducing and neutral stay quiet. The word carries the class, not a colour. */
const CLASS_TAG: Record<ChangeClass, string> = {
  risk_increasing: "bg-card font-medium text-foreground ring-1 ring-ink ring-inset",
  risk_reducing: "bg-background text-muted-foreground",
  neutral: "bg-background text-muted-foreground",
};

function ClassTag({ value }: { value: ChangeClass }) {
  return (
    <span data-slot="change-class" data-classification={value} className={cn(TAG, CLASS_TAG[value])}>
      {CHANGE_CLASS_LABEL[value]}
    </span>
  );
}

function When({ at }: { at: string }) {
  return (
    <time dateTime={at}>
      {dateLabel(at)} at <span className="font-mono tabular">{clock(at)}</span> {zoneLabel(at)}
    </time>
  );
}

function canceledLine(count: number): string {
  if (count === 0) return "No requests were waiting for you.";
  const requests = count === 1 ? "1 request waiting for you was" : `${count} requests waiting for you were`;
  return `${requests} canceled; anything still wanted is proposed again under this version.`;
}

function Application({ version, number, inEffect }: { version: MandateVersionRecord; number: number; inEffect: number }) {
  const a = version.application;
  if (version.previous === null) {
    return (
      <p>
        Deployed to paper on <When at={a.at} />.
      </p>
    );
  }
  switch (a.result) {
    case "applied":
      return (
        <p>
          {version.classification === "risk_increasing" ? (
            <>
              Applied at the next safe point, <When at={a.at} />.
            </>
          ) : (
            "Applied when you confirmed it."
          )}{" "}
          {canceledLine(a.approvals_canceled)}
        </p>
      );
    case "rejected":
      return (
        <p data-slot="version-rejected">
          Rejected at application, <When at={a.at} />. {a.reason} Version {inEffect} stays in effect; version {number} never applied.
        </p>
      );
    default: {
      const unhandled: never = a;
      throw new Error(`unhandled application ${JSON.stringify(unhandled)}`);
    }
  }
}

/**
 * A6: every confirmed version, newest first, each diffed against the version that was in effect,
 * every changed path with its classification (mandate spec §9.2), and how it applied (§2.2).
 * The classification is the one recorded with the version; the screen never works one out.
 */
export function MandateVersions({ agent }: { agent: Agent }) {
  const numberOf = new Map(agent.versions.map((v, i) => [v.mandate_version, i + 1]));
  const newestFirst = agent.versions.map((v, i) => ({ v, number: i + 1 })).reverse();
  return (
    <div className="grid max-w-3xl gap-(--block-gap)">
      <p className="max-w-measure text-sm text-muted-foreground">
        A risk-increasing version takes your passkey and applies at the next safe point: the agent&apos;s next check with no order in an unknown state. A risk-reducing or
        neutral version applies when you confirm it. When a version applies, requests waiting for you are canceled and proposed again under it.
      </p>
      <ol data-slot="versions" className="grid divide-y divide-border/70">
        {newestFirst.map(({ v, number }) => {
          const current = v.mandate_version === agent.mandate_version;
          const headingId = `version-${number}`;
          return (
            <li key={v.mandate_version} data-slot="version" data-current={current || undefined} data-result={v.application.result} className="grid gap-3 py-5 first:pt-2">
              <article aria-labelledby={headingId} className="grid gap-3">
                <header className="grid gap-1">
                  <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                    <h3 id={headingId} className="text-h3">
                      Version {number}
                    </h3>
                    {current ? <span className={cn(TAG, "bg-mandate text-mandate-strong ring-1 ring-mandate-edge ring-inset")}>In effect</span> : null}
                    {v.application.result === "rejected" ? <span className={cn(TAG, "bg-background text-foreground")}>Not applied</span> : null}
                    {v.classification ? <ClassTag value={v.classification} /> : null}
                  </div>
                  <p className="text-caption text-muted-foreground">
                    Confirmed by you{v.step_up ? " with your passkey" : ""} on <When at={v.confirmed_at} />. Hash{" "}
                    <span className="font-mono">{v.mandate_version.slice(7, 19)}</span>
                  </p>
                </header>
                {v.changes.length > 0 ? (
                  <ul data-slot="version-changes" className="grid">
                    {v.changes.map((c) => (
                      <li
                        key={c.path}
                        data-slot="version-change"
                        data-path={c.path}
                        className="grid gap-1.5 border-b border-border/70 py-3 text-sm last:border-b-0 sm:grid-cols-[minmax(0,13rem)_minmax(0,1fr)_auto] sm:items-center sm:gap-4"
                      >
                        <span className="text-muted-foreground">{changeLabel(c.path)}</span>
                        <span className="flex flex-wrap items-center gap-x-2">
                          <span className="font-mono tabular">{changeValue(c.path, c.from)}</span>
                          <ArrowRight aria-hidden className="size-5 text-muted-foreground" />
                          <span className="sr-only">to</span>
                          <span className="font-mono font-medium tabular">{changeValue(c.path, c.to)}</span>
                        </span>
                        <ClassTag value={c.classification} />
                      </li>
                    ))}
                  </ul>
                ) : null}
                <div className="max-w-measure text-sm">
                  <Application version={v} number={number} inEffect={numberOf.get(agent.mandate_version) ?? number} />
                </div>
              </article>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
