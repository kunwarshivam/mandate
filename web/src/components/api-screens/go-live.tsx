"use client";

import { useState } from "react";
import { DECISION_KEY } from "@/components/kumo/key";
import { Panel, WorkspaceGate } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { useRuntime } from "@/lib/mock-runtime";
import { PENDING_COUNSEL, PENDING_POLICY, RISK_DISCLOSURES } from "@/lib/pending-counsel";
import { useCan } from "@/lib/roles";

/**
 * Going live (compliance doc, "Risk disclosures accepted before going live"; DEC-2134): what an
 * agent's owner reads before it trades, the platform policy values counsel sets, and the request
 * for real money. Every disclosure and value is a placeholder marked {@link PENDING_COUNSEL}
 * (DEC-79). Until counsel signs off every customer trades paper: the request for real money is
 * sent to the deployment, which refuses it and, while the workspace's real-money gate is off,
 * records the refusal in the journal. Nothing here can move an agent off paper.
 */

/** The mark, shown beside every placeholder. */
export function PendingCounsel() {
  return (
    <span data-slot="pending-counsel" className="w-fit rounded-md border border-border px-2 py-0.5 text-xs font-medium">
      {PENDING_COUNSEL}
    </span>
  );
}

export function RiskDisclosures() {
  return (
    <section className="grid gap-4" aria-labelledby="risk-disclosures-title" data-slot="risk-disclosures">
      <div className="grid gap-2">
        <h2 id="risk-disclosures-title" className="text-h3">
          Risks of an automated agent
        </h2>
        <PendingCounsel />
      </div>
      {RISK_DISCLOSURES.map((document) => (
        <article key={document.id} className="grid gap-1" aria-labelledby={`disclosure-${document.id}`} data-document={document.id}>
          <h3 id={`disclosure-${document.id}`} className="font-medium">
            {document.title}
          </h3>
          {document.paragraphs.map((paragraph) => (
            <p key={paragraph} className="text-sm">
              {paragraph}
            </p>
          ))}
        </article>
      ))}
    </section>
  );
}

export function PendingPolicy() {
  return (
    <section className="grid gap-3" aria-labelledby="pending-policy-title">
      <div className="grid gap-2">
        <h2 id="pending-policy-title" className="text-h3">
          Platform limits
        </h2>
        <PendingCounsel />
        <p className="text-sm text-muted-foreground">Counsel sets these. Until then the most conservative values in the specs apply to every agent.</p>
      </div>
      <dl className="grid gap-2 sm:grid-cols-[max-content_1fr] sm:gap-x-6">
        {PENDING_POLICY.map((item) => (
          <div key={item.label} className="contents">
            <dt className="text-sm text-muted-foreground">{item.label}</dt>
            <dd className="text-sm">
              {item.value} <span className="text-muted-foreground">({item.source})</span>
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/** What a real-money request came to, in the owner's words. */
function answerText(answer: { outcome: "refused"; code: string } | { outcome: "unknown" }): string {
  if (answer.outcome === "unknown") return "We could not confirm what your deployment did with this request. Nothing was moved to real money.";
  switch (answer.code) {
    case "live_unavailable":
      return "Refused: real-money trading is off for your workspace until counsel signs off. The agent keeps trading on paper. Your deployment records each refused request in your journal while real money is off.";
    case "journal_unavailable":
      return "Refused: your deployment could not record this request, so it moved nothing. Try again.";
    case "forbidden":
      return "Refused: your role cannot ask for real money.";
    default:
      return "Refused: your deployment moved nothing to real money.";
  }
}

function RealMoney() {
  const { view, realMoney } = useRuntime();
  const canDeploy = useCan("agents.deploy");
  const agents = view?.agents ?? [];
  const [chosen, setChosen] = useState("");
  const [state, setState] = useState<{ phase: "idle" } | { phase: "asking" } | { phase: "answered"; text: string }>({ phase: "idle" });
  const agentId = chosen || agents[0]?.agent_id || "";

  const ask = () => {
    if (!realMoney || !agentId) return;
    setState({ phase: "asking" });
    void realMoney.request(agentId).then((answer) => setState({ phase: "answered", text: answerText(answer) }));
  };

  return (
    <section className="grid gap-3" aria-labelledby="real-money-title" data-slot="real-money">
      <div className="grid gap-2">
        <h2 id="real-money-title" className="text-h3">
          Real money
        </h2>
        <p className="text-sm">
          Real money: <strong data-slot="real-money-state">off</strong>. Every agent in this workspace trades paper until counsel signs off. Only the platform can turn real money on, and only for a workspace it names.
        </p>
      </div>
      {!realMoney ? <p className="text-sm text-muted-foreground">Connect a deployment to ask for real money.</p> : null}
      {realMoney && canDeploy && agents.length === 0 ? <p className="text-sm text-muted-foreground">No agent is deployed yet.</p> : null}
      {realMoney && canDeploy && agents.length > 0 ? (
        <div className="grid gap-3">
          <label className="grid w-fit gap-1 text-sm">
            Agent
            <select className="rounded-md border border-border bg-background px-2 py-1" value={agentId} onChange={(e) => setChosen(e.target.value)}>
              {agents.map((agent) => (
                <option key={agent.agent_id} value={agent.agent_id}>
                  {agent.label ?? agent.agent_id}
                </option>
              ))}
            </select>
          </label>
          <button type="button" className={DECISION_KEY} onClick={ask} disabled={state.phase === "asking"}>
            Ask to trade real money
          </button>
          {state.phase === "asking" ? <p role="status">Asking your deployment.</p> : null}
          {state.phase === "answered" ? <p role="status">{state.text}</p> : null}
        </div>
      ) : null}
    </section>
  );
}

export function GoLive() {
  const { ws } = useRuntime();
  return (
    <WorkspaceGate api>
      <div className="mx-auto grid w-full max-w-2xl grid-cols-1 gap-6">
        <header className="grid gap-2">
          <h1 className="text-h1">Going live</h1>
          <EnvironmentBadge environment={ws.environment} />
          <p className="text-sm text-muted-foreground">Read these before an agent trades. They are placeholders until counsel writes them.</p>
        </header>
        <Panel well>
          <RiskDisclosures />
        </Panel>
        <Panel>
          <PendingPolicy />
        </Panel>
        <Panel>
          <RealMoney />
        </Panel>
      </div>
    </WorkspaceGate>
  );
}
