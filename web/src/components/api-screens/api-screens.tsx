"use client";

import type { ReactNode } from "react";

/**
 * The screens over the workspace API's read models (DEC-1096): Home, the agents, one agent, the
 * approvals and one request, each showing exactly what the API serves and "Not reported" for the rest.
 */
export function BySource({ fixtures, api }: { fixtures: ReactNode; api: ReactNode }): ReactNode {
  void fixtures;
  void api;
  throw new Error("Unimplemented: E11-9");
}

export function ApiHome(): ReactNode {
  throw new Error("Unimplemented: E11-9");
}

export function ApiAgents(): ReactNode {
  throw new Error("Unimplemented: E11-9");
}

export function ApiAgent({ agentId }: { agentId: string }): ReactNode {
  void agentId;
  throw new Error("Unimplemented: E11-9");
}

export function ApiApprovals(): ReactNode {
  throw new Error("Unimplemented: E11-9");
}

export function ApiApproval({ approvalId }: { approvalId: string }): ReactNode {
  void approvalId;
  throw new Error("Unimplemented: E11-9");
}
