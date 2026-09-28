"use client";

import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ActiveRestriction, Agent, AgentMode, Approval, CancelReason, Environment, Iso, Workspace } from "@/fixtures/types";
import { clock } from "./format";
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

interface Runtime {
  ws: Workspace;
  now: Iso;
  reachable: boolean;
  commands: Command[];
  responses: Record<string, ApprovalResponse>;
  send: (kind: CommandKind, agentId: string | null, record?: CommandRecord) => Command;
  respond: (approvalId: string, response: "approve" | "skip", record: ApprovalRecord) => void;
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

function addSeconds(iso: Iso, ms: number): Iso {
  const offset = iso.slice(-6);
  const shifted = new Date(Date.parse(iso) + ms);
  const local = new Date(shifted.getTime() + offsetMinutes(offset) * 60_000);
  return `${local.toISOString().slice(0, 19)}${offset}`;
}

function offsetMinutes(offset: string): number {
  const sign = offset.startsWith("-") ? -1 : 1;
  const [h, m] = offset.slice(1).split(":").map(Number);
  return sign * (h * 60 + m);
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
  const nowRef = useRef(initial.now);
  const counter = useRef(0);
  const reachable = initial.status !== "unreachable";
  const silent = initial.journal === "silent";

  useEffect(() => {
    if (!tick) return;
    const started = Date.now();
    const id = window.setInterval(() => {
      const next = addSeconds(initial.now, Date.now() - started);
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
        setWs((current) => applyCommand(current, command, at));
        setCommands((list) => list.map((c) => (c.id === command.id ? { ...c, phase: "recorded", recordedAt: at } : c)));
      }, recordAfterMs);
      return command;
    },
    [reachable, silent, recordAfterMs],
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
        setWs((current) => applyResponse(current, entry, at, "recorded"));
        setResponses((map) => ({ ...map, [approvalId]: { ...entry, phase: "recorded", recordedAt: at } }));
        if (response === "skip") return;
        window.setTimeout(() => {
          const decidedAt = nowRef.current;
          setWs((current) => applyResponse(current, entry, decidedAt, "decided"));
          setResponses((map) => ({ ...map, [approvalId]: { ...entry, phase: "decided", recordedAt: at } }));
        }, recordAfterMs * 0.75);
      }, recordAfterMs);
    },
    [silent, recordAfterMs],
  );

  const value = useMemo(
    () => ({ ws, now, reachable, commands, responses, send, respond }),
    [ws, now, reachable, commands, responses, send, respond],
  );
  return <RuntimeContext.Provider value={value}>{children}</RuntimeContext.Provider>;
}

export function useRuntime(): Runtime {
  const runtime = useContext(RuntimeContext);
  if (!runtime) throw new Error("useRuntime outside RuntimeProvider");
  return runtime;
}
