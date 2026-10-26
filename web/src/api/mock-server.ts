/**
 * A fixture-backed stand-in for the workspace API: a `fetch` implementation for tests, for `npm run dev`
 * and for the e2e build (`src/app/v1/`). It answers from `src/fixtures/` in the shapes of the workspace
 * API spec, and its reads in DEC-975's provisional bodies (`reads.ts`), keeps
 * commands in memory by idempotency key and route (API-4), refuses a command without
 * `X-Mandate-Request: 1` as 403 `forbidden` before anything else (spec §3.3), plays `unreachable` as a network that
 * does not answer, and `result-unknown` as commands recorded whose answer is lost, so a later status
 * poll finds them (spec §5.5). A status is `recorded` when first read and `applied` after; a recorded
 * pause pauses its agent and a recorded Skip skips its request, so a screen can follow a command to its
 * effect. It refuses an approve without step-up as `step_up_required`, as spec §5.2 does, and judges
 * one with `cli_confirm` evidence in DEC-2104 item 6's order against the challenges it issued at
 * `POST /step-up/challenges`, standing in for the host's confirmation (`hostConfirms`). It creates a
 * mandate version from the document (DEC-2101 item 3), serves that version's confirmation screen as a
 * new mandate's (item 4), with a new `screen_digest` at each read. A confirm that does not carry the
 * digest it last served is `classification_changed`; one that does, without step-up, is
 * `step_up_required`; a version it never created is `not_found`. A confirm and a deployment take
 * passkey step-up (DEC-2128): the mock serves its member's passkeys at `GET /passkeys` and judges a
 * `passkey` evidence and its assertion against the challenge it issued for the action, the
 * assertion's client data naming that challenge as a `webauthn.get`. It checks no signature, which
 * only the server's verifier can; a test's authenticator stands in for the member's. A deployment
 * needs a version the mock recorded as confirmed and an agent id it has not deployed. A backtest
 * is the fixture's served `backtest` member under the mock's own envelope (DEC-2400).
 */
import { CSRF_HEADER, OPERATIONS, SESSION_ROUTE, STEP_UP_CHALLENGE_ROUTE, isIdempotencyKey, type Fetch } from "./client";
import { CONNECT_START_ROUTE } from "./connect";
import { deployStepUpDigest } from "./passkey";
import { CONNECT_DISCLOSURE, connectStepUpDigest } from "./step-up";
import type { Watermark } from "./types";
import { servedBacktestFixture } from "@/fixtures/backtests";
import { buildWorkspace } from "@/fixtures/workspace";
import type { Agent, Approval, ApprovalStatus, Health, Mandate, Scenario } from "@/fixtures/types";
import { dec, mul, toDecimalString } from "@/lib/decimal";
import { mandateVersion } from "@/lib/fixture-journey";
import { sha256Hex } from "@/lib/sha256";

export interface MockServerOptions {
  workspaceId: string;
  scenario?: Scenario;
  /**
   * Whether the host has confirmed each challenge the mock issues (DEC-2104): the mock stands in for
   * `mandate step-up confirm`, which no browser can run. `false` plays an owner who has not run it yet,
   * so an approve with that challenge's evidence is `step_up_missing`.
   */
  hostConfirms?: boolean;
  /**
   * `DisclosureAccepted` records on the workspace's control stream, in order, which its audit reads
   * serve with their bodies (journal spec §9.2, DEC-2134). Without it the control stream is not served.
   */
  acceptances?: ReadonlyArray<{ document: string; version: string; user: string; method: string | null }>;
  /** The member's passkeys `GET /passkeys` serves; one past its cool-off by default. */
  passkeys?: ReadonlyArray<{ passkey_id: string; cool_off_ends: string | null }>;
}

/** The mock deployment's RP ID, as the server derives it from the app's origin. */
export const MOCK_RP_ID = "app.owlhead.ai";
/** The mock member's user handle: 16 bytes as unpadded base64url. */
export const MOCK_USER_HANDLE = "AAECAwQFBgcICQoLDA0ODw";

/** The mock member's one passkey id: 16 bytes as unpadded base64url. */
export const MOCK_PASSKEY_ID = "bW9jay1wYXNza2V5LWlkLTE";

export interface RecordedCommand {
  event_id: string;
  path: string;
  idempotency_key: string;
  body: string;
}

export interface MockServer {
  fetch: Fetch;
  /** Commands the mock has recorded, in order. */
  recorded(): RecordedCommand[];
  /** Every `screen_digest` the confirmation read has served, in order. */
  servedDigests(): string[];
  /** Every step-up challenge the mock has issued, in order. */
  challenges(): IssuedChallenge[];
  /** The agent ids a deployment recorded, in order. */
  deployed(): string[];
  /** The connects the mock has started, by key, in order. */
  started(): Array<{ idempotency_key: string; connection_id: string }>;
  /** Every agent a real-money request named, in order; the mock refuses each `live_unavailable` (DEC-2134). */
  goLives(): string[];
}

