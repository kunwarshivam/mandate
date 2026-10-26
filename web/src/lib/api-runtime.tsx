"use client";

import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { readBacktestFrom } from "@/api/backtest";
import { type Fetch, createWorkspaceClient } from "@/api/client";
import { prepareApprovalResponse, prepareConfirm, prepareDeploy, prepareOwnerCommand } from "@/api/commands";
import { confirmAccepted, confirmStepUpDigest } from "@/api/confirm";
import { type AssertionRefusal, type Authenticator, type PasskeyStepUp, assertPasskey, browserAuthenticator, deployStepUpDigest, passkeysRead } from "@/api/passkey";
import { type ApiView, loadApi } from "@/api/read-models";
import { emptyWorkspace } from "@/api/reads";
import { REFRESH_EVERY_MS, type SessionKeeper, type SessionState } from "@/api/session";
import type { CommandPhase, PreparedCommand } from "@/api/types";
import { activityRead, journalPageRead } from "@/api/activity";
import { connectStartBody, connectionRead, connectionsRead } from "@/api/connect";
import { type PasskeyKind, type StepUpAction, type StepUpChallenge, confirmCode, connectStepUpDigest } from "@/api/step-up";
import { prepareCreateVersion, readConfirmation, versionCreated } from "@/api/versions";
import type { ContentRef, Iso, Workspace } from "@/fixtures/types";
import type { ApiSource } from "./data-source";
import { mandateVersion } from "./fixture-journey";
import { newAgentId } from "./ids";
import {
  type ApprovalRecord,
  type ApprovalResponse,
  type ActivityAnswer,
  type ChallengeAnswer,
  type Command,
  type ConnectPort,
  type CommandKind,
  type CommandRecord,
  type MandateChangeRequest,
  type Runtime,
  RuntimeContext,
  type SetUp,
} from "./mock-runtime";

/** How many of an agent stream's latest events its screen lists. */
const JOURNAL_TAIL = 20;
/** How many times an unanswered command is sent again, with the same key and body (API-4), before it is called unknown. */
const SENDS = 4;
/** How many times a recorded command's status is read before the runtime stops following it (spec §6.3 item 4). */
const STATUS_READS = 30;
const SETTLED: ReadonlySet<CommandPhase> = new Set(["applied", "refused", "ended"]);

