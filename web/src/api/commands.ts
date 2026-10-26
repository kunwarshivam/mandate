/**
 * The screens' owner commands and approval answers as workspace API calls (spec §5.2, §5.4; DEC-975).
 * Each gesture is prepared once, so a retry sends the same key and body (API-4). Nothing here sizes an
 * order or names a price the owner did not see: the web app only records what the owner chose.
 *
 * `step_up` is `null` except on an approve the owner confirmed on the host (`cli_confirm`, DEC-2104):
 * the API forwards a `null` for the stream owner to judge (§5.4), or answers `step_up_required` and
 * records nothing (§5.2), and the screen says so. `record` is `null` wherever the schema allows it, since the artifact upload the
 * spec's record names is not built; where a member must name what was shown (an answer's `record`, a
 * release's warning) it is the SHA-256 of the lines the screen showed, in their order.
 */
import type { WorkspaceClient } from "./client";
import type { Classification } from "./confirm";
import type { PasskeyStepUp } from "./passkey";
import { type StepUpChallenge, cliConfirmEvidence } from "./step-up";
import type { JsonObject, PreparedCommand } from "./types";
import type { AgentMode, ContentRef, ShownEnvironment } from "@/fixtures/types";
import type { ApprovalRecord, CommandKind, CommandRecord, ConfirmRecord, DeployToPaperRecord } from "@/lib/mock-runtime";
import { sha256Hex } from "@/lib/sha256";

export type Prepared = { ok: true; commands: PreparedCommand[] } | { ok: false; reason: string };

/** What a command needs from the last workspace read: the agents and their modes, the connection, and the environment the screen showed. */
export interface CommandScope {
  agents: ReadonlyArray<{ agent_id: string; mode: AgentMode | null }>;
  /** `""` when no connection has been read. */
  connectionId: string;
  environment: ShownEnvironment;
}

/** Names the web app's build in every record it sends (spec §5, mandate spec §10). */
export const UI_BUILD = "owlhead-web/E11-9";

const ref = (text: string): ContentRef => `sha256:${sha256Hex(text)}`;

/** The spec §5 `record`: the rendered screen as a content ref, and the build that rendered it. */
export function recordRef(record: CommandRecord | ApprovalRecord | ConfirmRecord | DeployToPaperRecord): { artifact: ContentRef; ui_build: ContentRef } {
  return { artifact: ref(JSON.stringify(record)), ui_build: ref(UI_BUILD) };
}

const one = (command: PreparedCommand): Prepared => ({ ok: true, commands: [command] });

