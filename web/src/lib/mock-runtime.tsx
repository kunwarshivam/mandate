"use client";

import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ActiveRestriction, Agent, AgentMode, Approval, CancelReason, ContentRef, Iso, ShownEnvironment, Workspace } from "@/fixtures/types";
import { FIRST_VERSION_CLASSIFICATION, type NewAgent, addMs, agentIdFor, checkDeploy, deployAgent, fillApproved, firstProposal, mandateVersion, noteSkip, submitApproved } from "./fixture-journey";
import { clock } from "./format";
import { type Origin, type Proposal, type Recorded, reachSafePoint, recordChange } from "./mandate-change";
import { RESTRICTIONS } from "./restrictions";
import { type BacktestRead, backtestNotFound, servedBacktest } from "@/api/backtest";
import { backtestOfVersion, servedBacktestFixture } from "@/fixtures/backtests";
import type { ApiView, ReadPosition } from "@/api/read-models";
import type { Enrolled, ListedPasskey } from "@/api/enrol";
import type { CatalogueAnswer } from "@/api/catalogue";
import type { JournalRecord } from "@/api/records";
import type { Applies, Classification } from "@/api/confirm";
import type { ShownChallenge, StepUpAction, StepUpChallenge } from "@/api/step-up";
import type { AgentHolding, AgentOrder, JournalLine } from "@/api/activity";
import type { ConnectionDetail } from "@/api/connect";
import type { Alerts } from "@/api/alerts";

/**
 * A stand-in for the workspace deployment. Commands and approval responses are held in memory
 * only and become visible as recorded after a delay, the way the runtime journals them: nothing on
 * screen changes optimistically, and nothing is written to browser storage.
 */

export type CommandKind = "pause" | "resume" | "stop" | "kill" | "release" | "pause_all" | "stop_all" | "close_all";

/**
 * What a record screen showed when the owner confirmed, sent with the command so the journal keeps
 * it (brief §4.1). `shown` is every line on the screen, in order; nothing on a record screen collapses.
 * `modes` is each agent's mode badge as it read.
 */
export interface CommandRecord {
  screen: "D10" | "D11";
  environment: ShownEnvironment;
  title: string;
  shown: string[];
  modes: Array<{ agent: string; badge: string }>;
}

export interface Command {
  id: string;
  kind: CommandKind;
  agentId: string | null;
  /** `unknown`: the deployment took the command but no journal entry came back. */
  phase: "sent" | "recorded" | "undelivered" | "unknown";
  sentAt: Iso;
  recordedAt?: Iso;
  record?: CommandRecord;
  /** Over the API, a resume or Stop waits for the owner's passkey before anything is sent (DEC-2140). */
  awaitingPasskey?: boolean;
  /** Why it was not sent, as the passkey check or the deployment said; a code, never shown as one. */
  reason?: string;
  /** Where the runtime settled it once its status was read (§5.5); `unconfirmed` when the reads ran out first. */
  settled?: "applied" | "refused" | "ended" | "unconfirmed";
  /** Over the API, a resume or Stop shows its effect only once the runtime applied it (DEC-2102 item 4). */
  awaitsRuntime?: boolean;
}

/**
 * What D6 showed when the owner responded (brief §4.1). `shown` is every line of the request as it
 * was fixed at first render. D6 lets model output sit behind "View model output", so the record says
 * whether the owner opened it, and when they did, its lines follow in `shown`.
 */
export interface ApprovalRecord {
  screen: "D6";
  environment: ShownEnvironment;
  shown: string[];
  modelOutputExpanded: boolean;
}

export interface ApprovalResponse {
  approvalId: string;
  response: "approve" | "skip";
  /**
   * `undelivered`: the deployment answered that nothing was recorded (`effect: none`), for the reason
   * in `reason`. `refused`: recorded, and the runtime refused it (`ApprovalResponded` refused), for
   * the reason in `reason`; nothing was approved, and the answer is not sent again.
   */
  phase: "sent" | "recorded" | "decided" | "refused" | "undelivered" | "unknown";
  sentAt: Iso;
  recordedAt?: Iso;
  reason?: string;
  record: ApprovalRecord;
}

/**
 * One step of setting up an agent, as its own record (DEC-2401). `recorded` once the journal has it;
 * `rejected`: refused, and `reason` says why in words (`rule` names the rule for the record, never the
 * sentence, C-6); `undelivered`: nothing was recorded, for `reason` (`not_sent` when the deployment
 * does not take this step from here yet); `unknown`: taken, with no journal entry back.
 */
