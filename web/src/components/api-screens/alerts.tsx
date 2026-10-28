"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { ALERT_KINDS, type Alert, type AlertDelivery, subjectHref } from "@/api/alerts";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { Section, WorkspaceGate } from "@/components/screens/common";
import { RECORD_ZONE, datedClock } from "@/lib/format";
import { type AlertsAnswer, useRuntime } from "@/lib/mock-runtime";

/**
 * The alerts over the workspace API (`GET /alerts`, §4.7, DEC-2141): each alert the journal records
 * about the workspace's agents and accounts, newest first. A line names the alert's kind from the
 * closed table in `@/api/alerts`, when it was raised, and what became of its notice to you; nothing
 * is said beyond what the alert's records hold, and an empty list says so. A read that fails is
 * named, never shown as no alerts.
 */

/** How often the list is read again while it is open. */
const POLL_MS = 10_000;

const CHANNEL: Record<AlertDelivery["channel"], string> = { email: "Email", web_push: "Push", sms: "SMS", telegram: "Telegram", slack: "Slack", phone: "Phone" };

const STATUS: Record<AlertDelivery["status"], string> = {
  delivered: "delivered",
  failed: "failed",
  suppressed_quiet_hours: "held for quiet hours",
  deferred_quiet_hours: "deferred to after quiet hours",
  abandoned: "not delivered",
};

const SUBJECT: Record<Alert["stream"]["type"], string> = { agent: "See the agent", account: "See positions", control: "See the records" };

/** Reads the alerts on open and every few seconds; a slower earlier read never replaces a later one. */
function useAlerts(): AlertsAnswer | null {
  const { alerts } = useRuntime();
  const [answer, setAnswer] = useState<AlertsAnswer | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    if (!alerts) return;
    let live = true;
    const load = async () => {
      generation.current += 1;
      const mine = generation.current;
      const read = await alerts();
      if (live && mine === generation.current) setAnswer(read);
    };
    void load();
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [alerts]);
  return answer;
}

function Notice({ alert }: { alert: Alert }) {
  if (alert.notice === null) return <>No notice recorded for this alert.</>;
  if (alert.notice.delivery.length === 0) return <>No delivery to you recorded.</>;
  return <>{alert.notice.delivery.map((d) => `${CHANNEL[d.channel]} ${STATUS[d.status]}`).join(", ")}.</>;
}

function AlertRow({ alert, now }: { alert: Alert; now: string }) {
  return (
    <li data-slot="api-alert" data-kind={alert.kind}>
      <Link href={subjectHref(alert)} className="press group -mx-3 grid gap-1 rounded-xl px-3 py-3.5 hover:bg-background">
        <span className="flex flex-wrap items-center gap-2">
          {alert.class === "safety" ? <span className="rounded-sm border border-foreground px-1.5 text-label">Safety</span> : <span className="rounded-sm border border-border px-1.5 text-label">Info</span>}
          <span className="font-semibold text-pretty">{ALERT_KINDS[alert.kind].label}</span>
          <span className="sr-only">. {SUBJECT[alert.stream.type]}</span>
          <ArrowRight className="ml-auto size-6 shrink-0" aria-hidden />
        </span>
        <span className="text-sm text-muted-foreground text-pretty">
          Raised <span className="font-mono tabular-nums">{datedClock(alert.raised_at, now, RECORD_ZONE)}</span>. <Notice alert={alert} />
        </span>
      </Link>
    </li>
  );
}

function Alerts() {
  const { now } = useRuntime();
  const answer = useAlerts();
  if (answer === null) return <p className="text-muted-foreground">Reading alerts…</p>;
  if (!answer.ok) return <p className="text-muted-foreground">Alerts not reported: your deployment answered {answer.reason}.</p>;
  const { alerts, truncated } = answer.read;
  return (
    <Section title="Alerts about your agents and accounts">
      {alerts.length === 0 ? (
        <p data-slot="no-alerts" className="text-muted-foreground">
          None yet.
        </p>
      ) : (
        <ul className="grid divide-y divide-border/70">
          {alerts.map((alert) => (
            <AlertRow key={alert.alert_id} alert={alert} now={now} />
          ))}
        </ul>
      )}
      {truncated ? <p className="text-sm text-muted-foreground">Older alerts are not shown.</p> : null}
    </Section>
  );
}

export function ApiAlerts() {
  return (
    <div className="grid">
      <PageHeader title="Alerts" description="What your deployment recorded about your agents and accounts, newest first, and what became of each notice to you." />
      <WorkspaceGate api>
        <Alerts />
      </WorkspaceGate>
    </div>
  );
}