/** Where the mock's start sends the browser: no real authorization server is ever named. */
export const MOCK_AUTHORIZATION_ORIGIN = "https://paper-oauth.mock.invalid";

export interface IssuedChallenge {
  challenge_id: string;
  kind: string;
  digest: string;
  issued_at: string;
  expires_at: string;
  used: boolean;
}

/** The mock's one principal, as `GET /v1/me/session` serves it. */
export const MOCK_PRINCIPAL = "01JBR2C4D6E8F0G2H4J6K8M0P3";
/** A challenge's life (DEC-2103 item 5). */
const CHALLENGE_SECONDS = 300;

const API_VERSION = "v1";
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** A fixture time (an ISO string with an offset) as a journal spec §4.7 timestamp. */
function canonical(iso: string): string {
  return `${new Date(iso).toISOString().slice(0, 23)}000000Z`;
}

/** A ULID-shaped id from the first 128 bits of a SHA-256, as spec §3.4 derives event ids. */
function eventId(seed: string): string {
  let bits = BigInt(`0x${sha256Hex(seed).slice(0, 32)}`);
  let out = "";
  for (let i = 0; i < 26; i += 1) {
    out = CROCKFORD[Number(bits & 31n)] + out;
    bits >>= 5n;
  }
  return out;
}

function respond(status: number, body: unknown, type = "application/json"): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": type } });
}

function problem(status: number, code: string, effect: "none" | "recorded", retryable = false): Response {
  return respond(status, { type: "about:blank", status, title: "Request refused", code, effect, event_id: null, retryable }, "application/problem+json");
}

/** `read-models/health.schema.json`: each component's state and freshness, aged against `servedAt`. An `ok` component is never stale; every fixture feed carries its time. */
function healthBody(health: Health, servedAt: string) {
  const component = (name: keyof Health) => {
    const { state, as_of } = health[name];
    const observed = canonical(as_of ?? servedAt);
    const age = Math.max(0, Math.floor((Date.parse(servedAt) - Date.parse(observed)) / 1000));
    return { state, freshness: { observed_at: observed, stale: state !== "ok", age_seconds: age, limit_source: `health.${name}` } };
  };
  return { market_data: component("market_data"), broker: component("broker"), deployment: component("deployment"), relay: component("relay") };
}

const ROUTES = Object.entries(OPERATIONS).map(([operation, route]) => ({
  operation,
  pattern: new RegExp(`^${route.replace(/\{[a-z]+\}/g, "[^/]+")}$`),
}));