export type StepPhase = "sent" | "recorded" | "rejected" | "undelivered" | "unknown";

export interface SetUpStep {
  phase: StepPhase;
  sentAt: Iso;
  recordedAt?: Iso;
  reason?: string;
  rule?: string;
  /** Over the API, while the owner's passkey check is open: nothing has been sent to the deployment yet. */
  awaitingPasskey?: boolean;
}

/** Step 1, `MandateVersionCreated` (spec §4.1): the version, its base, its classification against it, and its validation. */
export interface VersionStep extends SetUpStep {
  /** The version it is classified against; a new agent's first version has none. */
  base: ContentRef | null;
  /** Set once recorded. */
  classification: Classification | null;
  /** True once recorded: a version is created only from a passing validation. */
  validated: boolean;
}

/**
 * What A5 showed when the owner confirmed (brief §4.1, mandate spec §10): every line of the record
 * screen in order, every path it confirmed (journal rule 18), and the classification it showed.
 */
export interface ConfirmRecord {
  screen: "A5";
  environment: ShownEnvironment;
  shown: string[];
  paths: string[];
  classification: Classification;
}

/** Step 2, `MandateConfirmed` (spec §5.1). */
export interface ConfirmStep extends SetUpStep {
  record: ConfirmRecord;
  /** The digest the passkey check was bound to, or `null` when none ran. */
  stepUp: ContentRef | null;
  /** The server's classification and when the version applies, once the API answers (§5.1). */
  classification?: Classification;
  applies?: Applies;
}

/** What B3 showed when the owner deployed: every line, and the backtest it showed, if any. Paper only. */
export interface DeployToPaperRecord {
  screen: "B3";
  environment: "paper";
  shown: string[];
  backtestId: string | null;
}

/** Step 3, `AgentDeployed` or `DeploymentRejected` (spec §4.2). */
export interface DeployStep extends SetUpStep {
  record: DeployToPaperRecord;
}

/** One agent's set-up: its version and the three steps, each shown only once the one before it is recorded. */
export interface SetUp {
  id: string;
  /** The agent's id once deployed; `""` where the deployment assigns it. */
  agentId: string;
  version: ContentRef;
  created: VersionStep;
  confirmed: ConfirmStep | null;
  deployed: DeployStep | null;
}

/** What the change review showed when the owner confirmed (brief §4.1): every line, in order. */
export interface ChangeRecord {
  screen: "A6";
  environment: ShownEnvironment;
  shown: string[];
}

export interface MandateChangeRequest {
  id: string;
  agentId: string;
  version: ContentRef;
  number: number;
  /**
   * `waiting`: recorded as a risk-increasing version, which applies at the agent's next safe point.
   * `rejected`: refused when it was recorded or applied, and `reason` says why.
   * `unknown`: the deployment took it but no journal entry came back.
   */
  phase: "sent" | "waiting" | "applied" | "rejected" | "undelivered" | "unknown";
  sentAt: Iso;
  recordedAt?: Iso;
  appliedAt?: Iso;
  reason?: string;
  origin: Origin;
  record: ChangeRecord;
}