type Delivery = { phase: "recorded"; eventId: string; body?: unknown } | { phase: "undelivered"; reason: string } | { phase: "unknown"; eventId: string | null };

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
  authenticator,
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
  /** The WebAuthn call a passkey step-up makes; the browser's own when not given, `null` for a page with none. */
  authenticator?: Authenticator | null;
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
      wsRef.current = { ...emptyWorkspace("membership_unavailable", nowRef.current), environment: lastRead.current?.ws.environment ?? "unknown" };
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
        if (outcome.ok) return { phase: "recorded", eventId: outcome.accepted.event_id, body: outcome.body };
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
    (approvalId: string, response: "approve" | "skip", record: ApprovalRecord, challenge: StepUpChallenge | null = null) => {
      const entry: ApprovalResponse = { approvalId, response, phase: "sent", sentAt: nowRef.current, record };
      const update = (patch: Partial<ApprovalResponse>) => setResponses((map) => ({ ...map, [approvalId]: { ...entry, ...map[approvalId], ...patch } }));
      setResponses((map) => ({ ...map, [approvalId]: entry }));
      const approval = lastRead.current?.view.approvals.find((a) => a.approval_id === approvalId);
      const prepared = approval ? prepareApprovalResponse(client, approval, response, record, challenge) : ({ ok: false, reason: "not_found" } as const);
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

  /**
   * A `cli_confirm` challenge for `action` (§3.6, DEC-2104), with the session's principal the host
   * command names. Asked for only when the owner presses a button that needs it; nothing is retried,
   * since each press is its own challenge.
   */
  const stepUp = useCallback(
    async (action: StepUpAction): Promise<ChallengeAnswer> => {
      const [session, issued] = await Promise.all([client.readSession(), client.askChallenge(action)]);
      if (!issued.ok) return { ok: false, reason: issued.error.code };
      if (!session.ok) return { ok: false, reason: session.error.code };
      const code = confirmCode(issued.value.webauthn_challenge);
      if (code === null) return { ok: false, reason: "invalid_response" };
      return { ok: true, shown: { challenge: issued.value, code }, workspaceId: client.workspaceId, principalId: session.value.principal_id };
    },
    [client],
  );

  /**
   * The Alpaca paper connect (§4.5). The start commits the pending connection, so an answer lost on
   * the way is sent again with the same key and body, which answers the same start (DEC-2623 item 8);
   * it is never sent under a new key, which would be a second connect.
   */
  const connect = useMemo<ConnectPort>(
    () => ({
      challenge: () => stepUp({ kind: "connection", digest: connectStepUpDigest(client.workspaceId) }),
      start: async (challenge) => {
        const body = connectStartBody(challenge);
        if (!body) return { ok: false, reason: "step_up_mismatch", effect: "none" };
        const prepared = client.prepareConnect(body);
        let last: { reason: string; effect: "none" | "recorded" | "unknown" } = { reason: "network", effect: "unknown" };
        for (let attempt = 0; attempt < SENDS; attempt += 1) {
          const outcome = await client.sendConnect(prepared);
          if (outcome.ok) return { ok: true, authorizationUrl: outcome.value.authorization_url, connectionId: outcome.value.connection_id };
          last = { reason: outcome.error.code, effect: outcome.error.effect };
          if (outcome.error.effect !== "unknown" || !outcome.error.retryable) break;
          await wait(retryMs * 2 ** attempt);
        }
        return { ok: false, ...last };
      },
      read: async (connectionId) => {
        const read = await client.read(`/connections/${encodeURIComponent(connectionId)}`, connectionRead);
        return read.ok ? { ok: true, connection: read.value.connection } : { ok: false, reason: read.error.code };
      },
      list: async () => {
        const read = await client.read("/connections", connectionsRead);
        return read.ok ? { ok: true, connections: read.value.connections } : { ok: false, reason: read.error.code };
      },
    }),
    [client, stepUp, retryMs],
  );

  /**
   * The last `JOURNAL_TAIL` events of a stream (§4.8.1): one page of one event finds the head, a second
   * reads the tail up to it. An append between the two only adds to the next read.
   */
  const journalTail = useCallback(
    async (streamId: string): Promise<ActivityAnswer["journal"]> => {
      const route = `/journal/streams/${encodeURIComponent(streamId)}/events` as const;
      const decoder = journalPageRead(streamId);
      const first = await client.read(`${route}?after_seq=0&limit=1`, decoder);
      if (!first.ok) return { ok: false, reason: first.error.code };
      const after = Math.max(0, first.value.head_seq - JOURNAL_TAIL);
      const tail = await client.read(`${route}?after_seq=${after}&limit=${JOURNAL_TAIL}`, decoder);
      return tail.ok ? { ok: true, items: tail.value.events } : { ok: false, reason: tail.error.code };
    },
    [client],
  );

  /** `GET /agents/{id}/orders` and `/positions` (DEC-2111), with the agent stream's tail, read together when the agent's screen asks. */
  const activity = useCallback(
    async (agentId: string): Promise<ActivityAnswer> => {
      const decoders = activityRead(agentId);
      const base = `/agents/${encodeURIComponent(agentId)}` as const;
      const [orders, holdings, journal] = await Promise.all([client.read(`${base}/orders`, decoders.orders), client.read(`${base}/positions`, decoders.holdings), journalTail(`agent:${client.workspaceId}:${agentId}`)]);
      return {
        journal,
        orders: orders.ok ? { ok: true, account_ref: orders.value.account_ref, items: orders.value.items } : { ok: false, reason: orders.error.code },
        holdings: holdings.ok ? { ok: true, account_ref: holdings.value.account_ref, items: holdings.value.items } : { ok: false, reason: holdings.error.code },
      };
    },
    [client, journalTail],
  );

  /** Mandate changes go through drafts, versions and confirmation (spec §4.1), which this slice does not send. */
  const changeMandate = useCallback<Runtime["changeMandate"]>((proposal, origin, record) => {
    counter.current += 1;
    const request: MandateChangeRequest = { id: `chg-${counter.current}`, agentId: proposal.agentId, version: proposal.version, number: proposal.number, phase: "sent", sentAt: nowRef.current, origin, record };
    setMandateChanges((list) => [...list, request]);
    window.setTimeout(() => setMandateChanges((list) => list.map((c) => (c.id === request.id ? { ...c, phase: "undelivered", reason: "not_sent" } : c))), 0);
    return request;
  }, []);

  const [setUps, setSetUps] = useState<SetUp[]>([]);
  const setUpsRef = useRef<SetUp[]>([]);
  const patchSetUp = useCallback((id: string, patch: (s: SetUp) => SetUp) => {
    setUpsRef.current = setUpsRef.current.map((s) => (s.id === id ? patch(s) : s));
    setSetUps(setUpsRef.current);
  }, []);

  /**
   * Step 1, `POST /mandate-versions` (§3.11 item 11, DEC-2101 item 3): the mandate document itself.
   * The version is the one the server hashed, never one computed here; it is recorded only once the
   * server says so, and the served confirmation's classification is the one shown.
   */
  const createVersion = useCallback<Runtime["createVersion"]>(
    (request) => {
      counter.current += 1;
      const setUp: SetUp = {
        id: `set-${counter.current}`,
        agentId: "",
        version: mandateVersion(request.mandate),
        created: { phase: "sent", sentAt: nowRef.current, base: null, classification: null, validated: false },
        confirmed: null,
        deployed: null,
      };
      setUpsRef.current = [...setUpsRef.current, setUp];
      setSetUps(setUpsRef.current);
      const created = (patch: Partial<SetUp["created"]>, version?: SetUp["version"]) =>
        patchSetUp(setUp.id, (s) => ({ ...s, ...(version ? { version } : {}), created: { ...s.created, ...patch } }));
      void (async () => {
        const delivered = await deliver(prepareCreateVersion(client, request.mandate));
        if (!mounted.current) return;
        if (delivered.phase === "undelivered") return created({ phase: "undelivered", reason: delivered.reason });
        if (delivered.phase === "unknown") return created({ phase: "unknown" });
        const answer = versionCreated(delivered.body);
        if (!answer.ok) return created({ phase: "unknown" });
        const version = answer.value.mandate_version;
        const screen = await readConfirmation(client, version);
        if (!mounted.current) return;
        created({ phase: "recorded", recordedAt: nowRef.current, validated: true, classification: screen.ok ? screen.value.classification : null }, version);
      })();
      return setUp;
    },
    [client, deliver, patchSetUp],
  );

  /**
   * Passkey step-up for one action (§3.6, DEC-2128): a challenge bound to `kind` and `digest`, and
   * the member's passkeys, read together; then the authenticator signs the challenge. Each press is
   * its own challenge, never retried. Nothing reaches the journal here: a refusal means nothing was sent.
   */
  const passkeyStepUp = useCallback(
    async (kind: PasskeyKind, digest: ContentRef): Promise<{ ok: true; value: PasskeyStepUp } | { ok: false; reason: AssertionRefusal | string }> => {
      const [issued, held] = await Promise.all([client.askChallenge({ kind, digest }), client.read("/passkeys", passkeysRead)]);
      if (!issued.ok) return { ok: false, reason: issued.error.code };
      if (!held.ok) return { ok: false, reason: held.error.code };
      return assertPasskey(issued.value, held.value.passkeys, authenticator === undefined ? browserAuthenticator() : authenticator, nowRef.current);
    },
    [client, authenticator],
  );

  /**
   * Step 2, `POST /mandate-versions/{hash}/confirm` (§5.1, DEC-2101 item 5), bound to the
   * confirmation the server serves now: it is read again first, since its digest can move. When the
   * server says the confirm needs step-up (a first version is risk-increasing), the owner's passkey
   * signs a `confirm_version` challenge bound to §5.1's digest, and the confirm carries its evidence
   * and assertion (DEC-2128). The browser's `stepUp` argument is the fixtures' stand-in and is not read.
   */
  const confirmVersion = useCallback<Runtime["confirmVersion"]>(
    (setUpId, record) => {
      const setUp = setUpsRef.current.find((s) => s.id === setUpId);
      if (!setUp) return;
      const confirmed = (patch: Partial<NonNullable<SetUp["confirmed"]>>) =>
        patchSetUp(setUpId, (s) => ({ ...s, confirmed: s.confirmed ? { ...s.confirmed, ...patch } : null }));
      patchSetUp(setUpId, (s) => ({ ...s, confirmed: { phase: "sent", sentAt: nowRef.current, record, stepUp: null } }));
      void (async () => {
        if (setUp.created.phase !== "recorded") return confirmed({ phase: "undelivered", reason: "not_created" });
        const screen = await readConfirmation(client, setUp.version);
        if (!mounted.current) return;
        if (!screen.ok) return confirmed({ phase: "undelivered", reason: screen.error.code });
        const served = { screen_digest: screen.value.screen_digest, classification: screen.value.classification };
        let stepUp: PasskeyStepUp | null = null;
        const digest = confirmStepUpDigest(setUp.version, null, null);
        if (screen.value.step_up_required) {
          const asserted = await passkeyStepUp("confirm_version", digest);
          if (!mounted.current) return;
          if (!asserted.ok) return confirmed({ phase: "undelivered", reason: asserted.reason });
          stepUp = asserted.value;
          confirmed({ stepUp: digest });
        }
        const delivered = await deliver(prepareConfirm(client, { version: setUp.version, agentId: null, base: null, record, served, stepUp }));
        if (!mounted.current) return;
        if (delivered.phase === "undelivered") return confirmed({ phase: "undelivered", reason: delivered.reason });
        if (delivered.phase === "unknown") return confirmed({ phase: "unknown" });
        const accepted = confirmAccepted(delivered.body, "");
        confirmed(accepted.ok ? { phase: "recorded", recordedAt: nowRef.current, classification: accepted.value.classification, applies: accepted.value.applies } : { phase: "unknown" });
      })();
    },
    [client, deliver, patchSetUp, passkeyStepUp],
  );

  /**
   * Step 3, `POST /agents` (§4.2, §3.11 item 11), to paper only, under passkey step-up of kind
   * `deploy` bound to the canonical `{agent_id, mandate_version}` (DEC-2128). The agent's id is
   * minted here on the set-up's first deploy and kept for its retries, since an agent id is deployed
   * once (DEC-1173 item 1); the server checks every V-rule for the agent and the instruments before
   * the step-up. It is recorded once the server answers with its `AgentDeployed`.
   */
  const deployVersion = useCallback<Runtime["deployVersion"]>(
    (setUpId, record) => {
      const setUp = setUpsRef.current.find((s) => s.id === setUpId);
      if (!setUp) return;
      const agentId = setUp.agentId || newAgentId(Date.parse(nowRef.current));
      const deployed = (patch: Partial<NonNullable<SetUp["deployed"]>>) =>
        patchSetUp(setUpId, (s) => ({ ...s, deployed: s.deployed ? { ...s.deployed, ...patch } : null }));
      patchSetUp(setUpId, (s) => ({ ...s, agentId, deployed: { phase: "sent", sentAt: nowRef.current, record } }));
      void (async () => {
        if (setUp.confirmed?.phase !== "recorded") return deployed({ phase: "undelivered", reason: "not_confirmed" });
        const asserted = await passkeyStepUp("deploy", deployStepUpDigest(agentId, setUp.version));
        if (!mounted.current) return;
        if (!asserted.ok) return deployed({ phase: "undelivered", reason: asserted.reason });
        const delivered = await deliver(prepareDeploy(client, { agentId, version: setUp.version, record, stepUp: asserted.value }));
        if (!mounted.current) return;
        if (delivered.phase === "undelivered") return deployed({ phase: "undelivered", reason: delivered.reason });
        if (delivered.phase === "unknown") return deployed({ phase: "unknown" });
        deployed({ phase: "recorded", recordedAt: nowRef.current });
        await refresh();
      })();
    },
    [client, deliver, patchSetUp, passkeyStepUp, refresh],
  );

  /** No route lists a version's backtests (DEC-2400), so the deploy step shows none over the API. */
  const backtestFor = useCallback<Runtime["backtestFor"]>(() => null, []);

  /** `GET /backtests/{id}` (DEC-2400): read when a screen asks, not with the workspace's refresh, since no list names them. */
  const readBacktest = useCallback<Runtime["readBacktest"]>((backtestId) => readBacktestFrom(client, backtestId), [client]);

  const value = useMemo<Runtime>(
    () => ({ source: "api", view, ws, now, reachable: ws.status !== "unreachable", commands, responses, mandateChanges, send, respond, stepUp, connect, activity, setUps, createVersion, confirmVersion, deployVersion, backtestFor, changeMandate, readBacktest }),
    [view, ws, now, commands, responses, mandateChanges, send, respond, stepUp, connect, activity, setUps, createVersion, confirmVersion, deployVersion, backtestFor, changeMandate, readBacktest],
  );
  return <RuntimeContext.Provider value={value}>{children}</RuntimeContext.Provider>;
}
