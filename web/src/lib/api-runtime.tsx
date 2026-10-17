"use client";

import type { ReactNode } from "react";
import type { Fetch } from "@/api/client";
import type { ApiSource } from "./data-source";

/**
 * The runtime the screens use when the data source is a workspace deployment (DEC-970): the same
 * `Runtime` as the fixture stand-in, filled from the workspace API's reads and sending each command
 * through its idempotent write routes. Nothing is shown before the journal answers, nothing is
 * written to browser storage, and an answer that does not confirm a record is never shown as one.
 */
export function ApiRuntimeProvider(props: {
  source: Pick<ApiSource, "origin" | "workspaceId">;
  children: ReactNode;
  fetch?: Fetch;
  refreshMs?: number;
  statusPollMs?: number;
  retryMs?: number;
}): ReactNode {
  void props;
  throw new Error("Unimplemented: E11-9");
}
