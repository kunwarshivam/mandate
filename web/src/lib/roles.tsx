"use client";

import { type ReactNode, createContext, useContext, useMemo, useState } from "react";

/**
 * Workspace roles (PX-11), fixture only: the role lives in React state and is never stored. The
 * server will decide what a member may do; this only shapes what the screens offer.
 */
export type Role = "owner" | "operator" | "approver" | "viewer" | "auditor";

export const ROLES: ReadonlyArray<{ id: Role; label: string }> = [
  { id: "owner", label: "Owner" },
  { id: "operator", label: "Operator" },
  { id: "approver", label: "Approver" },
  { id: "viewer", label: "Viewer" },
  { id: "auditor", label: "Auditor" },
];

export type Capability =
  /** Open the Stop sheet at all. */
  | "stop.open"
  /** Pause an agent or the account. */
  | "stop.pause"
  /** Stop, kill, release and close everything. */
  | "stop.full"
  | "approvals.respond"
  /** Confirm a new mandate and deploy it, or confirm a new version of one: only the owner sets the envelope (rule 11). */
  | "agents.deploy"
  | "agents.view"
  | "audit.view"
  | "workspace.view";

export function can(role: Role, capability: Capability): boolean {
  switch (role) {
    case "owner":
      return true;
    case "operator":
      return capability !== "agents.deploy";
    case "approver":
      return capability !== "stop.full" && capability !== "agents.deploy";
    case "viewer":
      return capability === "agents.view" || capability === "workspace.view";
    case "auditor":
      return capability === "audit.view";
    default: {
      const unhandled: never = role;
      throw new Error(`unhandled role ${String(unhandled)}`);
    }
  }
}

interface RoleState {
  role: Role;
  setRole: (role: Role) => void;
}

const RoleContext = createContext<RoleState>({ role: "owner", setRole: () => {} });

export function RoleProvider({ initial = "owner", children }: { initial?: Role; children: ReactNode }) {
  const [role, setRole] = useState<Role>(initial);
  const value = useMemo(() => ({ role, setRole }), [role]);
  return <RoleContext.Provider value={value}>{children}</RoleContext.Provider>;
}

export function useRole(): RoleState {
  return useContext(RoleContext);
}

export function useCan(capability: Capability): boolean {
  return can(useRole().role, capability);
}
