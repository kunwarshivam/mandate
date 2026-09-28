import { type Capability, type Role, can } from "./roles";
import { findScreen } from "./screens";

const STOP_RECORDS = new Set(["kill-switch", "release", "stop-all", "close-all"]);

/**
 * What a role needs to open a path (PX-11). The shell renders an access-denied state in place of a
 * page the role may not open, so hiding a link is never the only guard. Auditors hold only
 * `audit.view`: they see the journal and exports, and nothing that acts. Paths with no page return
 * null and fall through to not-found.
 */
export function routeNeeds(pathname: string): Capability | null {
  const [head, second, third] = pathname.split("/").filter(Boolean);
  if (head === undefined) return "agents.view";
  if ((head === "agents" || head === "connections") && second && third && STOP_RECORDS.has(third)) return "stop.full";
  switch (head) {
    case "agents":
    case "approvals":
      return "agents.view";
    case "audit":
      return "audit.view";
    case "settings":
    case "design":
      return "workspace.view";
    default:
      return findScreen(`/${head}`)?.needs ?? null;
  }
}

export function canOpen(role: Role, pathname: string): boolean {
  const needs = routeNeeds(pathname);
  return needs === null || can(role, needs);
}

/** Where a role starts: the dashboard, or for an auditor the journal. */
export function homeFor(role: Role): { href: string; label: string } {
  if (can(role, "agents.view")) return { href: "/", label: "dashboard" };
  if (can(role, "audit.view")) return { href: "/audit", label: "audit" };
  return { href: "/settings", label: "workspace" };
}