export function prepareOwnerCommand(client: WorkspaceClient, ws: CommandScope, kind: CommandKind, agentId: string | null, record?: CommandRecord): Prepared {
  const agentCommand = (operation: "pause" | "resume" | "stop", body: JsonObject): Prepared =>
    agentId ? one(client.prepare(operation, { agent: agentId }, body)) : { ok: false, reason: "no_agent" };
  switch (kind) {
    case "pause":
      return agentCommand("pause", { record: null });
    case "resume":
      return agentCommand("resume", { record: null, step_up: null });
    case "stop":
      return agentCommand("stop", { release: false, warning_shown: null, record: null, step_up: null });
    case "release":
      if (!record) return { ok: false, reason: "no_warning_shown" };
      return agentCommand("stop", { release: true, warning_shown: ref(JSON.stringify(record.shown)), record: null, step_up: null });
    case "kill":
      if (!agentId) return { ok: false, reason: "no_agent" };
      return one(client.prepareKillSwitch({ scope: { kind: "agent", id: agentId }, environment_shown: ws.environment === "unknown" ? null : ws.environment, owner_exit: null, record: null, step_up: null }));
    case "pause_all": {
      const live = ws.agents.filter((a) => a.mode !== "stopped");
      if (live.length === 0) return { ok: false, reason: "no_agents" };
      return { ok: true, commands: live.map((a) => client.prepare("pause", { agent: a.agent_id }, { record: null })) };
    }
    case "stop_all":
    case "close_all": {
      const scope = ws.connectionId ? { kind: "connection" as const, id: ws.connectionId } : { kind: "workspace" as const, id: null };
      return one(client.prepareKillSwitch({ scope, environment_shown: ws.environment === "unknown" ? null : ws.environment, owner_exit: null, record: null, step_up: null }));
    }
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

/** An asset id as the exits route takes it (`common.schema.json#/$defs/AssetId`): a lowercase UUID. */
const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export const isAssetId = (instrument: string): boolean => ASSET_ID.test(instrument);

/**
 * The owner's exit from one instrument (§5.4, `POST /agents/{id}/exits`; rule 13): the asset and the
 * screen the owner confirmed. The bid confirmation is all `null`: the positions read serves no quote
 * with its size and time, so there is no bid the owner could have seen, and an equity sell outside the
 * regular session waits for it, which the screen says. It carries no step-up: the server records an
 * owner exit without one (§3.11 item 12), and it is never refused for step-up.
 */
export function prepareOwnerExit(client: WorkspaceClient, agentId: string, instrument: string, record: CommandRecord): Prepared {
  if (!agentId) return { ok: false, reason: "no_agent" };
  if (!isAssetId(instrument)) return { ok: false, reason: "no_asset_id" };
  return one(client.prepare("owner_exit", { agent: agentId }, { instrument, bid: null, bid_size: null, quoted_at: null, floor: null, record: recordRef(record), step_up: null }));
}

/**
 * An answer to an approval (§5.2). An approve carries the `cli_confirm` evidence of `challenge`, which
 * must be bound to this approval's content hash; a challenge for anything else is refused here and
 * nothing is sent. A Skip never carries step-up (API-7), whatever it is given.
 */
export function prepareApprovalResponse(
  client: WorkspaceClient,
  approval: { approval_id: string; content_hash?: ContentRef },
  response: "approve" | "skip",
  record: ApprovalRecord,
  challenge: StepUpChallenge | null = null,
): Prepared {
  if (!approval.content_hash) return { ok: false, reason: "no_content_hash" };
  const bound = challenge !== null && challenge.action.kind === "approve" && challenge.action.digest === approval.content_hash;
  if (response === "approve" && challenge !== null && !bound) return { ok: false, reason: "step_up_mismatch" };
  const step_up = response === "approve" && challenge !== null ? { ...cliConfirmEvidence(challenge) } : null;
  return one(
    client.prepare(
      "approval_response",
      { approval: approval.approval_id },
      { verdict: response === "approve" ? "approved" : "skipped", content_hash: approval.content_hash, record: recordRef(record), step_up, delegation: null },
    ),
  );
}

/** What `GET /mandate-versions/{hash}/confirmation` serves for the screen to render and the confirm to bind (spec §4.1). */
export interface ServedConfirmation {
  screen_digest: ContentRef;
  classification: Classification;
}

/**
 * What a confirm binds (spec §5.1): the version, the agent and base it is for (`null` for a new agent),
 * the A5 record, and the confirmation screen's values exactly as the server served them.
 */
export interface ConfirmInput {
  version: ContentRef;
  agentId: string | null;
  base: ContentRef | null;
  record: ConfirmRecord;
  served: ServedConfirmation;
  /** The passkey's evidence and assertion (DEC-2128), or `null` when no passkey was asked, which the server answers `step_up_required` for a risk-increasing version. */
  stepUp: PasskeyStepUp | null;
}

/**
 * `POST /mandate-versions/{hash}/confirm`, its body exactly as `confirm-request.schema.json` requires
 * it: every required member and no other. `screen_digest` and `classification_shown` are the served
 * confirmation's, passed through and never computed here: the digest proves the screen rendered the
 * server's data, which only the server's own value can. `warnings_acknowledged` is empty: the summary
 * shows no warning to acknowledge. Under passkey step-up the body carries `step_up` and its
 * `step_up_assertion` (DEC-2128 item 1); without, `step_up` is `null` and no assertion is sent.
 */
export function prepareConfirm(client: WorkspaceClient, input: ConfirmInput): PreparedCommand {
  const { record, served } = input;
  return client.prepare(
    "confirm_version",
    { version: input.version },
    {
      agent_id: input.agentId,
      base_version: input.base,
      confirmed_paths: record.paths,
      warnings_acknowledged: [],
      classification_shown: served.classification,
      screen_digest: served.screen_digest,
      record: recordRef(record),
      ...(input.stepUp ? { step_up: { ...input.stepUp.step_up }, step_up_assertion: { ...input.stepUp.step_up_assertion } } : { step_up: null }),
    },
  );
}

/** What a deployment binds (spec §3.11 item 11): the agent's id, its confirmed version, the B3 record, and the passkey's step-up. */
export interface DeployInput {
  agentId: string;
  version: ContentRef;
  record: DeployToPaperRecord;
  stepUp: PasskeyStepUp;
}

/**
 * `POST /agents` (§4.2, §3.11 item 11): exactly `agent_id`, `mandate_version`, `backtest_id`,
 * `record`, `step_up` and `step_up_assertion`. The backtest is the one B3 showed, or `null`; the
 * server records it and never reads it. Paper only: the server refuses a `live` document.
 */
export function prepareDeploy(client: WorkspaceClient, input: DeployInput): PreparedCommand {
  return client.prepare(
    "deploy",
    {},
    {
      agent_id: input.agentId,
      mandate_version: input.version,
      backtest_id: input.record.backtestId,
      record: recordRef(input.record),
      step_up: { ...input.stepUp.step_up },
      step_up_assertion: { ...input.stepUp.step_up_assertion },
    },
  );
}
