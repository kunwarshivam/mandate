/**
 * The screens' owner commands and approval answers as workspace API calls (spec §5.2, §5.4; DEC-970).
 * Each gesture is prepared once, so a retry sends the same key and body (API-4). Nothing here sizes an
 * order or names a price the owner did not see: the web app only records what the owner chose.
 *
 * `step_up` is always `null` until the browser's step-up ceremony exists (E9-4): the API forwards that
 * for the stream owner to judge (§5.4), or answers `step_up_required` and records nothing (§5.2), and
 * the screen says so. `record` is `null` wherever the schema allows it, since the artifact upload the
 * spec's record names is not built; where a member must name what was shown (an answer's `record`, a
 * release's warning) it is the SHA-256 of the lines the screen showed, in their order.
 */
import type { WorkspaceClient } from "./client";
import type { JsonObject, PreparedCommand } from "./types";
import type { Approval, ContentRef, Workspace } from "@/fixtures/types";
import type { ApprovalRecord, CommandKind, CommandRecord } from "@/lib/mock-runtime";
import { sha256Hex } from "@/lib/sha256";

export type Prepared = { ok: true; commands: PreparedCommand[] } | { ok: false; reason: string };

/** Names the web app's build in every record it sends (spec §5, mandate spec §10). */
export const UI_BUILD = "owlhead-web/E11-9";

const ref = (text: string): ContentRef => `sha256:${sha256Hex(text)}`;

/** The spec §5 `record`: the rendered screen as a content ref, and the build that rendered it. */
export function recordRef(record: CommandRecord | ApprovalRecord): { artifact: ContentRef; ui_build: ContentRef } {
  return { artifact: ref(JSON.stringify(record)), ui_build: ref(UI_BUILD) };
}

const one = (command: PreparedCommand): Prepared => ({ ok: true, commands: [command] });

export function prepareOwnerCommand(client: WorkspaceClient, ws: Workspace, kind: CommandKind, agentId: string | null, record?: CommandRecord): Prepared {
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
      return one(client.prepareKillSwitch({ scope: { kind: "agent", id: agentId }, environment_shown: ws.environment, owner_exit: null, record: null, step_up: null }));
    case "pause_all": {
      const live = ws.agents.filter((a) => a.mode !== "stopped");
      if (live.length === 0) return { ok: false, reason: "no_agents" };
      return { ok: true, commands: live.map((a) => client.prepare("pause", { agent: a.agent_id }, { record: null })) };
    }
    case "stop_all":
    case "close_all": {
      const scope = ws.connection.connection_id ? { kind: "connection" as const, id: ws.connection.connection_id } : { kind: "workspace" as const, id: null };
      return one(client.prepareKillSwitch({ scope, environment_shown: ws.environment, owner_exit: null, record: null, step_up: null }));
    }
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

export function prepareApprovalResponse(client: WorkspaceClient, approval: Approval, response: "approve" | "skip", record: ApprovalRecord): Prepared {
  if (!approval.content_hash) return { ok: false, reason: "no_content_hash" };
  return one(
    client.prepare(
      "approval_response",
      { approval: approval.approval_id },
      { verdict: response === "approve" ? "approved" : "skipped", content_hash: approval.content_hash, record: recordRef(record), step_up: null, delegation: null },
    ),
  );
}
