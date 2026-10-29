"use client";

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import type { ConnectionDetail } from "@/api/connect";
import { type ShownChallenge, connectConfirmCommand } from "@/api/step-up";
import { ConnectDisclosure } from "@/components/domain/connect-disclosure";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { Panel, Section, WorkspaceGate } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { type StartAnswer, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { HostConfirm } from "./host-confirm";

/**
 * Connections over the workspace API (§4.5; DEC-2620, DEC-2622, DEC-2623): every connection the
 * control stream names with its state, and the Alpaca paper connect. The connect shows the
 * token-breadth disclosure, then steps up with the member's passkey bound to it (DEC-2126). While the
 * member has no passkey that can confirm, or the browser cannot use one, it falls back to the interim
 * `cli_confirm` code (DEC-2125) and says why; once the owner has confirmed on the host, it starts the
 * connect. Either way it then sends the browser to Alpaca. Alpaca returns the
 * browser to the API's callback, which redirects to `/connections/{id}`. Paper only; nothing here is
 * stored, and the browser never holds the code Alpaca issues.
 */

const STATE_TEXT: Readonly<Record<ConnectionDetail["state"], string>> = {
  connecting: "Connecting: waiting for Alpaca and the permission checks.",
  established: "Connected.",
  refused: "Refused: nothing can trade through it.",
  revoked: "Revoked: nothing can trade through it.",
};

/** Why a start did not send the browser to Alpaca, by what the deployment answered. */
function startText(reason: string, effect: "none" | "recorded" | "unknown"): string {
  if (effect !== "none") return "We could not confirm whether the connect started. Check this list before you connect again.";
  switch (reason) {
    case "step_up_missing":
      return "Not started: your deployment found no confirmation of this code on the host. Run the command, then try again.";
    case "step_up_mismatch":
      return "Not started: the confirmation on the host was for another connect.";
    case "step_up_stale":
    case "step_up_reused":
      return "Not started: this code can no longer be used. Ask for a new one.";
    case "passkey_canceled":
      return "Not started: the passkey check was canceled or timed out.";
    case "passkey_failed":
      return "Not started: your passkey's answer could not be read. Try again.";
    case "step_up_method":
      return "Not started: your deployment asks for the code confirmed on the host for this connect. Try again.";
    case "live_unavailable":
      return "Not started: connecting is for paper only.";
    default:
      return "Not started: your deployment answered that nothing was recorded.";
  }
}

/** Passkey refusals after which the connect falls back to the host's code, and why, in words (DEC-2140). */
const FALLBACK_TEXT: Readonly<Record<string, string>> = {
  passkey_unavailable: "this browser can't use a passkey",
  no_passkey: "you have no passkey on your deployment yet. Add one in Settings",
  passkey_cooling_off: "your passkey is new, and confirms only once its first 24 hours have passed",
};

function noCode(reason: string): string {
  if (reason === "live_unavailable") return "No code: connecting is for paper only. Nothing was started.";
  return "No code: your deployment did not answer with one. Nothing was started; try again.";
}

type Flow =
  | { phase: "disclosure" }
  | { phase: "passkey" }
  | { phase: "asking"; fallback?: string }
  | { phase: "shown"; shown: ShownChallenge; command: string; fallback?: string }
  | { phase: "starting" }
  | { phase: "no_code"; reason: string }
  | { phase: "not_started"; reason: string; effect: "none" | "recorded" | "unknown" }
  | { phase: "leaving" };

function ConnectionRow({ connection }: { connection: ConnectionDetail }) {
  return (
    <li className="grid gap-1 py-3">
      <div className="flex flex-wrap items-center gap-2">
        <Link href={`/connections/${connection.connection_id}`} className="font-medium underline-offset-4 hover:underline">
          {connection.broker === "alpaca" ? "Alpaca" : connection.broker}
        </Link>
        <EnvironmentBadge environment={connection.environment} />
      </div>
      <p className="text-sm text-muted-foreground" data-state={connection.state}>
        {STATE_TEXT[connection.state]}
      </p>
    </li>
  );
}

export function ApiConnections({ go = (url: string) => window.location.assign(url) }: { go?: (url: string) => void }) {
  const { connect } = useRuntime();
  const canConnect = useCan("connections.manage");
  const [list, setList] = useState<{ ok: true; connections: ConnectionDetail[] } | { ok: false; reason: string } | null>(null);
  const [flow, setFlow] = useState<Flow | null>(null);

  const reload = useCallback(() => {
    if (!connect) return;
    void connect.list().then(setList);
  }, [connect]);
  useEffect(() => reload(), [reload]);

  const started = (answer: StartAnswer) => {
    if (answer.ok) {
      setFlow({ phase: "leaving" });
      return go(answer.authorizationUrl);
    }
    setFlow({ phase: "not_started", reason: answer.reason, effect: answer.effect });
    reload();
  };
  const ask = (fallback?: string) => {
    if (!connect) return;
    setFlow({ phase: "asking", fallback });
    void connect.challenge().then((answer) => {
      if (!answer.ok) return setFlow({ phase: "no_code", reason: answer.reason });
      setFlow({ phase: "shown", shown: answer.shown, command: connectConfirmCommand({ workspaceId: answer.workspaceId, principalId: answer.principalId, shown: answer.shown }), fallback });
    });
  };
  const confirm = () => {
    if (!connect) return;
    setFlow({ phase: "passkey" });
    void connect.startWithPasskey().then((answer) => {
      const fallback = answer.ok ? undefined : FALLBACK_TEXT[answer.reason];
      if (fallback) return ask(fallback);
      started(answer);
    });
  };
  const start = (shown: ShownChallenge) => {
    if (!connect) return;
    setFlow({ phase: "starting" });
    void connect.start(shown.challenge).then(started);
  };

  return (
    <WorkspaceGate api>
      <div className="mx-auto grid w-full max-w-2xl grid-cols-1 gap-6">
        <h1 className="text-h1">Connections</h1>
        <Section title="Broker accounts">
          {list === null ? (
            <p className="text-sm text-muted-foreground">Reading connections.</p>
          ) : !list.ok ? (
            <p className="text-sm text-muted-foreground">Connections not reported.</p>
          ) : list.connections.length === 0 ? (
            <p className="text-sm">No broker account is connected.</p>
          ) : (
            <ul className="divide-y divide-border" aria-label="Broker accounts">
              {list.connections.map((c) => (
                <ConnectionRow key={c.connection_id} connection={c} />
              ))}
            </ul>
          )}
        </Section>
        {canConnect ? (
          <Section title="Connect Alpaca paper trading">
            {flow === null || flow.phase === "no_code" || flow.phase === "not_started" ? (
              <div className="grid gap-3">
                {flow?.phase === "no_code" ? <p role="status">{noCode(flow.reason)}</p> : null}
                {flow?.phase === "not_started" ? <p role="status">{startText(flow.reason, flow.effect)}</p> : null}
                <button type="button" className={cn("w-fit", KEY)} onClick={() => setFlow({ phase: "disclosure" })}>
                  Connect Alpaca paper
                </button>
              </div>
            ) : null}
            {flow?.phase === "disclosure" ? (
              <Panel well>
                <div className="grid gap-4">
                  <ConnectDisclosure />
                  <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                    <button type="button" className={cn("min-w-28", KEY)} onClick={() => setFlow(null)}>
                      Cancel
                    </button>
                    <button type="button" className={DECISION_KEY} onClick={confirm}>
                      Connect with Alpaca
                    </button>
                  </div>
                </div>
              </Panel>
            ) : null}
            {flow?.phase === "passkey" ? <p role="status">Waiting for your passkey. Nothing is started until your browser&apos;s passkey check is done.</p> : null}
            {(flow?.phase === "asking" || flow?.phase === "shown") && flow.fallback ? <p role="note">Confirming on the host instead: {flow.fallback}.</p> : null}
            {flow?.phase === "asking" ? <p role="status">Asking your deployment for a code. Nothing is started yet.</p> : null}
            {flow?.phase === "shown" ? (
              <HostConfirm key={flow.shown.challenge.challenge_id} shown={flow.shown} command={flow.command} submitLabel="Connect with Alpaca" onSubmit={() => start(flow.shown)} onCancel={() => setFlow(null)} />
            ) : null}
            {flow?.phase === "starting" ? <p role="status">Starting the connect.</p> : null}
            {flow?.phase === "leaving" ? <p role="status">Taking you to Alpaca.</p> : null}
          </Section>
        ) : null}
        <Section title="Robinhood Agentic Trading">
          <div className="grid gap-2" data-slot="robinhood-real-money">
            <p className="text-sm">
              Real money: <strong>off</strong>. Robinhood has no paper environment, so a Robinhood account is always real money. Real money stays off for this workspace until counsel signs off; your
              agents trade on Alpaca paper.
            </p>
            <Link href="/go-live" className="w-fit text-sm underline underline-offset-4">
              Going live
            </Link>
          </div>
        </Section>
      </div>
    </WorkspaceGate>
  );
}

/** How often a connecting connection is read again until it settles. */
const CONNECTING_POLL_MS = 3000;

/** One connection (`GET /connections/{id}`): where the API's callback lands the browser after Alpaca (DEC-2626). */
export function ApiConnection({ connectionId, pollMs = CONNECTING_POLL_MS }: { connectionId: string; pollMs?: number }) {
  const { connect } = useRuntime();
  const [read, setRead] = useState<{ ok: true; connection: ConnectionDetail } | { ok: false; reason: string } | null>(null);

  useEffect(() => {
    if (!connect) return;
    let live = true;
    let timer: number | undefined;
    const load = () =>
      void connect.read(connectionId).then((answer) => {
        if (!live) return;
        setRead(answer);
        if (answer.ok && answer.connection.state === "connecting") timer = window.setTimeout(load, pollMs);
      });
    load();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [connect, connectionId, pollMs]);

  const c = read?.ok ? read.connection : null;
  return (
    <WorkspaceGate api>
      <article className="mx-auto grid w-full max-w-2xl grid-cols-1 gap-6" aria-labelledby="connection-title">
        <header className="grid gap-2">
          <h1 id="connection-title" className="text-h1">
            {c ? (c.broker === "alpaca" ? "Alpaca" : c.broker) : "Connection"}
          </h1>
          {c ? <EnvironmentBadge environment={c.environment} /> : null}
        </header>
        {read === null ? <p className="text-sm text-muted-foreground">Reading the connection.</p> : null}
        {read && !read.ok ? <p role="status">{read.reason === "not_found" ? "Connection not found." : "The connection is not reported."}</p> : null}
        {c ? (
          <Panel well>
            <div className="grid gap-2">
              <p role="status" data-state={c.state}>
                {STATE_TEXT[c.state]}
              </p>
              {c.state === "established" && c.scopes ? <p className="text-sm text-muted-foreground">Access granted: {c.scopes.join(", ")}.</p> : null}
              {c.refusal ? (
                <p className="text-sm text-muted-foreground">
                  The {c.refusal.check ?? "connect"} check refused it ({c.refusal.reason.replaceAll("_", " ")}).
                </p>
              ) : null}
            </div>
          </Panel>
        ) : null}
        <Link href="/connections" className="w-fit text-sm underline underline-offset-4">
          All connections
        </Link>
      </article>
    </WorkspaceGate>
  );
}
