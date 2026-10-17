/**
 * The workspace as the screens read it, built from the workspace API's documented reads (spec §4.3,
 * §4.5, §4.7, §4.9) instead of `src/fixtures/` (E11-9, DEC-970). Until `schemas/workspace-api/read-models/`
 * lands, each read's body carries the fixture type of §4.9's table under one member, which DEC-970
 * pins; the decoders refuse what would mislead a screen: a decimal sent as a JSON number, a value a
 * closed safety enum does not list, an approval without the content hash an answer must bind.
 */
import type { WorkspaceClient } from "./client";
import type { ApiError, Watermark } from "./types";
import type { Workspace } from "@/fixtures/types";

export type Loaded = { ok: true; ws: Workspace; as_of: Watermark[] } | { ok: false; error: ApiError };

export async function loadWorkspace(client: WorkspaceClient): Promise<Loaded> {
  void client;
  throw new Error("Unimplemented: E11-9");
}

/** The workspace the screens show before the first read answers, or when none can: no agent, no request, no figure. */
export function emptyWorkspace(status: Workspace["status"], now: string): Workspace {
  void status;
  void now;
  throw new Error("Unimplemented: E11-9");
}
