"use client";

import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type Fetch, createWorkspaceClient } from "@/api/client";
import { prepareApprovalResponse, prepareOwnerCommand } from "@/api/commands";
import { type ApiView, loadApi } from "@/api/read-models";
import { emptyWorkspace } from "@/api/reads";
import { REFRESH_EVERY_MS, type SessionKeeper, type SessionState } from "@/api/session";
import type { CommandPhase, PreparedCommand } from "@/api/types";
import type { Iso, Workspace } from "@/fixtures/types";
import type { ApiSource } from "./data-source";
import { mandateVersion } from "./fixture-journey";
import {
  type ApprovalRecord,
  type ApprovalResponse,
  type Command,
  type CommandKind,
  type CommandRecord,
  type Deployment,
  type MandateChangeRequest,
  type Runtime,
  RuntimeContext,
} from "./mock-runtime";

/** How many times an unanswered command is sent again, with the same key and body (API-4), before it is called unknown. */
const SENDS = 4;
/** How many times a recorded command's status is read before the runtime stops following it (spec §6.3 item 4). */
const STATUS_READS = 30;
const SETTLED: ReadonlySet<CommandPhase> = new Set(["applied", "refused", "ended"]);

type Delivery = { phase: "recorded"; eventId: string } | { phase: "undelivered"; reason: string } | { phase: "unknown"; eventId: string | null };

const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));
const later = (ms: number, iso: Iso): Iso => new Date(Date.parse(iso) + ms).toISOString();

/**
 * The runtime the screens use when the data source is a workspace deployment (DEC-975): the same
 * `Runtime` as the fixture stand-in, filled from the workspace API's reads and sending each command
 * through its idempotent write routes. Nothing is shown before the journal answers, nothing is
 * written to browser storage, and an answer that does not confirm a record is never shown as one.
 * A command is sent whatever the reads say: Stop needs only the API (API-8).
 */
