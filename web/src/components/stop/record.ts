import { orderPriceText, protectionText } from "@/components/domain/positions";
import type { Agent, ExternalPosition, Position, WorkingOrder, Workspace } from "@/fixtures/types";
import { quantity } from "@/lib/format";
import { ORDER_STATE_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { type RecordKind, stepUpLine } from "./commands";

export interface RecordList {
  key: "orders" | "positions" | "agents" | "untouched";
  heading: string;
  items: string[];
  empty: string;
  note?: string;
}

/**
 * Everything a kill-switch (D10) or release (D11) screen shows, fixed when the screen first renders
 * so nothing changes underneath the owner while they confirm (brief §4.1).
 */
export interface StopRecord {
  kind: RecordKind;
  title: string;
  /** The agent's label, or "Account" for the whole account; used in the status lines. */
  label: string;
  subject: string;
  scope: string;
  warning: string | null;
  lists: RecordList[];
  afterwards: string;
  /** Why there is nothing to confirm, or null when the action can be taken. */
  nothingToDo: string | null;
  agentId: string | null;
  agentIds: string[];
  stepUp: string;
  back: { href: string; label: string };
}

/** Titles are fixed per screen so the heading and its environment render before any data. */
export const RECORD_TITLE: Record<RecordKind, string> = {
  kill: "Kill switch",
  release: "Stop and release positions",
  stop_all: "Stop all agents",
  close_all: "Close everything",
};

const SESSION_NOTE = "Outside the regular session, stock sells wait for it to open; crypto sells at once.";

function orderLine(o: WorkingOrder, owner?: string): string {
  const priceText = orderPriceText(o);
  return `${owner ? `${owner}: ` : ""}${o.side === "buy" ? "Buy" : "Sell"} ${quantity(o.qty)} ${o.instrument.symbol}${priceText ? `, ${priceText}` : ""} (${PURPOSE_LABEL[o.purpose]}, ${ORDER_STATE_LABEL[o.state].toLowerCase()})`;
}

function positionLine(p: Position, owner?: string): string {
  return `${owner ? `${owner}: ` : ""}${quantity(p.qty)} ${p.instrument.symbol} (${protectionText(p).replace(/\.$/, "")})`;
}

function releaseLine(p: Position): string {
  return `${quantity(p.qty)} ${p.instrument.symbol}. Protected now by ${protectionText(p).replace(/\.$/, "")}. After release: no protection.`;
}

function ownLine(p: ExternalPosition): string {
  return `Your ${quantity(p.qty)} ${p.instrument.symbol}, which no agent manages`;
}

function sellsStock(positions: Position[]): boolean {
  return positions.some((p) => p.instrument.asset_class === "us_equity");
}

const STOPPED = "Stopped. There is nothing more to stop for this agent.";

function killRecord(ws: Workspace, agent: Agent): StopRecord {
  const others = ws.agents.filter((a) => a.agent_id !== agent.agent_id);
  return {
    kind: "kill",
    title: RECORD_TITLE.kill,
    label: agent.label,
    subject: `${agent.label} (${agent.mandate.name})`,
    scope: "Cancels only this agent's orders, sells only its positions, leaves other agents and your own holdings untouched, and ends the agent: it becomes stopped.",
    warning: null,
    lists: [
      { key: "orders", heading: "Orders it cancels", items: agent.orders.map((o) => orderLine(o)), empty: "No open orders, so nothing is canceled." },
      {
        key: "positions",
        heading: "Positions it sells",
        items: agent.positions.map((p) => positionLine(p)),
        empty: "No positions, so nothing is sold.",
        ...(sellsStock(agent.positions) ? { note: SESSION_NOTE } : {}),
      },
      {
        key: "untouched",
        heading: "What it leaves alone",
        items: [...others.map((a) => `${a.label}, with its orders and positions`), ...ws.external_positions.map(ownLine)],
        empty: "Nothing else is on this account.",
      },
    ],
    afterwards: `${agent.label} is stopped for good, and its approval requests are canceled. Each step is journaled and shown below as it is recorded.`,
    nothingToDo: agent.mode === "stopped" ? STOPPED : null,
    agentId: agent.agent_id,
    agentIds: [agent.agent_id],
    stepUp: stepUpLine("kill", agent.label, agent.positions.length),
    back: { href: `/agents/${agent.agent_id}`, label: agent.label },
  };
}

function releaseRecord(ws: Workspace, agent: Agent): StopRecord {
  const others = ws.agents.filter((a) => a.agent_id !== agent.agent_id);
  return {
    kind: "release",
    title: RECORD_TITLE.release,
    label: agent.label,
    subject: `${agent.label} (${agent.mandate.name})`,
    scope: "Ends the agent and hands its positions to you. From then on you manage them yourself; Owlhead does not.",
    warning: `The positions become yours and unprotected: ${agent.label}'s protective orders are canceled and nothing watches them. This warning is recorded with your confirmation.`,
    lists: [
      { key: "positions", heading: "Positions released to you", items: agent.positions.map(releaseLine), empty: "No positions to release." },
      { key: "orders", heading: "Orders it cancels, protection included", items: agent.orders.map((o) => orderLine(o)), empty: "No open orders, so nothing is canceled." },
      {
        key: "untouched",
        heading: "What it leaves alone",
        items: [...others.map((a) => `${a.label}, with its orders and positions`), ...ws.external_positions.map(ownLine)],
        empty: "Nothing else is on this account.",
      },
    ],
    afterwards: `${agent.label} is stopped for good, and its approval requests are canceled. The released positions join your own holdings.`,
    nothingToDo: agent.mode === "stopped" ? STOPPED : agent.positions.length === 0 ? "It holds no positions, so there is nothing to release." : null,
    agentId: agent.agent_id,
    agentIds: [agent.agent_id],
    stepUp: stepUpLine("release", agent.label, agent.positions.length),
    back: { href: `/agents/${agent.agent_id}`, label: agent.label },
  };
}

function stopAllRecord(ws: Workspace): StopRecord {
  const active = ws.agents.filter((a) => a.mode !== "stopped");
  const stopped = ws.agents.filter((a) => a.mode === "stopped");
  const positions = active.flatMap((a) => a.positions);
  return {
    kind: "stop_all",
    title: RECORD_TITLE.stop_all,
    label: "Account",
    subject: `${ws.connection.broker} account`,
    scope: "Each agent's own kill switch: cancels only that agent's orders, sells only its positions, and ends it. Your own holdings are untouched.",
    warning: null,
    lists: [
      { key: "agents", heading: "Agents it stops", items: active.map((a) => `${a.label} (${a.mandate.name})`), empty: "No agent is running." },
      { key: "orders", heading: "Orders it cancels", items: active.flatMap((a) => a.orders.map((o) => orderLine(o, a.label))), empty: "No open orders, so nothing is canceled." },
      {
        key: "positions",
        heading: "Positions it sells",
        items: active.flatMap((a) => a.positions.map((p) => positionLine(p, a.label))),
        empty: "No positions, so nothing is sold.",
        ...(sellsStock(positions) ? { note: SESSION_NOTE } : {}),
      },
      {
        key: "untouched",
        heading: "What it leaves alone",
        items: [...ws.external_positions.map(ownLine), ...stopped.map((a) => `${a.label}, already stopped`)],
        empty: "Nothing else is on this account.",
      },
    ],
    afterwards: "Every agent on this account is stopped for good, and their approval requests are canceled. Each step is journaled and shown below as it is recorded.",
    nothingToDo: active.length === 0 ? "Every agent on this account is already stopped." : null,
    agentId: null,
    agentIds: ws.agents.map((a) => a.agent_id),
    stepUp: stepUpLine("stop_all", "", 0),
    back: { href: "/", label: "Dashboard" },
  };
}

function closeAllRecord(ws: Workspace): StopRecord {
  const active = ws.agents.filter((a) => a.mode !== "stopped");
  const positions = ws.agents.flatMap((a) => a.positions);
  const stock = sellsStock(positions) || ws.external_positions.some((p) => p.instrument.asset_class === "us_equity");
  return {
    kind: "close_all",
    title: RECORD_TITLE.close_all,
    label: "Account",
    subject: `${ws.connection.broker} account`,
    scope: "The broker's cancel-all and close-position for the whole account. It does not stop at what Owlhead manages: every open order and every position on the account is affected, including ones no agent placed.",
    warning: null,
    lists: [
      {
        key: "orders",
        heading: "Orders it cancels",
        items: [...ws.agents.flatMap((a) => a.orders.map((o) => orderLine(o, a.label))), "Any other open order on the account, including ones Owlhead did not place"],
        empty: "",
      },
      {
        key: "positions",
        heading: "Positions it closes",
        items: [...ws.agents.flatMap((a) => a.positions.map((p) => positionLine(p, a.label))), ...ws.external_positions.map(ownLine)],
        empty: "No positions, so nothing is closed.",
        ...(stock ? { note: SESSION_NOTE } : {}),
      },
      { key: "agents", heading: "Agents it stops", items: active.map((a) => `${a.label} (${a.mandate.name})`), empty: "No agent is running." },
    ],
    afterwards: "Every order is canceled, every position closed, your own holdings included, and every agent stopped. Each step is journaled and shown below as it is recorded.",
    nothingToDo: null,
    agentId: null,
    agentIds: ws.agents.map((a) => a.agent_id),
    stepUp: stepUpLine("close_all", "", 0),
    back: { href: "/", label: "Dashboard" },
  };
}

/** The record for a kind and its target, or null when the workspace has no such agent or connection. */
export function buildRecord(kind: RecordKind, ws: Workspace, targetId: string): StopRecord | null {
  switch (kind) {
    case "kill":
    case "release": {
      const agent = ws.agents.find((a) => a.agent_id === targetId);
      if (!agent) return null;
      return kind === "kill" ? killRecord(ws, agent) : releaseRecord(ws, agent);
    }
    case "stop_all":
    case "close_all":
      if (ws.connection.connection_id !== targetId) return null;
      return kind === "stop_all" ? stopAllRecord(ws) : closeAllRecord(ws);
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

/** Every line the screen shows, in order, as the journal keeps it with the command. */
export function recordLines(record: StopRecord): string[] {
  return [
    record.title,
    record.subject,
    record.scope,
    ...(record.warning ? [record.warning] : []),
    ...record.lists.flatMap((l) => [l.heading, ...(l.items.length > 0 ? l.items : [l.empty]), ...(l.note ? [l.note] : [])]),
    record.afterwards,
  ];
}