export interface Runtime {
  /** Where the workspace comes from (DEC-975): the recorded fixtures, or a workspace deployment's API. */
  source: "fixtures" | "api";
  /** The workspace the API runtime reads, which its stream ids name (§4.8.1); `null` over the fixtures. */
  workspaceId: string | null;
  /** The workspace API's read models as served (DEC-1096); `null` over the fixtures, and until the reads answer. */
  view: ApiView | null;
  ws: Workspace;
  now: Iso;
  reachable: boolean;
  commands: Command[];
  responses: Record<string, ApprovalResponse>;
  mandateChanges: MandateChangeRequest[];
  send: (kind: CommandKind, agentId: string | null, record?: CommandRecord) => Command;
  /** An approve over the API carries the challenge the owner confirmed on the host (DEC-2104); the fixtures take none. */
  respond: (approvalId: string, response: "approve" | "skip", record: ApprovalRecord, challenge?: StepUpChallenge | null) => void;
  /** Asks the deployment for a `cli_confirm` challenge (DEC-2103, DEC-2104); `null` over the fixtures, whose step-up is the passkey stand-in. */
  stepUp: ((action: StepUpAction) => Promise<ChallengeAnswer>) | null;
  /** The Alpaca paper connect over the API (§4.5, DEC-2623); `null` over the fixtures, which connect nothing. */
  connect: ConnectPort | null;
  /** The owner's request to move an agent to real money (DEC-2134); `null` over the fixtures, which have no deployment to ask. */
  realMoney: RealMoneyPort | null;
  /** Whether the owner accepted a disclosure on the host, read from the control stream (DEC-2134); `null` over the fixtures. */
  disclosures: DisclosurePort | null;
  /** An agent's orders and own positions over the API (§4.7 D3, DEC-2111); `null` over the fixtures, whose screens show their own. */
  activity: ((agentId: string) => Promise<ActivityAnswer>) | null;
  /** An agent's own positions alone (`GET /agents/{id}/positions`, DEC-2111), for the positions screen; `null` over the fixtures. */
  holdings: ((agentId: string) => Promise<PositionsAnswer>) | null;
  /** The registered instruments and signal models a version may name (§4.1, DEC-2136); `null` over the fixtures, whose set-up uses the fixture catalogue. */
  catalogue: (() => Promise<CatalogueAnswer>) | null;
  /**
   * The owner's exit from one instrument (`POST /agents/{id}/exits`, §5.4, rule 13), followed to its
   * outcome; `onRecorded` runs once the journal holds it. `null` over the fixtures.
   */
  ownerExit: ((agentId: string, instrument: string, record: CommandRecord, onRecorded: () => void) => Promise<ExitOutcome>) | null;
  /** A stream's latest `count` records with their bodies, each checked against its hash (§4.8.1), oldest first; `null` over the fixtures. */
  streamRecords: ((streamId: string, count: number) => Promise<RecordsAnswer>) | null;
  /** A stream's latest `count` records, oldest first (§4.8.1), for the audit screens; `null` over the fixtures. */
  streamTail: ((streamId: string, count: number) => Promise<ActivityAnswer["journal"]>) | null;
  /** The owner alerts about the workspace's agents and accounts (`GET /alerts`, DEC-2141); `null` over the fixtures. */
  alerts: (() => Promise<AlertsAnswer>) | null;
  /** The member's workspace passkeys (§3.6); `null` over the fixtures. */
  passkeys: PasskeyPort | null;
  setUps: SetUp[];
  /** Step 1: `POST /mandate-drafts/{id}/versions`. */
  createVersion: (request: NewAgent) => SetUp;
  /** Step 2: `POST /mandate-versions/{hash}/confirm`, with the digest the passkey check was bound to. */
  confirmVersion: (setUpId: string, record: ConfirmRecord, stepUp: ContentRef | null) => void;
  /** Step 3: `POST /agents`, to paper only. */
  deployVersion: (setUpId: string, record: DeployToPaperRecord) => void;
  /** The completed backtest of a version the screen shows at deployment, or `null`. */
  backtestFor: (version: ContentRef) => string | null;
  changeMandate: (proposal: Proposal, origin: Origin, record: ChangeRecord) => MandateChangeRequest;
  /** One backtest (`GET /backtests/{id}`, DEC-2400): the fixture's served body or the API's answer, through the same decoder. */
  readBacktest: (backtestId: string) => Promise<BacktestRead>;
}

/** A challenge to show, with what the host command names; or why there is none, and nothing was asked of the host. */
export type ChallengeAnswer = { ok: true; shown: ShownChallenge; workspaceId: string; principalId: string } | { ok: false; reason: string };

/** Where the start sends the browser, or why it does not, with whether anything may have been recorded. */
export type StartAnswer = { ok: true; authorizationUrl: string; connectionId: string } | { ok: false; reason: string; effect: "none" | "recorded" | "unknown" };

export type ConnectionAnswer = { ok: true; connection: ConnectionDetail } | { ok: false; reason: string };

/** What a request for real money came to: refused with the deployment's code, or not known. */
export type RealMoneyAnswer = { outcome: "refused"; code: string } | { outcome: "unknown" };

/** Whether a disclosure's version is accepted: `unknown` when the journal could not be read (a role without the audit read, or no answer). */
export type Acceptance = "accepted" | "not_accepted" | "unknown";

export interface DisclosurePort {
  accepted: (document: string, version: string) => Promise<Acceptance>;
  /** The workspace and the signed-in principal the host command names; `null` when the session does not say. */
  owner: () => Promise<{ workspaceId: string; principalId: string } | null>;
}

