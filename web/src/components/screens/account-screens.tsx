"use client";

import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { PageHeader } from "@/components/kumo/page-header/page-header";
import { SourceTag } from "@/components/domain/mode";
import { GateDecisionRow } from "@/components/domain/gate-decision";
import { Timeline } from "@/components/domain/timeline";
import type { HealthState, TimelineEvent } from "@/fixtures/types";
import { RECORD_ZONE, clock, datedClock } from "@/lib/format";
import { Age } from "@/components/domain/as-of";
import { nothingNeedsYou } from "@/lib/attention";
import { allFeedsOk } from "@/lib/feeds";
import { openRequests, useRuntime } from "@/lib/mock-runtime";
import { RESTRICTIONS } from "@/lib/restrictions";
import { useCan } from "@/lib/roles";
import { type Screen, decisionHref, screensIn } from "@/lib/screens";
import { useSession } from "@/lib/session";
import { PasskeysSection } from "@/components/auth/passkeys-section";
import { PushSection } from "@/components/notifications/push-section";
import { ComingSoon } from "./coming-soon";
import { AllClear, Panel, Section, WorkspaceGate } from "./common";

const HEALTH_LABEL = { market_data: "Market data", broker: "Broker", deployment: "Deployment", relay: "Push relay" } as const;

const STATE_WORD: Record<HealthState, string> = { ok: "Current", stale: "Stale", down: "Down" };

/**
 * Feeds first, then the agents' conditions. On a calm day, when every feed answers and Home would say
 * all clear, the page opens with Home's own line (C-13), so four current feeds do not read as four
 * alerts. Each feed's as-of time ends on the row's right edge, so the times read as one column.
 */
