import type { Capability } from "./roles";

/**
 * Every screen the navigation can reach, with what it is for (DEC-2293: two screens and one
 * settings page). The dock, the phone's More sheet, the breadcrumbs, the command palette, and the
 * route-coverage test all read this list, so a link cannot point at a screen that does not exist.
 */
export type ScreenGroup = "main" | "workspace";

export interface Screen {
  key: string;
  href: string;
  label: string;
  purpose: string;
  group: ScreenGroup;
  built: boolean;
  /** What a role needs to see this entry in the navigation. */
  needs: Capability;
}

export const GROUP_LABEL: Record<ScreenGroup, string | null> = {
  main: null,
  workspace: "Workspace",
};

export const GROUP_NEEDS: Record<ScreenGroup, Capability> = {
  main: "agents.view",
  workspace: "workspace.view",
};

export const SCREENS: readonly Screen[] = [
  { key: "home", href: "/", label: "Home", purpose: "Your agents as conversations: what each said last, and which need you.", group: "main", built: true, needs: "agents.view" },
  { key: "new", href: "/new", label: "New agent", purpose: "Describe an agent in your own words; it asks what it needs, then you confirm it and deploy it to paper.", group: "main", built: true, needs: "agents.view" },
  { key: "settings", href: "/settings", label: "Settings", purpose: "Connections, each agent's chat model and daily AI spending cap, notifications, and your passkeys.", group: "workspace", built: true, needs: "workspace.view" },
];

/** An agent's thread. */
export function agentHref(agentId: string): string {
  return `/agents/${agentId}`;
}

/** The screens a role is offered a door to: the built ones. */
export function listedScreens(): Screen[] {
  return SCREENS.filter((s) => s.built);
}

export function findScreen(href: string): Screen | undefined {
  return SCREENS.find((s) => s.href === href);
}

export interface Crumb {
  href: string;
  label: string;
}

/** The kill-switch and connection record screens, named by kind like every record. */
const STOP_LABEL: Record<string, string> = {
  "kill-switch": "Kill switch",
  release: "Release",
  "stop-all": "Stop all",
  "close-all": "Close everything",
};

/**
 * The trail for a path. Agent labels come from the workspace; a label is the owner's name for the
 * agent, never model output. Record IDs stay out of the trail: a record is named by its kind.
 */
export function crumbsFor(pathname: string, agentLabel: (id: string) => string | undefined): Crumb[] {
  const trail: Crumb[] = [{ href: "/", label: "Home" }];
  if (pathname === "/") return trail;
  const [head, second, third] = pathname.split("/").filter(Boolean);
  switch (head) {
    case "agents":
      if (!second) return trail;
      trail.push({ href: agentHref(second), label: agentLabel(second) ?? "Agent" });
      if (third && STOP_LABEL[third]) trail.push({ href: pathname, label: STOP_LABEL[third] });
      return trail;
    case "approvals":
      if (second) trail.push({ href: pathname, label: "Request" });
      return trail;
    case "connections":
      trail.push({ href: "/settings", label: "Settings" });
      if (second && !third) trail.push({ href: pathname, label: "Connection" });
      if (third && STOP_LABEL[third]) trail.push({ href: pathname, label: STOP_LABEL[third] });
      return trail;
    default: {
      const screen = findScreen(`/${head}`);
      if (screen) trail.push({ href: screen.href, label: screen.label });
      return trail;
    }
  }
}
