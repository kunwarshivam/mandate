/**
 * The owner alerts over the workspace API (`GET /alerts`, §4.7, DEC-2141): each alert the journal
 * records about the workspace's agents and accounts, newest first, with its notice and only the
 * reading member's own deliveries. The API serves a kind, never text: the screen names each kind
 * from this closed table, so an alert says only what its kind is (AGENTS.md rule 6, API-10), and a
 * kind the table does not list is refused rather than shown under a guessed name.
 */
import { array, closedEnum, integer, isRecord, nullable, object, string, timestamp, watermark } from "./decode";
import type { Decoded, Decoder, Timestamp, Watermark } from "./types";

/** The kinds `read-models/alerts` serves, each with its class and the generic words the screen shows (notifications spec §3.2). */
export const ALERT_KINDS = {
  risk_limit: { class: "safety", label: "A risk limit was reached" },
  kill_switch: { class: "safety", label: "Kill switch activated" },
  kill_switch_unapplied: { class: "safety", label: "A kill switch could not be applied" },
  agent_held: { class: "safety", label: "An agent is held" },
  retirement_held: { class: "safety", label: "An agent's retirement is held" },
  account_restriction: { class: "safety", label: "The broker restricted the account" },
  connection_unavailable: { class: "safety", label: "Your connection cannot be reached or used" },
  protection: { class: "safety", label: "A position's protection changed" },
  protection_unattributed: { class: "safety", label: "New positions are held: a protective order has no owner" },
  attribution_deficit: { class: "safety", label: "New positions are held: your agents hold more than the account" },
  exit_stalled: { class: "safety", label: "An exit is stalled" },
  reconciliation: { class: "safety", label: "The account did not reconcile" },
  external_activity: { class: "safety", label: "Activity on the account outside your agents" },
  account_state: { class: "safety", label: "The account's state changed" },
  data_feed_down: { class: "safety", label: "A data feed is down" },
  integrity_incident: { class: "safety", label: "A records integrity incident" },
  version_risk_increasing: { class: "safety", label: "A mandate change increases risk" },
  delegation_added: { class: "safety", label: "A delegation was added" },
  connection_added: { class: "safety", label: "A connection was added" },
  went_live: { class: "safety", label: "An agent went live" },
  daily_brief: { class: "info", label: "A daily brief is ready" },
  delegation_ended: { class: "info", label: "A delegation ended" },
  model_status: { class: "info", label: "A model's status changed" },
  research_status: { class: "info", label: "Research status changed" },
  spend_cap: { class: "info", label: "A spend cap was reached" },
  approval_closed: { class: "info", label: "An approval closed" },
} as const satisfies Record<string, { class: "safety" | "info"; label: string }>;

export type AlertKind = keyof typeof ALERT_KINDS;
export type AlertClass = (typeof ALERT_KINDS)[AlertKind]["class"];

const KINDS = Object.keys(ALERT_KINDS) as AlertKind[];
const CHANNELS = ["email", "web_push", "sms", "telegram", "slack", "phone"] as const;
const STATUSES = ["delivered", "failed", "suppressed_quiet_hours", "deferred_quiet_hours", "abandoned"] as const;
const STREAM_TYPES = ["control", "agent", "account"] as const;

export interface AlertDelivery {
  channel: (typeof CHANNELS)[number];
  status: (typeof STATUSES)[number];
  attempt: number;
  recorded_at: Timestamp;
}

export interface AlertNotice {
  issued_at: Timestamp;
  delivery: AlertDelivery[];
}

export interface AlertStream {
  type: (typeof STREAM_TYPES)[number];
  agent_id: string | null;
  account_ref: string | null;
}

export interface Alert {
  alert_id: string;
  kind: AlertKind;
  class: AlertClass;
  raised_at: Timestamp;
  stream: AlertStream;
  subject: { event_id: string; event_type: string };
  owner_command: string | null;
  notice: AlertNotice | null;
}

export interface Alerts {
  alerts: Alert[];
  truncated: boolean;
  as_of: Watermark[];
}

const fail = (path: string, problem: "wrong_type" | "not_canonical"): Decoded<never> => ({ ok: false, issue: { path, problem, value: null, allowed: null } });

const boolean: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path, "wrong_type"));

/** An attempt number: 1 or more (journal spec §9.15). */
const attempt: Decoder<number> = (value, path) => {
  const read = integer(value, path);
  return read.ok && read.value < 1 ? fail(path, "not_canonical") : read;
};

/** At most `max` items, as the schema bounds the list. */
function bounded<T>(item: Decoder<T>, max: number): Decoder<T[]> {
  return (value, path) => {
    const read = array(item)(value, path);
    return read.ok && read.value.length > max ? fail(path, "not_canonical") : read;
  };
}

const EVENT_TYPE = /^[A-Z][A-Za-z0-9]{0,63}$/;
const eventType: Decoder<string> = (value, path) => {
  const read = string(value, path);
  return read.ok && !EVENT_TYPE.test(read.value) ? fail(path, "not_canonical") : read;
};

const delivery = object<AlertDelivery>({ channel: closedEnum(CHANNELS), status: closedEnum(STATUSES), attempt, recorded_at: timestamp });
const notice = object<AlertNotice>({ issued_at: timestamp, delivery: bounded(delivery, CHANNELS.length) });

/** A stream names its agent or its account and nothing else; any other pairing is refused. */
const stream: Decoder<AlertStream> = (value, path) => {
  const read = object<AlertStream>({ type: closedEnum(STREAM_TYPES), agent_id: nullable(string), account_ref: nullable(string) })(value, path);
  if (!read.ok) return read;
  const { type, agent_id, account_ref } = read.value;
  const named = type === "agent" ? agent_id !== null && account_ref === null : type === "account" ? agent_id === null && account_ref !== null : agent_id === null && account_ref === null;
  return named ? read : fail(path, "not_canonical");
};

/** One alert: its class must be its kind's, and only a kill switch names an owner command, so a mislabelled alert is refused, not shown. */
const alert: Decoder<Alert> = (value, path) => {
  const read = object<Alert>({
    alert_id: string,
    kind: closedEnum(KINDS),
    class: closedEnum(["safety", "info"] as const),
    raised_at: timestamp,
    stream,
    subject: object<Alert["subject"]>({ event_id: string, event_type: eventType }),
    owner_command: nullable(string),
    notice: nullable(notice),
  })(value, path);
  if (!read.ok) return read;
  if (read.value.class !== ALERT_KINDS[read.value.kind].class) return fail(`${path}/class`, "not_canonical");
  return read.value.owner_command !== null && read.value.kind !== "kill_switch" ? fail(`${path}/owner_command`, "not_canonical") : read;
};

/** `GET /alerts`' body (`read-models/alerts`). */
export const alertsRead: Decoder<Alerts> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  return object<Alerts>({ alerts: bounded(alert, 200), truncated: boolean, as_of: array(watermark) })(value, path);
};

/** Where an alert's subject is seen: its agent, the account's positions, or the workspace's records. */
export function subjectHref(alert: Alert): string {
  if (alert.stream.type === "agent" && alert.stream.agent_id !== null) return `/agents/${encodeURIComponent(alert.stream.agent_id)}`;
  if (alert.stream.type === "account") return "/positions";
  return "/audit";
}
