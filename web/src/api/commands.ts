/**
 * The screens' owner commands and approval answers as workspace API calls (spec §5.2, §5.4; DEC-970).
 * Each gesture is prepared once, so a retry sends the same key and body (API-4). Nothing here sizes an
 * order or names a price the owner did not see: the web app only records what the owner chose.
 */
import type { WorkspaceClient } from "./client";
import type { PreparedCommand } from "./types";
import type { Approval, ContentRef, Workspace } from "@/fixtures/types";
import type { ApprovalRecord, CommandKind, CommandRecord } from "@/lib/mock-runtime";

export type Prepared = { ok: true; commands: PreparedCommand[] } | { ok: false; reason: string };

export function prepareOwnerCommand(client: WorkspaceClient, ws: Workspace, kind: CommandKind, agentId: string | null, record?: CommandRecord): Prepared {
  void client;
  void ws;
  void kind;
  void agentId;
  void record;
  throw new Error("Unimplemented: E11-9");
}

export function prepareApprovalResponse(client: WorkspaceClient, approval: Approval, response: "approve" | "skip", record: ApprovalRecord): Prepared {
  void client;
  void approval;
  void response;
  void record;
  throw new Error("Unimplemented: E11-9");
}

/** The spec §5 `record`: the rendered screen's lines as a content ref, and the build that rendered them. */
export function recordRef(shown: readonly string[]): { artifact: ContentRef; ui_build: ContentRef } {
  void shown;
  throw new Error("Unimplemented: E11-9");
}