/** The real-money request (`POST /agents/{id}/go-live`, DEC-2134). It is always refused until counsel signs off. */
export interface RealMoneyPort {
  request: (agentId: string) => Promise<RealMoneyAnswer>;
}

export interface ConnectPort {
  /** A `cli_confirm` challenge of kind `connection`, bound to this workspace and the disclosure shown (DEC-2621 item 4, DEC-2125). */
  challenge: () => Promise<ChallengeAnswer>;
  /** `POST /connections/oauth/start` with the confirmed challenge's evidence, retried with the same key while its answer is lost (DEC-2623 item 8). */
  start: (challenge: StepUpChallenge) => Promise<StartAnswer>;
  /** `GET /connections/{id}` (DEC-2620). */
  read: (connectionId: string) => Promise<ConnectionAnswer>;
  /** `GET /connections`. */
  list: () => Promise<{ ok: true; connections: ConnectionDetail[] } | { ok: false; reason: string }>;
}

/**
 * Where an owner exit came to: settled by the agent's runtime (`applied`, `refused`, `ended`), recorded
 * with no settled outcome read yet, not delivered with the refusal's code, or not known.
 */
export type ExitOutcome = { phase: "applied" | "refused" | "ended" | "recorded" } | { phase: "undelivered"; reason: string } | { phase: "unknown" };

/** The owner alerts the API served (DEC-2141), or why not. */
export type AlertsAnswer = { ok: true; read: Alerts } | { ok: false; reason: string };

/** A stream's records with their bodies, and whether they reach back to the stream's first record; or why they were not read. */
export type RecordsAnswer = { ok: true; items: JournalRecord[]; fromStart: boolean } | { ok: false; reason: string };

/** An agent's own positions with its account's holdings from the same read (DEC-2111), or why not. */
export type PositionsAnswer = { ok: true; account_ref: string | null; items: AgentHolding[]; account_positions: ReadPosition[] | null } | { ok: false; reason: string };

/** The member's own workspace passkeys over the API (§3.6): the list, and adding one. */
export interface PasskeyPort {
  list: () => Promise<{ ok: true; passkeys: ListedPasskey[] } | { ok: false; reason: string }>;
  /** Registers a new passkey; `unknown` when the enrolment may have been recorded without an answer. */
  enrol: () => Promise<{ ok: true; enrolled: Enrolled } | { ok: false; reason: string; effect: "none" | "unknown" }>;
}

/** Each list as served, or not reported: a list that cannot be read is never shown as empty. */
export interface ActivityAnswer {
  orders: { ok: true; account_ref: string | null; items: AgentOrder[] } | { ok: false; reason: string };
  holdings: { ok: true; account_ref: string | null; items: AgentHolding[] } | { ok: false; reason: string };
  /** The agent stream's latest events, newest last; `forbidden` for a role that may not read records (§4.8.1). */
  journal: { ok: true; items: JournalLine[] } | { ok: false; reason: string };
}

export const RuntimeContext = createContext<Runtime | null>(null);

const SEVERITY: Record<AgentMode, number> = { normal: 0, exits_only: 1, paused: 2, stopped: 3 };

export function effectiveMode(restrictions: ActiveRestriction[]): AgentMode {
  let mode: AgentMode = "normal";
  for (const r of restrictions) {
    const imposed = RESTRICTIONS[r.code].mode;
    if (imposed && SEVERITY[imposed] > SEVERITY[mode]) mode = imposed;
  }
  return mode;
}

function note(ws: Workspace, agentId: string, at: Iso, text: string, kind: "mode" | "order" = "mode") {
  const list = ws.timeline[agentId] ?? [];
  ws.timeline[agentId] = [{ event_id: `mock-${agentId}-${list.length}-${at}`, at, kind, text }, ...list];
}

function cancelApprovals(ws: Workspace, agentId: string, at: Iso, reason: CancelReason, text: string) {
  for (const a of ws.approvals) {
    if (a.agent_id === agentId && a.status === "delivered") {
      a.status = "superseded";
      a.resolution = { at, text, cancel_reason: reason };
    }
  }
}

