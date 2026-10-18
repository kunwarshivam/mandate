import type { AgentDetail, ApiView } from "@/api/read-models";
import type { ShownEnvironment } from "@/fixtures/types";
import { stepUpLine } from "./commands";
import { type AgentModeLine, RECORD_TITLE, type RecordList, type StopRecord } from "./record";

/**
 * The kill switch's record screens (D10) over the workspace API (DEC-1096, DEC-2408): built from
 * what the API serves, the agents and their modes, never from the fixture-shaped workspace, which
 * is empty there. The API serves no open orders, and its positions are the account's, not one
 * agent's, so those lists say "not reported" and never "none": a list that read empty would claim
 * the switch cancels and sells nothing. The switch itself never waits on a read (rule 13), so a
 * screen with lists not reported can still be confirmed. "Close everything" sends the same account
 * kill switch as "Stop all agents" from here (DEC-1007), so its text says that, not the broker's
 * close-all. Release (D11) needs the positions it hands over, so over the API it has no record, and
 * the Stop sheet does not offer it.
 */
const ORDERS_NOT_REPORTED = "Not reported: the deployment does not serve open orders yet. The kill switch cancels them whatever this list shows.";
const POSITIONS_NOT_REPORTED = "Not reported for one agent: the deployment serves the account's holdings, not each agent's. The kill switch sells only this agent's.";
const STOPPED = "Stopped. There is nothing more to stop for this agent.";
const AGENTS_UNAVAILABLE = "Not available: the agents could not be read. The switch stops every one of them whatever this list shows.";
/** The connection segment of the account's record route when no connection has been read: the switch still works (rule 13). */
export const UNREAD_CONNECTION = "unread";

const labelOf = (a: AgentDetail) => a.label ?? a.agent_id;

function modesOf(agents: AgentDetail[]): AgentModeLine[] {
  return agents.flatMap((a) => (a.mode === null ? [] : [{ agentId: a.agent_id, label: labelOf(a), mode: a.mode }]));
}

const notReported = (key: RecordList["key"], heading: string, empty: string): RecordList => ({ key, heading, items: [], empty });

function killRecord(view: ApiView, environment: ShownEnvironment, agent: AgentDetail): StopRecord {
  const label = labelOf(agent);
  const others = view.agents.filter((a) => a.agent_id !== agent.agent_id);
  return {
    kind: "kill",
    environment,
    title: RECORD_TITLE.kill,
    label,
    subject: label,
    scope: "Cancels only this agent's orders, sells only its positions, leaves other agents and your own holdings untouched, and ends the agent: it becomes stopped.",
    warning: null,
    lists: [
      notReported("orders", "Orders it cancels", ORDERS_NOT_REPORTED),
      notReported("positions", "Positions it sells", POSITIONS_NOT_REPORTED),
      { key: "untouched", heading: "What it leaves alone", items: others.map((a) => `${labelOf(a)}, with its orders and positions`), empty: "No other agent is on this workspace." },
    ],
    afterwards: `${label} is stopped for good, and its approval requests are canceled. Each step is journaled and shown below as it is recorded.`,
    nothingToDo: agent.mode === "stopped" ? STOPPED : null,
    agentId: agent.agent_id,
    modes: modesOf([agent]),
    stepUp: stepUpLine("kill", label, 0),
    back: { href: `/agents/${agent.agent_id}`, label },
  };
}

function accountRecord(kind: "stop_all" | "close_all", view: ApiView | null, environment: ShownEnvironment, connectionRead: boolean): StopRecord {
  const active = view ? view.agents.filter((a) => a.mode !== "stopped") : [];
  const agents: RecordList = { key: "agents", heading: "Agents it stops", items: active.map(labelOf), empty: view ? "No agent is running." : AGENTS_UNAVAILABLE };
  const orders = notReported("orders", "Orders it cancels", ORDERS_NOT_REPORTED);
  const closeAll = kind === "close_all";
  return {
    kind,
    environment,
    title: RECORD_TITLE[kind],
    label: "Account",
    subject: connectionRead ? "The workspace's account" : "Every agent in this workspace",
    scope: !connectionRead
      ? "The account has not been read, so this is the workspace's kill switch: it stops every agent in this workspace, each by its own kill switch. Your own holdings are untouched."
      : closeAll
      ? "From here this sends the account's kill switch: each agent on the account is stopped by its own kill switch. The broker's cancel-all and close-position, for orders and holdings no agent placed, is not sent from the web app yet."
      : "Each agent's own kill switch: cancels only that agent's orders, sells only its positions, and ends it. Your own holdings are untouched.",
    warning: null,
    lists: closeAll ? [orders, agents] : [agents, orders],
    afterwards: closeAll
      ? "Every agent on this account is stopped for good. Orders and holdings no agent placed are left as they are. Each step is journaled and shown below as it is recorded."
      : "Every agent on this account is stopped for good, and their approval requests are canceled. Each step is journaled and shown below as it is recorded.",
    nothingToDo: !closeAll && view && view.agents.length > 0 && active.length === 0 ? "Every agent on this account is already stopped." : null,
    agentId: null,
    modes: view ? modesOf(view.agents) : [],
    stepUp: stepUpLine(kind, "", 0),
    back: { href: "/", label: "Dashboard" },
  };
}

/**
 * The record over the API, or null: no such agent or connection, or a release, which the API cannot
 * show. The account's records build with no read at all, since the kill switch never waits on one;
 * with no connection read, only "Stop all agents" is offered, as the workspace's switch, and it says so.
 */
export function buildApiRecord(kind: StopRecord["kind"], view: ApiView | null, environment: ShownEnvironment, connectionId: string, targetId: string): StopRecord | null {
  switch (kind) {
    case "kill": {
      const agent = view?.agents.find((a) => a.agent_id === targetId);
      return view && agent ? killRecord(view, environment, agent) : null;
    }
    case "stop_all":
    case "close_all":
      if (connectionId !== "" && connectionId === targetId) return accountRecord(kind, view, environment, true);
      return connectionId === "" && targetId === UNREAD_CONNECTION && kind === "stop_all" ? accountRecord(kind, view, environment, false) : null;
    case "release":
      return null;
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}