export function createMockServer(options: MockServerOptions): MockServer {
  const { workspaceId, scenario = "normal", hostConfirms = true } = options;
  /** The member's passkeys, which an enrolment here adds to. */
  const passkeys: Array<{ passkey_id: string; cool_off_ends: string | null }> = [...(options.passkeys ?? [{ passkey_id: MOCK_PASSKEY_ID, cool_off_ends: null }])];
  /** Registration challenges held, by id (§3.6): each is dropped by the submit that names it. */
  const registrations = new Map<string, { bytes: string; step_up_needed: boolean }>();
  /** Versions a passkey confirm recorded, and agents a deployment recorded, by id. */
  const confirmedVersions = new Set<string>();
  const deployedAgents = new Map<string, string>();
  const issued: IssuedChallenge[] = [];
  /** Connects started, each read `connecting` once and `established` after, as a completed paper connect would be. */
  const connects: Array<{ idempotency_key: string; body: string; connection_id: string; reads: number }> = [];
  /** Every agent a real-money request named, each refused (DEC-2134). */
  const goLives: string[] = [];
  const workspace = structuredClone(buildWorkspace(scenario));
  const prefix = `/v1/workspaces/${workspaceId}`;
  const commands: RecordedCommand[] = [];
  /** Versions created here, by their hash: a new mandate's, so the confirmation has no agent or base. */
  const versions = new Map<string, Mandate>();
  /** The digest last served for each version: each read serves a new one, as the server's moves with the journal and the date (DEC-2101 item 4). */
  const screens = new Map<string, string>();
  const digests: string[] = [];

  function asOf(): Watermark[] {
    const seq = commands.length;
    return [{ stream_id: `ctl:${workspaceId}`, seq, hash: `sha256:${sha256Hex(`${workspaceId}:control:${seq}`)}`, recorded_at: canonical(workspace.now) }];
  }

  const servedAt = canonical(workspace.now);
  const read = (body: object) => respond(200, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: asOf(), ...body });

  const approvalHash = (approval: Approval) => `sha256:${sha256Hex(`approval:${approval.approval_id}:${approval.requested_at}:${approval.bound.mandate_version}`)}`;
  const agentOf = (id: string) => workspace.agents.find((a) => a.agent_id === id);
  const statusReads = new Map<string, number>();
  /** A fixture decimal in the journal's canonical form (journal spec §4.6), as the API serves it. */
  const canon = (value: string) => toDecimalString(dec(value));

  /** `read-models/agents`' row (#1499): the deployed agent with its mandate document's name and envelope. */
  function agentRow(agent: Agent) {
    const m = agent.mandate;
    return {
      agent_id: agent.agent_id,
      environment: m.environment,
      mandate_version: agent.mandate_version,
      deployed_at: canonical(agent.deployed_at),
      mode: agent.mode,
      pending_approvals: workspace.approvals.filter((a) => a.agent_id === agent.agent_id && a.status === "delivered").length,
      label: m.name,
      mandate: {
        allocation_usd: canon(m.capital.allocation_usd),
        max_loss_from_allocation: canon(m.capital.max_loss_from_allocation),
        goal: m.goal.type,
        goal_end_date: m.goal.end_date,
        asset_classes: m.universe.asset_classes,
        max_position_usd: canon(m.risk.max_position_usd),
        max_order_usd: canon(m.risk.max_order_usd),
        max_orders_per_day: m.risk.max_orders_per_day,
        max_daily_loss: canon(m.risk.max_daily_loss),
        max_drawdown: canon(m.risk.max_drawdown),
      },
    };
  }

  /** `read-models/agent`: the row with the account's holdings at its last reconciliation (`account_positions`), every agent's, as one account's are. */
  function agentDetail(agent: Agent) {
    const positions = workspace.agents.flatMap((a) => a.positions).map((p) => ({ instrument: p.instrument.symbol, qty: canon(p.qty), broker_qty: canon(p.broker_qty), mismatch: dec(p.qty) !== dec(p.broker_qty), observed_at: canonical(p.mark_as_of) }));
    return { ...agentRow(agent), account_positions: positions };
  }

  /** The API names an approval by its `ApprovalRequested` event id, a bare ULID; the fixtures prefix theirs with `apr_`. */
  const servedId = (approval: Approval) => approval.approval_id.replace(/^apr_/, "");
  const approvalBy = (id: string) => workspace.approvals.find((a) => servedId(a) === id);

  const STATE: Record<ApprovalStatus, string> = { delivered: "pending", acted: "acted", gate_skipped: "canceled", rejected: "skipped", expired: "timed_out", superseded: "canceled" };

  /** `read-models/approvals`' row: the approval and the order it binds (#1499). */
  function approvalRow(approval: Approval) {
    const b = approval.bound;
    return {
      approval_id: servedId(approval),
      agent_id: approval.agent_id,
      state: STATE[approval.status],
      requested_at: canonical(approval.requested_at),
      deadline: Math.floor(Date.parse(approval.deadline) / 1000),
      content_hash: approvalHash(approval),
      order: { instrument: b.symbol, asset_class: b.asset_class, side: b.side, qty: canon(b.qty), limit: canon(b.limit), order_usd: toDecimalString(mul(dec(b.qty), dec(b.limit))), purpose: b.purpose, tif: null, protection: null },
    };
  }

  /** The control stream, when the options name acceptances: its opening, then each `DisclosureAccepted` with its body, base64. */
  function controlStream(streamId: string) {
    if (streamId !== `ctl:${workspaceId}` || !options.acceptances) return null;
    const body = (payload: unknown) => btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify({ event_type: "DisclosureAccepted", payload }))));
    const accepted = options.acceptances.map(({ document, version, user, method }) => ({
      type: "DisclosureAccepted",
      body: body({ document, version, user, step_up: method === null ? null : { assertion_id: "cli-1", authenticated_at: workspace.now, method } }),
    }));
    return [{ type: "StreamOpened", body: "e30=" }, ...accepted].map((e, i) => ({ seq: i + 1, event_id: eventId(`${streamId}\n${i}`), event_type: e.type, recorded_at: servedAt, prev_hash: "0".repeat(64), hash: "0".repeat(64), body: e.body }));
  }

  /**
   * An agent stream's events as the mock tells them (`agent:{ws}:{agent}`): the deployment, then each
   * order submitted and each filled, in time order. Types and times only; the body is a stand-in.
   */
  function agentStream(streamId: string) {
    if (streamId === `ctl:${workspaceId}`) return controlStream(streamId);
    if (streamId === `acct:${workspaceId}:${workspace.connection.connection_id}`) return accountStream(streamId);
    const match = streamId.match(/^agent:([^:]+):(.+)$/);
    const agent = match && match[1] === workspaceId ? agentOf(match[2]) : undefined;
    if (!agent) return null;
    const orders = [...agent.past_orders, ...agent.orders].sort((a, b) => Date.parse(a.submitted_at) - Date.parse(b.submitted_at));
    const told = [{ type: "AgentDeployed", at: orders[0]?.submitted_at ?? workspace.now }, ...orders.flatMap((o) => [{ type: "OrderSubmitted", at: o.submitted_at }, ...(o.state === "Filled" ? [{ type: "FillApplied", at: "closed_at" in o ? String(o.closed_at) : o.submitted_at }] : [])])];
    return told.map((e, i) => ({ seq: i + 1, event_id: eventId(`${streamId}\n${i}`), event_type: e.type, recorded_at: canonical(e.at), prev_hash: "0".repeat(64), hash: "0".repeat(64), body: "e30=" }));
  }

  /** The connection's account stream: every agent's orders requested and filled, in time order. Types and times only. */
  function accountStream(streamId: string) {
    const orders = workspace.agents.flatMap((a) => [...a.past_orders, ...a.orders]).sort((a, b) => Date.parse(a.submitted_at) - Date.parse(b.submitted_at));
    const told = orders.flatMap((o) => [{ type: "OrderRequestRecorded", at: o.submitted_at }, ...(o.state === "Filled" ? [{ type: "FillApplied", at: "closed_at" in o ? String(o.closed_at) : o.submitted_at }] : [])]);
    return told.map((e, i) => ({ seq: i + 1, event_id: eventId(`${streamId}\n${i}`), event_type: e.type, recorded_at: canonical(e.at), prev_hash: "0".repeat(64), hash: "0".repeat(64), body: "e30=" }));
  }

  function get(route: string, query: URLSearchParams): Response {
    const journal = route.match(/^\/journal\/streams\/([^/]+)\/events$/);
    const allowed = (name: string) => (route === "/approvals" && name === "state") || (route === "/alerts" && name === "limit") || (journal !== null && (name === "after_seq" || name === "limit"));
    if ([...query.keys()].some((name) => !allowed(name))) return problem(404, "not_found", "none");
    if (journal) {
      const lines = agentStream(decodeURIComponent(journal[1])) ?? controlStream(decodeURIComponent(journal[1]));
      if (!lines) return problem(404, "not_found", "none");
      const after = Number(query.get("after_seq") ?? "0");
      const limit = Number(query.get("limit") ?? "100");
      if (!Number.isSafeInteger(after) || after < 0 || !Number.isSafeInteger(limit) || limit < 1 || limit > 1000) return problem(422, "invalid", "none");
      const events = lines.filter((e) => e.seq > after).slice(0, limit);
      const head = { seq: lines.length, hash: "0".repeat(64), recorded_at: servedAt };
      return read({ stream_id: decodeURIComponent(journal[1]), head, events, next_after_seq: events.at(-1)?.seq ?? after, at_head: (events.at(-1)?.seq ?? after) >= lines.length });
    }
    const oneEvent = route.match(/^\/journal\/events\/([^/]+)$/);
    if (oneEvent) {
      const event = (controlStream(`ctl:${workspaceId}`) ?? []).find((e) => e.event_id === decodeURIComponent(oneEvent[1]));
      return event ? read({ event: { ...event, stream_id: `ctl:${workspaceId}`, artifact_refs: [], config_refs: {} } }) : problem(404, "not_found", "none");
    }
    if (route === "/health") return read(healthBody(workspace.health, servedAt));
    /** `read-models/alerts` (DEC-2141): the fixtures journal no owner alert, so none is invented. */
    if (route === "/alerts") return read({ alerts: [], truncated: false });
    if (route === "/passkeys") return read({ passkeys: passkeys.map((p) => ({ passkey_id: p.passkey_id, alg: "es256", enrolled_at: servedAt, cool_off_ends: p.cool_off_ends })) });
    if (route === "/agents") return read({ agents: workspace.agents.map(agentRow) });
    if (route === "/connections") return read({ connections: connectionRows() });
    const oneConnection = route.match(/^\/connections\/([^/]+)$/);
    if (oneConnection) {
      const row = connectionRows().find((c) => c.connection_id === decodeURIComponent(oneConnection[1]));
      return row ? read({ connection: row }) : problem(404, "not_found", "none");
    }
    if (route === "/approvals") {
      const state = query.get("state");
      if (state !== "pending" && state !== "resolved") return problem(422, "invalid", "none");
      const listed = workspace.approvals.filter((a) => (a.status === "delivered") === (state === "pending"));
      return read({ approvals: listed.map(approvalRow) });
    }
    const activity = route.match(/^\/agents\/([^/]+)\/(orders|positions)$/);
    if (activity) {
      const agent = agentOf(decodeURIComponent(activity[1]));
      if (!agent) return problem(404, "not_found", "none");
      const head = { agent_id: agent.agent_id, account_ref: workspace.connection.connection_id };
      if (activity[2] === "positions") {
        const positions = agent.positions.map((p) => ({ instrument_id: p.instrument.asset_id, filled_qty: canon(p.qty), cost_basis: null, risk_mark: null, market_value: null, unrealized_pnl: null, protection: { orders: [], covered_qty: "0", stop: null, take_profit: null, unprotected_since: null } }));
        return read({ ...head, positions, account_positions: agentDetail(agent).account_positions });
      }
      const served = [...agent.past_orders.map((o) => ({ ...o, updated: o.closed_at })), ...agent.orders.map((o) => ({ ...o, updated: o.submitted_at }))];
      const orders = served.map((o) => ({
        client_order_id: o.client_order_id,
        intent_id: null,
        purpose: o.purpose,
        instrument_id: o.instrument.symbol,
        side: o.side,
        type: o.limit_price === null ? "market" : "limit",
        tif: o.time_in_force,
        qty: canon(o.qty),
        limit_price: o.limit_price === null ? null : canon(o.limit_price),
        stop_price: o.stop_price === null ? null : canon(o.stop_price),
        state: o.state.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase(),
        filled_qty: canon(o.filled_qty),
        submitted_at: canonical(o.submitted_at),
        updated_at: canonical(o.updated),
      }));
      return read({ ...head, orders });
    }
    const agentRoute = route.match(/^\/agents\/([^/]+)$/);
    if (agentRoute) {
      const agent = agentOf(decodeURIComponent(agentRoute[1]));
      return agent ? read({ agent: agentDetail(agent) }) : problem(404, "not_found", "none");
    }
    const approvalRoute = route.match(/^\/approvals\/([^/]+)$/);
    if (approvalRoute) {
      const approval = approvalBy(decodeURIComponent(approvalRoute[1]));
      return approval ? read({ approval: { ...approvalRow(approval), content: { action: { side: approval.bound.side } }, schema_version: 2 } }) : problem(404, "not_found", "none");
    }
    const backtestRoute = route.match(/^\/backtests\/([^/]+)$/);
    if (backtestRoute) {
      const body = servedBacktestFixture(decodeURIComponent(backtestRoute[1])) as { backtest: unknown } | undefined;
      return body ? read({ backtest: body.backtest }) : problem(404, "not_found", "none");
    }
    const screen = route.match(/^\/mandate-versions\/([^/]+)\/confirmation$/);
    if (screen) {
      const version = decodeURIComponent(screen[1]);
      if (!versions.has(version)) return problem(404, "not_found", "none");
      const shown = { mandate_version: version, agent_id: null, base_version: null, classification: "risk_increasing", required_paths: ["/capital/allocation_usd", "/environment"], warnings: [], step_up_required: true };
      const digest = `sha256:${sha256Hex(`${JSON.stringify(shown)}\n${digests.length}`)}`;
      screens.set(version, digest);
      digests.push(digest);
      return read({ ...shown, document: versions.get(version), screen_digest: digest });
    }
    const status = route.match(/^\/commands\/([0-9A-Z]{26})$/);
    const command = status ? commands.find((c) => c.event_id === status[1]) : undefined;
    if (command) {
      const reads = (statusReads.get(command.event_id) ?? 0) + 1;
      statusReads.set(command.event_id, reads);
      if (reads === 1) return read({ phase: "recorded", steps: [] });
      const step = { stream_id: "control", seq: commands.indexOf(command) + 1, event_type: "OwnerCommandApplied", recorded_at: servedAt, reason: null };
      return read({ phase: "applied", steps: [step] });
    }
    return problem(404, "not_found", "none");
  }

  /** What the deployment would show once it acted: a pause pauses its agent, a Skip skips its request. */
  function apply(operation: string, path: string, body: Record<string, unknown>) {
    const pause = path.match(/\/agents\/([^/]+)\/pause$/);
    const agent = pause ? agentOf(decodeURIComponent(pause[1])) : undefined;
    if (operation === "pause" && agent && agent.mode !== "stopped") {
      if (!agent.restrictions.some((r) => r.code === "owner_pause")) agent.restrictions.push({ code: "owner_pause", since: workspace.now });
      if (agent.mode === "normal" || agent.mode === "exits_only") agent.mode = "paused";
    }
    const answer = path.match(/\/approvals\/([^/]+)\/responses$/);
    const approval = answer ? approvalBy(decodeURIComponent(answer[1])) : undefined;
    if (operation === "approval_response" && approval && approval.status === "delivered" && body.verdict === "skipped") {
      approval.status = "rejected";
      approval.resolution = { at: workspace.now, text: "Skipped by you. Nothing was sent." };
    }
  }

  /**
   * DEC-2104 item 6's order for `cli_confirm` evidence, with the host's confirmation played by
   * `hostConfirms`; `null` when it verifies, and then the challenge is spent.
   */
  function judgeCliConfirm(evidence: unknown, kind: string, digest: unknown): string | null {
    if (evidence == null) return "step_up_required";
    if (typeof evidence !== "object" || Array.isArray(evidence)) return "step_up_missing";
    const { assertion_id, authenticated_at, method } = evidence as Record<string, unknown>;
    if (method !== "cli_confirm") return "step_up_missing";
    const challenge = issued.find((c) => c.challenge_id === assertion_id);
    if (!challenge) return "step_up_missing";
    if (challenge.kind !== kind || challenge.digest !== digest) return "step_up_mismatch";
    if (authenticated_at !== challenge.issued_at) return "step_up_stale";
    if (challenge.used) return "step_up_reused";
    if (!hostConfirms) return "step_up_missing";
    challenge.used = true;
    return null;
  }

  /**
   * `passkey` evidence and its assertion for `kind` and `digest` (DEC-2108 items 7 and 8, DEC-2128
   * item 1), in the server's order as far as a mock can follow it; `null` when it verifies, and then
   * the challenge is spent. The signature is not checked.
   */
  function judgePasskey(evidence: unknown, assertion: unknown, kind: string, digest: unknown): { status: number; code: string } | null {
    if (evidence == null) return assertion == null ? { status: 401, code: "step_up_required" } : { status: 422, code: "invalid" };
    if (typeof evidence !== "object" || Array.isArray(evidence)) return { status: 401, code: "step_up_missing" };
    const { assertion_id, authenticated_at, method } = evidence as Record<string, unknown>;
    if (method !== "passkey") return assertion == null ? { status: 401, code: "step_up_method" } : { status: 422, code: "invalid" };
    if (typeof assertion !== "object" || assertion === null) return { status: 401, code: "step_up_missing" };
    const { passkey_id, client_data_json, authenticator_data, signature } = assertion as Record<string, unknown>;
    if (typeof authenticator_data !== "string" || typeof signature !== "string" || typeof client_data_json !== "string") return { status: 422, code: "invalid" };
    if (!passkeys.some((p) => p.passkey_id === passkey_id && (p.cool_off_ends === null || p.cool_off_ends <= servedAt))) return { status: 401, code: "step_up_missing" };
    const challenge = issued.find((c) => c.challenge_id === assertion_id);
    if (!challenge) return { status: 401, code: "step_up_missing" };
    if (challenge.kind !== kind || challenge.digest !== digest) return { status: 401, code: "step_up_mismatch" };
    if (authenticated_at !== challenge.issued_at) return { status: 401, code: "step_up_stale" };
    if (challenge.used) return { status: 401, code: "step_up_reused" };
    let client: { type?: unknown; challenge?: unknown } = {};
    try {
      client = JSON.parse(Buffer.from(client_data_json, "base64url").toString("utf8")) as typeof client;
    } catch {
      return { status: 401, code: "step_up_missing" };
    }
    const bytes = [...challengeKeys.values()].find((e) => e.challenge === challenge)?.bytes;
    if (client.type !== "webauthn.get" || client.challenge !== bytes) return { status: 401, code: "step_up_missing" };
    challenge.used = true;
    return null;
  }

  /** `POST /step-up/challenges` (§3.6): a challenge bound to the action, never journaled; the same key answers the same challenge. */
  const challengeKeys = new Map<string, { body: string; challenge: IssuedChallenge; bytes: string }>();
  function challenge(headers: Headers, body: string): Response {
    const key = headers.get("Idempotency-Key") ?? "";
    if (!isIdempotencyKey(key)) return problem(422, "invalid", "none");
    const earlier = challengeKeys.get(key);
    if (earlier && earlier.body !== body) return problem(409, "idempotency_conflict", "none");
    let action: { kind?: unknown; digest?: unknown } = {};
    try {
      action = ((JSON.parse(body) as { action?: typeof action }).action ?? {}) as typeof action;
    } catch {
      return problem(422, "invalid", "none");
    }
    if (typeof action.kind !== "string" || typeof action.digest !== "string" || !/^sha256:[0-9a-f]{64}$/.test(action.digest)) return problem(422, "invalid", "none");
    if (workspace.environment === "live") return problem(403, "live_unavailable", "none");
    const entry = earlier ?? {
      body,
      bytes: Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url"),
      challenge: {
        challenge_id: eventId(`${workspaceId}\nstep_up_challenge\n${key}`),
        kind: action.kind,
        digest: action.digest,
        issued_at: servedAt,
        expires_at: canonical(new Date(Date.parse(workspace.now) + CHALLENGE_SECONDS * 1000).toISOString()),
        used: false,
      },
    };
    if (!earlier) {
      challengeKeys.set(key, entry);
      issued.push(entry.challenge);
    }
    const c = entry.challenge;
    return respond(201, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: [], challenge_id: c.challenge_id, webauthn_challenge: entry.bytes, action: { kind: c.kind, digest: c.digest }, issued_at: c.issued_at, expires_at: c.expires_at });
  }

  /** `read-models/connections`' rows: the fixture's established connection, then each connect started here. */
  function connectionRows() {
    const row = (connection_id: string, state: string) => ({ connection_id, broker: "alpaca", environment: workspace.environment === "live" ? "live" : "paper", state, scopes: state === "established" ? ["account:write", "trading"] : null, since: servedAt, refusal: null, revoked_reason: null, agents: [] as string[] });
    return [row(workspace.connection.connection_id, "established"), ...connects.map((c) => row(c.connection_id, c.reads++ === 0 ? "connecting" : "established"))];
  }

  /**
   * `POST /connections/oauth/start` (DEC-2623 item 6's order after the session and body): live is
   * refused, a disclosure other than the current one is `invalid`, then the step-up for the connect
   * digest; a retry with the same key and body answers the same start (item 8).
   */
  function startConnect(headers: Headers, body: string): Response {
    const key = headers.get("Idempotency-Key") ?? "";
    if (!isIdempotencyKey(key)) return problem(422, "invalid", "none");
    const earlier = connects.find((c) => c.idempotency_key === key);
    if (earlier && earlier.body !== body) return problem(409, "idempotency_conflict", "none");
    let members: Record<string, unknown> = {};
    try {
      members = JSON.parse(body) as Record<string, unknown>;
    } catch {
      return problem(422, "invalid", "none");
    }
    if (members.broker !== "alpaca" || (members.environment !== "paper" && members.environment !== "live")) return problem(422, "invalid", "none");
    if (workspace.environment === "live" || members.environment === "live") return problem(403, "live_unavailable", "none");
    const disclosure = members.disclosure as { document?: unknown; version?: unknown } | undefined;
    if (disclosure?.document !== CONNECT_DISCLOSURE.document || disclosure.version !== CONNECT_DISCLOSURE.version) return problem(422, "invalid", "none");
    if (!earlier) {
      const refused = judgeCliConfirm(members.step_up, "connection", connectStepUpDigest(workspaceId));
      if (refused) return problem(401, refused, "none");
    }
    const entry = earlier ?? { idempotency_key: key, body, connection_id: `conn_${eventId(`${workspaceId}\nconnect\n${key}`)}`, reads: 0 };
    if (!earlier) connects.push(entry);
    const state = sha256Hex(`state\n${key}`);
    const expires = canonical(new Date(Date.parse(workspace.now) + 600_000).toISOString());
    return respond(201, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: asOf(), authorization_url: `${MOCK_AUTHORIZATION_ORIGIN}/oauth/authorize?state=${state}`, expires_at: expires, connection_id: entry.connection_id });
  }

  /** `POST /passkeys/registration-challenges` (§3.6): a challenge held for one submit, bound to this member. */
  function register(headers: Headers): Response {
    if (!isIdempotencyKey(headers.get("Idempotency-Key") ?? "")) return problem(422, "invalid", "none");
    if (workspace.environment === "live") return problem(403, "live_unavailable", "none");
    const id = eventId(`${workspaceId}\nregistration\n${headers.get("Idempotency-Key")}`);
    const bytes = Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url");
    const held = { bytes, step_up_needed: passkeys.length > 0 };
    registrations.set(id, held);
    const expires = canonical(new Date(Date.parse(workspace.now) + CHALLENGE_SECONDS * 1000).toISOString());
    return respond(201, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: [], challenge_id: id, webauthn_challenge: bytes, rp_id: MOCK_RP_ID, user_handle: MOCK_USER_HANDLE, exclude: passkeys.map((p) => p.passkey_id), step_up_needed: held.step_up_needed, issued_at: servedAt, expires_at: expires });
  }

  /**
   * `POST /passkeys` (§3.6): the registration held and dropped, then the step-up a member who holds a
   * passkey gives (`credential_enrol`), then the client data naming the registration as a
   * `webauthn.create`. The attestation is not verified, which only the server's `enrol` does. A
   * first passkey cools off for 24 hours.
   */
  function enrol(headers: Headers, body: string): Response {
    if (!isIdempotencyKey(headers.get("Idempotency-Key") ?? "")) return problem(422, "invalid", "none");
    let m: Record<string, unknown> = {};
    try {
      m = JSON.parse(body) as Record<string, unknown>;
    } catch {
      return problem(422, "invalid", "none");
    }
    const id = typeof m.challenge_id === "string" ? m.challenge_id : "";
    const held = registrations.get(id);
    registrations.delete(id);
    if (!held) return problem(422, "invalid", "none");
    if (held.step_up_needed) {
      const refused = judgePasskey(m.step_up, m.step_up_assertion, "credential_enrol", `sha256:${sha256Hex(JSON.stringify({ op: "enrol", registration: id }))}`);
      if (refused) return problem(refused.status, refused.code, "none");
    } else if (m.step_up !== null || m.step_up_assertion !== null) return problem(422, "invalid", "none");
    let client: { type?: unknown; challenge?: unknown } = {};
    try {
      client = JSON.parse(Buffer.from(String(m.client_data_json), "base64url").toString("utf8")) as typeof client;
    } catch {
      return problem(422, "invalid", "none");
    }
    const passkey = typeof m.passkey_id === "string" ? m.passkey_id : "";
    if (client.type !== "webauthn.create" || client.challenge !== held.bytes || typeof m.attestation_object !== "string" || passkey.length < 22 || passkeys.some((p) => p.passkey_id === passkey)) return problem(422, "invalid", "none");
    const cool = passkeys.length === 0 ? canonical(new Date(Date.parse(workspace.now) + 24 * 3600 * 1000).toISOString()) : null;
    passkeys.push({ passkey_id: passkey, cool_off_ends: cool });
    return respond(201, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: asOf(), event_id: eventId(`${workspaceId}\nenrol\n${id}`), passkey_id: passkey, alg: "es256", cool_off_ends: cool });
  }

  function post(route: string, headers: Headers, body: string): Response {
    if (headers.get(CSRF_HEADER) !== "1") return problem(403, "forbidden", "none");
    if (route === "/passkeys/registration-challenges") return register(headers);
    if (route === "/passkeys") return enrol(headers, body);
    if (route === STEP_UP_CHALLENGE_ROUTE) return challenge(headers, body);
    if (route === CONNECT_START_ROUTE) return startConnect(headers, body);
    if (/^\/agents\/[^/]+\/go-live$/.test(route)) {
      goLives.push(decodeURIComponent(route.split("/")[2] ?? ""));
      return problem(409, "live_unavailable", "none");
    }
    const match = ROUTES.find((r) => r.pattern.test(route));
    if (!match) return problem(404, "not_found", "none");
    const key = headers.get("Idempotency-Key") ?? "";
    if (!isIdempotencyKey(key)) return problem(422, "invalid", "none");
    let parsed: unknown;
    try {
      parsed = JSON.parse(body);
    } catch {
      return problem(422, "invalid", "none");
    }
    const members = typeof parsed === "object" && parsed !== null && !Array.isArray(parsed) ? (parsed as Record<string, unknown>) : {};
    if (match.operation === "approval_response" && members.verdict === "approved") {
      const refused = judgeCliConfirm(members.step_up, "approve", members.content_hash);
      if (refused) return problem(401, refused, "none");
    }
    const repeated = commands.find((c) => c.idempotency_key === key && c.path === prefix + route);
    if (match.operation === "confirm_version" && !repeated) {
      const version = decodeURIComponent(route.split("/")[2] ?? "");
      if (!versions.has(version)) return problem(404, "not_found", "none");
      if (members.screen_digest !== screens.get(version)) return problem(409, "classification_changed", "none");
      const refused = judgePasskey(members.step_up, members.step_up_assertion, "confirm_version", `sha256:${sha256Hex(JSON.stringify({ agent_id: null, base_version: null, mandate_version: version }))}`);
      if (refused) return problem(refused.status, refused.code, "none");
      confirmedVersions.add(version);
    }
    if (match.operation === "deploy" && !repeated) {
      const agent = typeof members.agent_id === "string" ? members.agent_id : "";
      const version = typeof members.mandate_version === "string" ? members.mandate_version : "";
      if (!/^[A-Za-z0-9_-]{1,64}$/.test(agent)) return problem(422, "invalid", "none");
      if (!versions.has(version)) return problem(404, "not_found", "none");
      if (!confirmedVersions.has(version)) return problem(422, "invalid", "none");
      if (deployedAgents.has(agent) || agentOf(agent)) return problem(422, "invalid", "none");
      const refused = judgePasskey(members.step_up, members.step_up_assertion, "deploy", deployStepUpDigest(agent, version as `sha256:${string}`));
      if (refused) return problem(refused.status, refused.code, "none");
      deployedAgents.set(agent, version);
    }
    if (match.operation === "create_version" && (typeof members.environment !== "string" || members.environment === "live")) return problem(422, "invalid", "none");
    const path = prefix + route;
    let command = commands.find((c) => c.idempotency_key === key && c.path === path);
    if (command && command.body !== body) return problem(409, "idempotency_conflict", "none");
    if (!command) {
      command = { event_id: eventId(`${workspaceId}\n${match.operation}\n${key}`), path, idempotency_key: key, body };
      commands.push(command);
      apply(match.operation, path, members);
    }
    if (match.operation === "approval_response") return respond(202, { response_id: command.event_id, phase: "recorded" });
    if (match.operation === "confirm_version")
      return respond(202, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: asOf(), command_id: command.event_id, phase: "recorded", classification: "risk_increasing", applies: "now" });
    if (match.operation === "create_version") {
      const version = mandateVersion(members as unknown as Mandate);
      versions.set(version, members as unknown as Mandate);
      return respond(202, { command_id: command.event_id, phase: "recorded", mandate_version: version, warnings: [] });
    }
    return respond(202, { command_id: command.event_id, phase: "recorded", step_up_status: "missing" });
  }

  const fetch: Fetch = async (input, init) => {
    const method = (init.method ?? "GET").toUpperCase();
    if (scenario === "unreachable") throw new TypeError("Failed to fetch");
    const url = new URL(input, "http://mock.invalid");
    if (method === "GET" && url.pathname === SESSION_ROUTE) {
      const expires = canonical(new Date(Date.parse(workspace.now) + 15 * 60 * 1000).toISOString());
      return respond(200, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: [], principal_id: MOCK_PRINCIPAL, kind: "full", expires_at: expires });
    }
    if (!url.pathname.startsWith(`${prefix}/`)) return problem(404, "not_found", "none");
    const route = url.pathname.slice(prefix.length);
    if (method === "GET") return get(route, url.searchParams);
    if (method === "POST") {
      const answer = post(route, new Headers(init.headers), typeof init.body === "string" ? init.body : "");
      if (scenario === "result-unknown") throw new TypeError("The deployment recorded the request and the answer was lost");
      return answer;
    }
    return problem(404, "not_found", "none");
  };

  return { fetch, deployed: () => [...deployedAgents.keys()], recorded: () => commands.map((c) => ({ ...c })), servedDigests: () => [...digests], challenges: () => issued.map((c) => ({ ...c })), goLives: () => [...goLives], started: () => connects.map(({ idempotency_key, connection_id }) => ({ idempotency_key, connection_id })) };
}
