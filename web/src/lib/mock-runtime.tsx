"use client";

import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ActiveRestriction, Agent, AgentMode, Approval, CancelReason, ContentRef, Environment, Iso, Workspace } from "@/fixtures/types";
import { type NewAgent, addMs, agentIdFor, checkDeploy, deployAgent, fillApproved, firstProposal, mandateVersion, noteSkip, submitApproved } from "./fixture-journey";
import { clock } from "./format";
import { type Origin, type Proposal, type Recorded, reachSafePoint, recordChange } from "./mandate-change";
import { RESTRICTIONS } from "./restrictions";

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
  environment: Environment;
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
}

/**
 * What D6 showed when the owner responded (brief §4.1). `shown` is every line of the request as it
 * was fixed at first render. D6 lets model output sit behind "View model output", so the record says
 * whether the owner opened it, and when they did, its lines follow in `shown`.
 */
export interface ApprovalRecord {
  screen: "D6";
  environment: Environment;
  shown: string[];
  modelOutputExpanded: boolean;
}

export interface ApprovalResponse {
  approvalId: string;
  response: "approve" | "skip";
  phase: "sent" | "recorded" | "decided" | "unknown";
  sentAt: Iso;
  recordedAt?: Iso;
  record: ApprovalRecord;
}

/** What A5 showed when the owner confirmed (brief §4.1): every line of the record screen, in order. */
export interface DeployRecord {
  screen: "A5";
  environment: Environment;
  shown: string[];
}

export interface Deployment {
  id: string;
  /**
   * `rejected`: the checks repeated when it applied failed (V-047, V-002, V-006), and `reason` says which.
   * `unknown`: the deployment took it but no journal entry came back.
   */
  phase: "sent" | "recorded" | "rejected" | "undelivered" | "unknown";
  sentAt: Iso;
  recordedAt?: Iso;
  agentId: string;
  version: ContentRef;
  reason?: string;
  /** The rule a rejection falls under, for the record and never the sentence (C-6); set for V-047 only. */
  rule?: string;
  record: DeployRecord;
}

/** What the change review showed when the owner confirmed (brief §4.1): every line, in order. */
export interface ChangeRecord {
  screen: "A6";
  environment: Environment;
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
  ws: Workspace;
  now: Iso;
  reachable: boolean;
  commands: Command[];
  responses: Record<string, ApprovalResponse>;
  deployments: Deployment[];
  mandateChanges: MandateChangeRequest[];
  send: (kind: CommandKind, agentId: string | null, record?: CommandRecord) => Command;
  respond: (approvalId: string, response: "approve" | "skip", record: ApprovalRecord) => void;
  deploy: (request: NewAgent, record: DeployRecord) => Deployment;
  changeMandate: (proposal: Proposal, origin: Origin, record: ChangeRecord) => MandateChangeRequest;
}

const RuntimeContext = createContext<Runtime | null>(null);

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
}: {
  initial: Workspace;
  children: ReactNode;
  recordAfterMs?: number;
  tick?: boolean;
}) {
  const [ws, setWs] = useState(initial);
  const [now, setNow] = useState(initial.now);
  const [commands, setCommands] = useState<Command[]>([]);
  const [responses, setResponses] = useState<Record<string, ApprovalResponse>>({});
  const [deployments, setDeployments] = useState<Deployment[]>([]);
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
  const reachable = initial.status !== "unreachable";
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

  const deploy = useCallback(
    (request: NewAgent, record: DeployRecord): Deployment => {
      counter.current += 1;
      const seed = `${initial.scenario}:${counter.current}`;
      const deployment: Deployment = {
        id: `dep-${counter.current}`,
        phase: "sent",
        sentAt: nowRef.current,
        agentId: agentIdFor(request.mandate, seed),
        version: mandateVersion(request.mandate),
        record,
      };
      setDeployments((list) => [...list, deployment]);
      const update = (patch: Partial<Deployment>) => setDeployments((list) => list.map((d) => (d.id === deployment.id ? { ...d, ...patch } : d)));
      window.setTimeout(() => {
        const at = nowRef.current;
        if (!reachable) return update({ phase: "undelivered" });
        if (silent) return update({ phase: "unknown" });
        const check = checkDeploy(wsRef.current, request.mandate);
        if (!check.ok) return update({ phase: "rejected", recordedAt: at, reason: check.reason, ...(check.rule ? { rule: check.rule } : {}) });
        change((current) => deployAgent(current, request, at, seed).ws);
        update({ phase: "recorded", recordedAt: at });
        window.setTimeout(() => change((current) => firstProposal(current, deployment.agentId, nowRef.current, seed)), recordAfterMs * 2);
      }, recordAfterMs);
      return deployment;
    },
    [initial.scenario, reachable, silent, recordAfterMs, change],
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

  const value = useMemo(
    () => ({ ws, now, reachable, commands, responses, deployments, mandateChanges, send, respond, deploy, changeMandate }),
    [ws, now, reachable, commands, responses, deployments, mandateChanges, send, respond, deploy, changeMandate],
  );
  return <RuntimeContext.Provider value={value}>{children}</RuntimeContext.Provider>;
}

export function useRuntime(): Runtime {
  const runtime = useContext(RuntimeContext);
  if (!runtime) throw new Error("useRuntime outside RuntimeProvider");
  return runtime;
}
