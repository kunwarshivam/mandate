import type { AgentDetail, ApiView } from "@/api/read-models";
import type { ShownEnvironment } from "@/fixtures/types";
import { stepUpLine } from "./commands";
import { type AgentModeLine, RECORD_TITLE, type RecordList, type StopRecord } from "./record";

/**
 * The kill switch's record screens (D10) over the workspace API (DEC-1096, DEC-2408): built from what the API
 * serves, the agents and their modes, never from the fixture-shaped workspace, which is empty there.
 * The API serves no open orders, and its positions are the account's, not one agent's, so those lists
 * say "not reported" and never "none": a list that read empty would claim the switch cancels and
 * sells nothing. The switch itself never waits on a read (rule 13), so a screen with lists not
 * reported can still be confirmed. Release (D11) needs the positions it hands over, so over the API
 * it has no record, and the Stop sheet does not offer it.
 */
const ORDERS_NOT_REPORTED = "Not reported: the deployment does not serve open orders yet. The kill switch cancels them whatever this list shows.";
const POSITIONS_NOT_REPORTED = "Not reported for one agent: the deployment serves the account's holdings, not each agent's. The kill switch sells only this agent's.";
const STOPPED = "Stopped. There is nothing more to stop for this agent.";

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

function accountRecord(kind: "stop_all" | "close_all", view: ApiView, environment: ShownEnvironment): StopRecord {
  const active = view.agents.filter((a) => a.mode !== "stopped");
  const agents: RecordList = { key: "agents", heading: "Agents it stops", items: active.map(labelOf), empty: "No agent is running." };
  const orders = notReported("orders", "Orders it cancels", ORDERS_NOT_REPORTED);
  const closeAll = kind === "close_all";
  return {
    kind,
    environment,
    title: RECORD_TITLE[kind],
    label: "Account",
    subject: "The workspace's account",
    scope: closeAll
      ? "The broker's cancel-all and close-position for the whole account. It does not stop at what Owlhead manages: every open order and every position on the account is affected, including ones no agent placed."
      : "Each agent's own kill switch: cancels only that agent's orders, sells only its positions, and ends it. Your own holdings are untouched.",
    warning: null,
    lists: closeAll ? [orders, agents] : [agents, orders],
    afterwards: closeAll
      ? "Every order is canceled, every position closed, your own holdings included, and every agent stopped. Each step is journaled and shown below as it is recorded."
      : "Every agent on this account is stopped for good, and their approval requests are canceled. Each step is journaled and shown below as it is recorded.",
    nothingToDo: !closeAll && view.agents.length > 0 && active.length === 0 ? "Every agent on this account is already stopped." : null,
    agentId: null,
    modes: modesOf(view.agents),
    stepUp: stepUpLine(kind, "", 0),
    back: { href: "/", label: "Dashboard" },
  };
}

/** The record over the API, or null: no such agent or connection, or a release, which the API cannot show. */
export function buildApiRecord(kind: StopRecord["kind"], view: ApiView, environment: ShownEnvironment, connectionId: string, targetId: string): StopRecord | null {
  switch (kind) {
    case "kill": {
      const agent = view.agents.find((a) => a.agent_id === targetId);
      return agent ? killRecord(view, environment, agent) : null;
    }
    case "stop_all":
    case "close_all":
      return connectionId !== "" && connectionId === targetId ? accountRecord(kind, view, environment) : null;
    case "release":
      return null;
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}