export function ApiRuntimeProvider({
  source,
  children,
  fetch,
  refreshMs = 5000,
  statusPollMs = 1000,
  retryMs = 1000,
  session,
  keepAliveMs = REFRESH_EVERY_MS,
}: {
  source: Pick<ApiSource, "origin" | "workspaceId">;
  children: ReactNode;
  fetch?: Fetch;
  refreshMs?: number;
  statusPollMs?: number;
  retryMs?: number;
  /** The API session (§3.11 items 5 and 6); none in the e2e build, whose mock API reads no cookie. */
  session?: SessionKeeper;
  keepAliveMs?: number;
}) {
  /** Reads wait while a refresh rotates the cookie; commands never wait (API-7). */
  const client = useMemo(() => {
    const send: Fetch = fetch ?? ((input, init) => window.fetch(input, init));
    const guarded: Fetch = (input, init) => ((init.method ?? "GET") === "GET" && session ? session.guard(() => send(input, init)) : send(input, init));
    return createWorkspaceClient({ workspaceId: source.workspaceId, baseUrl: source.origin, fetch: guarded });
  }, [source.workspaceId, source.origin, fetch, session]);
  /** Whether an API session is open; with no keeper the API's own cookie is the caller's business. */
  const signedIn = useRef<SessionState | null>(session ? null : "open");
  const [ws, setWs] = useState<Workspace>(() => emptyWorkspace("loading", new Date().toISOString()));
  const [now, setNow] = useState<Iso>(ws.now);
  const [commands, setCommands] = useState<Command[]>([]);
  const [responses, setResponses] = useState<Record<string, ApprovalResponse>>({});
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [mandateChanges, setMandateChanges] = useState<MandateChangeRequest[]>([]);
  const wsRef = useRef(ws);
  /**
   * The last workspace the reads gave. Commands are prepared from it while the reads fail, so pause-all
   * still reaches every agent last seen and stop-all its connection (API-8, rule 13); nothing is shown from it.
   */
  const lastRead = useRef<{ ws: Workspace; view: ApiView } | null>(null);
  /** The read models as last served; `null` whenever the screens must show none (loading, unreachable, signed out). */
  const [view, setView] = useState<ApiView | null>(null);
  /** Each refresh's number: an answer to an older one never overwrites a newer one. */
  const reads = useRef(0);
  const nowRef = useRef(now);
  /** The server's clock as of the last read, against this device's, so the clock runs on the deployment's time. */
  const served = useRef<{ at: Iso; local: number } | null>(null);
  const counter = useRef(0);
  const mounted = useRef(true);

  /** Opens the API session; a workspace that cannot be read for want of one says so and shows nothing. */
  const opening = useRef<Promise<boolean> | null>(null);
  const openSession = useCallback((): Promise<boolean> => {
    if (!session) return Promise.resolve(true);
    opening.current ??= session
      .open()
      .then((state) => {
        signedIn.current = state;
        return state === "open";
      })
      .finally(() => {
        opening.current = null;
      });
    return opening.current;
  }, [session]);

  const refresh = useCallback(async () => {
    reads.current += 1;
    const read = reads.current;
    if (signedIn.current !== "open" && signedIn.current !== "outage") await openSession();
    if (signedIn.current !== "open" && signedIn.current !== "outage") {
      if (!mounted.current || read !== reads.current) return;
      wsRef.current = emptyWorkspace(signedIn.current === "unreachable" ? "unreachable" : "signed_out", nowRef.current);
      setWs(wsRef.current);
      setView(null);
      return;
    }
    let loaded = await loadApi(client);
    if (!loaded.ok && loaded.error.code === "unauthenticated" && session && (await openSession())) loaded = await loadApi(client);
    if (!mounted.current || read !== reads.current) return;
    if (!loaded.ok && loaded.error.code === "unauthenticated" && session) {
      wsRef.current = emptyWorkspace("signed_out", nowRef.current);
    } else if (!loaded.ok && loaded.error.code === "membership_unavailable") {
      wsRef.current = { ...emptyWorkspace("membership_unavailable", nowRef.current), environment: lastRead.current?.ws.environment ?? "paper" };
    } else if (loaded.ok) {
      served.current = { at: loaded.ws.now, local: Date.now() };
      lastRead.current = { ws: loaded.ws, view: loaded.view };
      wsRef.current = loaded.ws;
    } else {
      const last = wsRef.current;
      const down = emptyWorkspace("unreachable", nowRef.current);
      wsRef.current = last.status === "ready" ? { ...down, environment: last.environment, health: { ...last.health, deployment: { state: "down", as_of: last.health.deployment.as_of } } } : down;
    }
    setWs(wsRef.current);
    setView(loaded.ok && wsRef.current === loaded.ws ? loaded.view : null);
  }, [client, openSession, session]);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const poll = window.setInterval(() => void refresh(), refreshMs);
    const keep = session
      ? window.setInterval(() => {
          if (signedIn.current === "open" || signedIn.current === "outage" || signedIn.current === "unreachable")
            void session.refresh().then((state) => (signedIn.current = state === "unreachable" && signedIn.current !== "unreachable" ? signedIn.current : state));
        }, keepAliveMs)
      : null;
    const clock = window.setInterval(() => {
      const base = served.current;
      const next = base ? later(Date.now() - base.local, base.at) : new Date().toISOString();
      nowRef.current = next;
      setNow(next);
    }, 1000);
    return () => {
      mounted.current = false;
      window.clearInterval(poll);
      if (keep !== null) window.clearInterval(keep);
      window.clearInterval(clock);
    };
  }, [refresh, refreshMs, session, keepAliveMs]);

  /** Sends one prepared gesture until the API answers, with the same key and body each time (API-4, API-13). */
  const deliver = useCallback(
    async (command: PreparedCommand): Promise<Delivery> => {
      let eventId: string | null = null;
      for (let attempt = 0; attempt < SENDS; attempt += 1) {
        const outcome = await client.send(command);
        if (outcome.ok) return { phase: "recorded", eventId: outcome.accepted.event_id };
        const { error } = outcome;
        if (error.effect === "none") return { phase: "undelivered", reason: error.code };
        eventId = error.event_id ?? eventId;
        if (error.effect === "recorded" && eventId) return { phase: "recorded", eventId };
        if (!error.retryable) break;
        await wait(retryMs * 2 ** attempt);
      }
      return { phase: "unknown", eventId };
    },
    [client, retryMs],
  );

  /** Follows a recorded command to `applied`, `refused` or `ended`, then reads the workspace again (spec §6.3). */
  const follow = useCallback(
    async (eventId: string): Promise<CommandPhase | null> => {
      for (let read = 0; read < STATUS_READS && mounted.current; read += 1) {
        const status = await client.commandStatus(eventId);
        if (status.ok && SETTLED.has(status.value.phase)) {
          await refresh();
          return status.value.phase;
        }
        await wait(statusPollMs);
      }
      await refresh();
      return null;
    },
    [client, refresh, statusPollMs],
  );

  const send = useCallback(
    (kind: CommandKind, agentId: string | null, record?: CommandRecord): Command => {
      counter.current += 1;
      const command: Command = { id: `cmd-${counter.current}`, kind, agentId, phase: "sent", sentAt: nowRef.current, ...(record ? { record } : {}) };
      const update = (patch: Partial<Command>) => setCommands((list) => list.map((c) => (c.id === command.id ? { ...c, ...patch } : c)));
      setCommands((list) => [...list, command]);
      const last = lastRead.current;
      const scope = last
        ? { agents: last.view.agents, connectionId: last.ws.connection.connection_id, environment: last.ws.environment }
        : { agents: [], connectionId: "", environment: wsRef.current.environment };
      const prepared = prepareOwnerCommand(client, scope, kind, agentId, record);
      void (async () => {
        if (!prepared.ok) return update({ phase: "undelivered" });
        const delivered = await Promise.all(prepared.commands.map(deliver));
        if (!mounted.current) return;
        const phases = new Set(delivered.map((d) => d.phase));
        if (phases.size === 1 && phases.has("recorded")) {
          update({ phase: "recorded", recordedAt: nowRef.current });
          for (const d of delivered) if (d.phase === "recorded") void follow(d.eventId);
          return;
        }
        if (phases.size === 1 && phases.has("undelivered")) return update({ phase: "undelivered" });
        update({ phase: "unknown" });
        for (const d of delivered) if (d.phase !== "undelivered" && d.eventId) void follow(d.eventId);
      })();
      return command;
    },
    [client, deliver, follow],
  );

  const respond = useCallback(
    (approvalId: string, response: "approve" | "skip", record: ApprovalRecord) => {
      const entry: ApprovalResponse = { approvalId, response, phase: "sent", sentAt: nowRef.current, record };
      const update = (patch: Partial<ApprovalResponse>) => setResponses((map) => ({ ...map, [approvalId]: { ...entry, ...map[approvalId], ...patch } }));
      setResponses((map) => ({ ...map, [approvalId]: entry }));
      const approval = lastRead.current?.view.approvals.find((a) => a.approval_id === approvalId);
      const prepared = approval ? prepareApprovalResponse(client, approval, response, record) : ({ ok: false, reason: "not_found" } as const);
      void (async () => {
        if (!prepared.ok) return update({ phase: "undelivered", reason: prepared.reason });
        const [delivered] = await Promise.all(prepared.commands.map(deliver));
        if (!mounted.current) return;
        if (delivered.phase === "undelivered") return update({ phase: "undelivered", reason: delivered.reason });
        if (delivered.phase === "unknown") {
          update({ phase: "unknown" });
          if (delivered.eventId) void follow(delivered.eventId);
          return;
        }
        update({ phase: "recorded", recordedAt: nowRef.current });
        const settled = await follow(delivered.eventId);
        if (response === "approve" && settled === "applied" && mounted.current) update({ phase: "decided" });
      })();
    },
    [client, deliver, follow],
  );

  /** Set-up and mandate changes go through drafts, versions and confirmation (spec §4.1), which this slice does not send. */
  const deploy = useCallback<Runtime["deploy"]>((request, record) => {
    counter.current += 1;
    const deployment: Deployment = { id: `dep-${counter.current}`, phase: "sent", sentAt: nowRef.current, agentId: "", version: mandateVersion(request.mandate), record };
    setDeployments((list) => [...list, deployment]);
    window.setTimeout(() => setDeployments((list) => list.map((d) => (d.id === deployment.id ? { ...d, phase: "undelivered", reason: "not_sent" } : d))), 0);
    return deployment;
  }, []);

  const changeMandate = useCallback<Runtime["changeMandate"]>((proposal, origin, record) => {
    counter.current += 1;
    const request: MandateChangeRequest = { id: `chg-${counter.current}`, agentId: proposal.agentId, version: proposal.version, number: proposal.number, phase: "sent", sentAt: nowRef.current, origin, record };
    setMandateChanges((list) => [...list, request]);
    window.setTimeout(() => setMandateChanges((list) => list.map((c) => (c.id === request.id ? { ...c, phase: "undelivered", reason: "not_sent" } : c))), 0);
    return request;
  }, []);

  const value = useMemo<Runtime>(
    () => ({ source: "api", view, ws, now, reachable: ws.status !== "unreachable", commands, responses, deployments, mandateChanges, send, respond, deploy, changeMandate }),
    [view, ws, now, commands, responses, deployments, mandateChanges, send, respond, deploy, changeMandate],
  );
  return <RuntimeContext.Provider value={value}>{children}</RuntimeContext.Provider>;
}