function applyToAgent(ws: Workspace, agent: Agent, kind: CommandKind, at: Iso) {
  if (agent.mode === "stopped") return;
  const time = clock(at);
  switch (kind) {
    case "pause":
    case "pause_all": {
      if (!agent.restrictions.some((r) => r.code === "owner_pause")) agent.restrictions.push({ code: "owner_pause", since: at });
      agent.mode = effectiveMode(agent.restrictions);
      cancelApprovals(ws, agent.agent_id, at, "owner_pause", "Canceled: you paused the agent.");
      note(ws, agent.agent_id, at, `Paused by you at ${time}. Resting protection stays in place.`);
      return;
    }
    case "resume": {
      agent.restrictions = agent.restrictions.filter((r) => r.code !== "owner_pause");
      agent.mode = effectiveMode(agent.restrictions);
      note(ws, agent.agent_id, at, `Resumed by you at ${time}.`);
      return;
    }
    case "stop": {
      agent.mode = "stopped";
      agent.restrictions = [{ code: "stopped", since: at }];
      agent.orders = [];
      cancelApprovals(ws, agent.agent_id, at, "owner_stop", "Canceled: you stopped the agent.");
      note(ws, agent.agent_id, at, `Stopped by you at ${time}. The agent held no positions.`);
      return;
    }
    case "kill":
    case "stop_all":
    case "close_all": {
      const orders = agent.orders.length;
      const positions = agent.positions.length;
      agent.mode = "stopped";
      agent.restrictions = [{ code: "stopped", since: at }];
      agent.orders = [];
      agent.positions = [];
      cancelApprovals(ws, agent.agent_id, at, "kill_switch", "Canceled: a kill switch was used.");
      note(
        ws,
        agent.agent_id,
        at,
        `Kill switch at ${time}: ${orders} ${orders === 1 ? "order" : "orders"} canceled; ${positions} ${positions === 1 ? "position" : "positions"} sold (fixture fills). Agent stopped.`,
      );
      return;
    }
    case "release": {
      for (const p of agent.positions) ws.external_positions.push({ instrument: p.instrument, qty: p.qty });
      const positions = agent.positions.length;
      agent.mode = "stopped";
      agent.restrictions = [{ code: "stopped", since: at }];
      agent.orders = [];
      agent.positions = [];
      cancelApprovals(ws, agent.agent_id, at, "owner_stop", "Canceled: you stopped the agent.");
      note(ws, agent.agent_id, at, `Stopped by you at ${time}; ${positions} ${positions === 1 ? "position" : "positions"} released to you without protection.`);
      return;
    }
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

function applyCommand(ws: Workspace, command: Command, at: Iso): Workspace {
  const next = structuredClone(ws);
  const targets = command.agentId ? next.agents.filter((a) => a.agent_id === command.agentId) : next.agents;
  for (const agent of targets) applyToAgent(next, agent, command.kind, at);
  if (command.kind === "close_all") next.external_positions = [];
  return next;
}

function applyResponse(ws: Workspace, response: ApprovalResponse, at: Iso, phase: "recorded" | "decided"): Workspace {
  const next = structuredClone(ws);
  const approval = next.approvals.find((a) => a.approval_id === response.approvalId);
  if (!approval || approval.status !== "delivered") return next;
  if (response.response === "skip") {
    approval.status = "rejected";
    approval.resolution = { at, text: "Skipped by you. Nothing was sent." };
    return next;
  }
  if (phase === "recorded") {
    approval.approvals_so_far = [...approval.approvals_so_far, { user_label: "You", at }];
    return next;
  }
  if (approval.approvals_so_far.length >= approval.approvers_required) {
    approval.status = "acted";
    approval.resolution = { at, text: "Approved. The gate re-ran and allowed it; the order was submitted to the paper broker." };
  }
  return next;
}

/** An approval as it stands at `now`: past its deadline without a recorded response, it is skipped. */
export function approvalAt(approval: Approval, now: Iso): Approval {
  if (approval.status !== "delivered" || Date.parse(now) < Date.parse(approval.deadline)) return approval;
  return { ...approval, status: "expired", resolution: { at: approval.deadline, text: "Skipped at the deadline. Nothing was sent." } };
}

/**
 * The requests waiting for the owner at `now`, soonest deadline first: what Home's Needs you lists, and
 * one half of what keeps Home and Alerts from saying all clear (`nothingNeedsYou`).
 */
export function openRequests(ws: Workspace, now: Iso): Approval[] {
  return ws.approvals
    .map((a) => approvalAt(a, now))
    .filter((a) => a.status === "delivered")
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
}

export function RuntimeProvider({
  initial,
  children,
  recordAfterMs = 1600,
  tick = true,
  offline = false,
}: {
  initial: Workspace;
  children: ReactNode;
  recordAfterMs?: number;
  tick?: boolean;
  /**
   * Holds no deployment at all, so every command and answer is undelivered, whatever `initial`'s
   * status: the API providers' stand-in while no workspace is named, which must never show a record.
   */
  offline?: boolean;
}) {
  const [ws, setWs] = useState(initial);
  const [now, setNow] = useState(initial.now);
  const [commands, setCommands] = useState<Command[]>([]);
  const [responses, setResponses] = useState<Record<string, ApprovalResponse>>({});
  const [mandateChanges, setMandateChanges] = useState<MandateChangeRequest[]>([]);
  const nowRef = useRef(initial.now);
  const wsRef = useRef(initial);
  const counter = useRef(0);

  /** Every change to the workspace, applied in order to the latest one, so a check made before it never reads a stale copy. */
  const change = useCallback((apply: (current: Workspace) => Workspace) => {
    wsRef.current = apply(wsRef.current);
    setWs(wsRef.current);
  }, []);
  const recorded = useCallback((apply: (current: Workspace) => { ws: Workspace; recorded: Recorded } | null): Recorded | null => {
    const result = apply(wsRef.current);
    if (!result) return null;
    wsRef.current = result.ws;
    setWs(result.ws);
    return result.recorded;
  }, []);
  const reachable = !offline && initial.status !== "unreachable";
  const silent = initial.journal === "silent";
  /** A version waiting for a safe point is checked again until it settles, so the checks stop with the provider. */
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!tick) return;
    const started = Date.now();
    const id = window.setInterval(() => {
      const next = addMs(initial.now, Date.now() - started);
      nowRef.current = next;
      setNow(next);
    }, 1000);
    return () => window.clearInterval(id);
  }, [initial.now, tick]);

  const send = useCallback(
    (kind: CommandKind, agentId: string | null, record?: CommandRecord): Command => {
      counter.current += 1;
      const command: Command = { id: `cmd-${counter.current}`, kind, agentId, phase: "sent", sentAt: nowRef.current, ...(record ? { record } : {}) };
      setCommands((list) => [...list, command]);
      window.setTimeout(() => {
        const at = nowRef.current;
        if (!reachable) {
          setCommands((list) => list.map((c) => (c.id === command.id ? { ...c, phase: "undelivered" } : c)));
          return;
        }
        if (silent) {
          setCommands((list) => list.map((c) => (c.id === command.id ? { ...c, phase: "unknown" } : c)));
          return;
        }
        change((current) => applyCommand(current, command, at));
        setCommands((list) => list.map((c) => (c.id === command.id ? { ...c, phase: "recorded", recordedAt: at } : c)));
      }, recordAfterMs);
      return command;
    },
    [reachable, silent, recordAfterMs, change],
  );

  const respond = useCallback(
    (approvalId: string, response: "approve" | "skip", record: ApprovalRecord) => {
      const entry: ApprovalResponse = { approvalId, response, phase: "sent", sentAt: nowRef.current, record };
      setResponses((map) => ({ ...map, [approvalId]: entry }));
      window.setTimeout(() => {
        const at = nowRef.current;
        if (silent) {
          setResponses((map) => ({ ...map, [approvalId]: { ...entry, phase: "unknown" } }));
          return;
        }
        change((current) => noteSkip(applyResponse(current, entry, at, "recorded"), approvalId, at));
        setResponses((map) => ({ ...map, [approvalId]: { ...entry, phase: "recorded", recordedAt: at } }));
        if (response === "skip") return;
        window.setTimeout(() => {
          const decidedAt = nowRef.current;
          change((current) => submitApproved(applyResponse(current, entry, decidedAt, "decided"), approvalId, decidedAt));
          setResponses((map) => ({ ...map, [approvalId]: { ...entry, phase: "decided", recordedAt: at } }));
          window.setTimeout(() => {
            const filledAt = nowRef.current;
            change((current) => fillApproved(current, approvalId, filledAt));
          }, recordAfterMs);
        }, recordAfterMs * 0.75);
      }, recordAfterMs);
    },
    [silent, recordAfterMs, change],
  );

  const changeMandate = useCallback(
    (proposal: Proposal, origin: Origin, record: ChangeRecord): MandateChangeRequest => {
      counter.current += 1;
      const request: MandateChangeRequest = {
        id: `chg-${counter.current}`,
        agentId: proposal.agentId,
        version: proposal.version,
        number: proposal.number,
        phase: "sent",
        sentAt: nowRef.current,
        origin,
        record,
      };
      setMandateChanges((list) => [...list, request]);
      const update = (patch: Partial<MandateChangeRequest>) => setMandateChanges((list) => list.map((c) => (c.id === request.id ? { ...c, ...patch } : c)));
      const settle = (outcome: Recorded, at: Iso) => {
        switch (outcome.result) {
          case "applied":
            return update({ phase: "applied", appliedAt: at });
          case "rejected":
            return update({ phase: "rejected", reason: outcome.reason });
          case "pending":
            return update({ phase: "waiting" });
          default: {
            const unhandled: never = outcome;
            throw new Error(`unhandled outcome ${JSON.stringify(unhandled)}`);
          }
        }
      };
      const nextCheck = () =>
        window.setTimeout(() => {
          if (!mounted.current) return;
          const at = nowRef.current;
          const outcome = recorded((current) => reachSafePoint(current, proposal.agentId, proposal.version, origin, at));
          if (outcome) settle(outcome, at);
          else nextCheck();
        }, recordAfterMs * 2);
      window.setTimeout(() => {
        const at = nowRef.current;
        if (!reachable) return update({ phase: "undelivered" });
        if (silent) return update({ phase: "unknown" });
        const outcome = recorded((current) => recordChange(current, proposal, origin, at));
        if (!outcome) return;
        update({ recordedAt: at });
        settle(outcome, at);
        if (outcome.result === "pending") nextCheck();
      }, recordAfterMs);
      return request;
    },
    [reachable, silent, recordAfterMs, recorded],
  );

  const [setUps, setSetUps] = useState<SetUp[]>([]);
  /** The set-ups as last written, and each one's mandate and seed, so a step reads the step before it as recorded. */
  const setUpsRef = useRef<SetUp[]>([]);
  const drafts = useRef(new Map<string, { request: NewAgent; seed: string }>());
  const patchSetUp = useCallback((id: string, patch: (s: SetUp) => SetUp) => {
    setUpsRef.current = setUpsRef.current.map((s) => (s.id === id ? patch(s) : s));
    setSetUps(setUpsRef.current);
  }, []);

  /**
   * Step 1: the version is created only from a passing validation, which repeats V-047, V-002 and
   * V-006 against the workspace as it stands; a first version has no base, and is classified as
   * `FIRST_VERSION_CLASSIFICATION`.
   */
  const createVersion = useCallback(
    (request: NewAgent): SetUp => {
      counter.current += 1;
      const seed = `${initial.scenario}:${counter.current}`;
      const setUp: SetUp = {
        id: `set-${counter.current}`,
        agentId: agentIdFor(request.mandate, seed),
        version: mandateVersion(request.mandate),
        created: { phase: "sent", sentAt: nowRef.current, base: null, classification: null, validated: false },
        confirmed: null,
        deployed: null,
      };
      drafts.current.set(setUp.id, { request, seed });
      setUpsRef.current = [...setUpsRef.current, setUp];
      setSetUps(setUpsRef.current);
      const created = (patch: Partial<VersionStep>) => patchSetUp(setUp.id, (s) => ({ ...s, created: { ...s.created, ...patch } }));
      window.setTimeout(() => {
        const at = nowRef.current;
        if (!reachable) return created({ phase: "undelivered", reason: "unreachable" });
        if (silent) return created({ phase: "unknown" });
        const check = checkDeploy(wsRef.current, request.mandate);
        if (!check.ok) return created({ phase: "rejected", recordedAt: at, reason: check.reason, ...(check.rule ? { rule: check.rule } : {}) });
        created({ phase: "recorded", recordedAt: at, classification: FIRST_VERSION_CLASSIFICATION, validated: true });
      }, recordAfterMs);
      return setUp;
    },
    [initial.scenario, reachable, silent, recordAfterMs, patchSetUp],
  );

  /**
   * Step 2, as the API judges it (§5.1): a version not created is refused first, and a risk-increasing
   * one needs the step-up, which the in-memory journal takes from the passkey check (DEC-2401 item 3).
   */
  const confirmVersion = useCallback(
    (setUpId: string, record: ConfirmRecord, stepUp: ContentRef | null) => {
      const setUp = setUpsRef.current.find((s) => s.id === setUpId);
      if (!setUp) return;
      const confirmed = (patch: Partial<ConfirmStep>) => patchSetUp(setUpId, (s) => ({ ...s, confirmed: s.confirmed ? { ...s.confirmed, ...patch } : null }));
      /** What the journal holds when the confirm arrives: a version not yet recorded was not created. */
      const created = setUp.created;
      patchSetUp(setUpId, (s) => ({ ...s, confirmed: { phase: "sent", sentAt: nowRef.current, record, stepUp } }));
      window.setTimeout(() => {
        const at = nowRef.current;
        if (created.phase !== "recorded") return confirmed({ phase: "undelivered", reason: "not_created" });
        if (!reachable) return confirmed({ phase: "undelivered", reason: "unreachable" });
        if (silent) return confirmed({ phase: "unknown" });
        if (created.classification === "risk_increasing" && stepUp === null) return confirmed({ phase: "undelivered", reason: "step_up_required" });
        confirmed({ phase: "recorded", recordedAt: at });
      }, recordAfterMs);
    },
    [reachable, silent, recordAfterMs, patchSetUp],
  );

  /** Step 3: only a confirmed version deploys, and V-047, V-002 and V-006 are checked again as it applies. */
  const deployVersion = useCallback(
    (setUpId: string, record: DeployToPaperRecord) => {
      const setUp = setUpsRef.current.find((s) => s.id === setUpId);
      const draft = drafts.current.get(setUpId);
      if (!setUp || !draft) return;
      const deployed = (patch: Partial<DeployStep>) => patchSetUp(setUpId, (s) => ({ ...s, deployed: s.deployed ? { ...s.deployed, ...patch } : null }));
      /** What the journal holds when the deployment arrives: only a recorded confirmation deploys. */
      const confirmedAtSend = setUp.confirmed?.phase === "recorded";
      patchSetUp(setUpId, (s) => ({ ...s, deployed: { phase: "sent", sentAt: nowRef.current, record } }));
      window.setTimeout(() => {
        const at = nowRef.current;
        if (!confirmedAtSend) return deployed({ phase: "undelivered", reason: "not_confirmed" });
        if (!reachable) return deployed({ phase: "undelivered", reason: "unreachable" });
        if (silent) return deployed({ phase: "unknown" });
        const check = checkDeploy(wsRef.current, draft.request.mandate);
        if (!check.ok) return deployed({ phase: "rejected", recordedAt: at, reason: check.reason, ...(check.rule ? { rule: check.rule } : {}) });
        change((current) => deployAgent(current, draft.request, at, draft.seed).ws);
        deployed({ phase: "recorded", recordedAt: at });
        window.setTimeout(() => change((current) => firstProposal(current, setUp.agentId, nowRef.current, draft.seed)), recordAfterMs * 2);
      }, recordAfterMs);
    },
    [reachable, silent, recordAfterMs, patchSetUp, change],
  );

  /** The fixtures' completed run of exactly this version, if they hold one. */
  const backtestFor = useCallback((version: ContentRef): string | null => backtestOfVersion(version), []);

  /** A fixture backtest in its served shape, through the API's own decoder; an id no fixture holds is the API's 404. */
  const readBacktest = useCallback(async (backtestId: string): Promise<BacktestRead> => {
    const body = servedBacktestFixture(backtestId);
    return body === undefined ? backtestNotFound() : servedBacktest(body, backtestId);
  }, []);

  const value = useMemo(
    () => ({ source: "fixtures" as const, workspaceId: null, view: null, ws, now, reachable, commands, responses, mandateChanges, send, respond, stepUp: null, connect: null, realMoney: null, disclosures: null, activity: null, holdings: null, ownerExit: null, catalogue: null, streamTail: null, streamRecords: null, alerts: null, passkeys: null, setUps, createVersion, confirmVersion, deployVersion, backtestFor, changeMandate, readBacktest }),
    [ws, now, reachable, commands, responses, mandateChanges, send, respond, setUps, createVersion, confirmVersion, deployVersion, backtestFor, changeMandate, readBacktest],
  );
  return <RuntimeContext.Provider value={value}>{children}</RuntimeContext.Provider>;
}

export function useRuntime(): Runtime {
  const runtime = useContext(RuntimeContext);
  if (!runtime) throw new Error("useRuntime outside RuntimeProvider");
  return runtime;
}