function Alerts() {
  const { ws, now } = useRuntime();
  const health = (Object.keys(HEALTH_LABEL) as Array<keyof typeof HEALTH_LABEL>).map((key) => ({ key, ...ws.health[key] }));
  const conditions = ws.agents.flatMap((agent) => agent.restrictions.map((r) => ({ agent, r })));
  const calm = allFeedsOk(ws) && nothingNeedsYou(ws, openRequests(ws, now));
  return (
    <div className="grid grid-cols-1 gap-(--section-gap)">
      {calm ? <AllClear /> : null}
      <Section title="Data and deployment">
        <ul className="grid divide-y divide-border/70">
          {health.map((h) => (
            <li key={h.key} data-state={h.state} className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 py-3">
              <span className="font-semibold">{HEALTH_LABEL[h.key]}</span>
              <span className="ml-auto flex flex-wrap items-baseline justify-end gap-x-2 gap-y-1 text-sm">
                {h.state === "ok" ? null : <span className="rounded-sm border border-foreground px-1.5 text-label">{STATE_WORD[h.state]}</span>}
                <span data-slot="as-of" className="whitespace-nowrap text-muted-foreground">
                  as of <span className="font-mono tabular">{clock(h.as_of)}</span>, <Age at={h.as_of} now={now} />
                </span>
              </span>
            </li>
          ))}
        </ul>
      </Section>
      <Section title="Agent conditions">
        {conditions.length === 0 ? (
          <p className="text-muted-foreground">No agent is restricted.</p>
        ) : (
          <ul className="grid divide-y divide-border/70">
            {conditions.map(({ agent, r }) => {
              const text = RESTRICTIONS[r.code];
              return (
                <li key={`${agent.agent_id}-${r.code}-${r.symbol ?? ""}`}>
                  <Link href={`/agents/${agent.agent_id}`} className="press group -mx-3 grid gap-1 rounded-xl px-3 py-3.5 hover:bg-background">
                    <span className="flex flex-wrap items-center gap-2">
                      <SourceTag source={text.source} />
                      <span className="font-semibold">
                        {agent.label}: {text.label}
                        {r.symbol ? ` (${r.symbol})` : ""}
                      </span>
                      <ArrowRight className="ml-auto size-6 shrink-0" aria-hidden />
                    </span>
                    <span className="text-sm text-muted-foreground">
                      Since <span className="font-mono tabular">{datedClock(r.since, now, RECORD_ZONE)}</span>. Blocks: {text.blocks}. Ends when: {text.endsWhen}.
                    </span>
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </Section>
    </div>
  );
}

export function AlertsScreen() {
  return (
    <div className="grid">
      <PageHeader title="Alerts" description="Conditions that change what agents may do, each with the time it was last true." />
      <WorkspaceGate>
        <Alerts />
      </WorkspaceGate>
    </div>
  );
}

/** Auditors read decisions here but cannot open agent pages, so only roles that can see agents get links. */
function Decisions() {
  const { ws } = useRuntime();
  const linked = useCan("agents.view");
  if (ws.decisions.length === 0) return <p className="text-muted-foreground">No gate decisions yet.</p>;
  return (
    <Panel className="py-0.5 sm:py-0.5">
      <ul>
        {ws.decisions.map((d) => (
          <GateDecisionRow
            key={d.event_id}
            decision={d}
            agent={ws.agents.find((a) => a.agent_id === d.agent_id)}
            showAgent
            href={linked ? decisionHref(d.agent_id, d.event_id) : undefined}
          />
        ))}
      </ul>
    </Panel>
  );
}

function AllTimeline() {
  const { ws, now } = useRuntime();
  const events: TimelineEvent[] = ws.agents
    .flatMap((a) => (ws.timeline[a.agent_id] ?? []).map((e) => ({ ...e, text: `${a.label}: ${e.text}` })))
    .sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  return (
    <Panel>
      <Timeline events={events} now={now} />
    </Panel>
  );
}

/** A screen from the registry: built ones render their content, the rest say what they will hold. */
export function RegistryScreen({ screen }: { screen: Screen }) {
  const session = useSession();
  const body = (() => {
    switch (screen.key) {
      case "settings-profile":
        return session ? (
          <div className="grid gap-(--section-gap)">
            <PasskeysSection />
            <ComingSoon purpose="Your name and sign-in sessions." />
          </div>
        ) : (
          <ComingSoon purpose={screen.purpose} />
        );
      case "settings-notifications":
        return (
          <div className="grid gap-(--section-gap)">
            <PushSection />
            <ComingSoon purpose={screen.purpose} />
          </div>
        );
      case "audit-decisions":
        return (
          <WorkspaceGate>
            <Decisions />
          </WorkspaceGate>
        );
      case "audit-timeline":
        return (
          <WorkspaceGate>
            <AllTimeline />
          </WorkspaceGate>
        );
      default:
        return <ComingSoon purpose={screen.purpose} />;
    }
  })();
  return (
    <div className="grid">
      <PageHeader title={screen.label} description={screen.built ? screen.purpose : undefined} />
      {body}
    </div>
  );
}

/** The Audit and Workspace index pages: each child screen with what it is for. */
export function SectionIndexScreen({ title, purpose, prefix }: { title: string; purpose: string; prefix: string }) {
  return (
    <div className="grid">
      <PageHeader title={title} description={purpose} />
      <ul className="grid max-w-3xl divide-y divide-border/70">
        {screensIn(prefix)
          .filter((s) => s.built)
          .map((s) => (
            <li key={s.key}>
              <Link href={s.href} className="press group -mx-3 grid gap-1 rounded-xl px-3 py-3.5 hover:bg-background">
                <span className="flex items-center justify-between gap-3 font-semibold">
                  {s.label}
                  <ArrowRight className="size-6 shrink-0 text-foreground" aria-hidden />
                </span>
                <span className="text-sm text-muted-foreground">{s.purpose}</span>
              </Link>
            </li>
          ))}
      </ul>
      {screensIn(prefix).some((s) => !s.built) ? (
        <p data-slot="coming-list" className="mt-(--block-gap) max-w-3xl text-sm text-muted-foreground">
          Coming during the beta:{" "}
          {screensIn(prefix)
            .filter((s) => !s.built)
            .map((s) => s.label.toLowerCase())
            .join(", ")}
          .
        </p>
      ) : null}
    </div>
  );
}
