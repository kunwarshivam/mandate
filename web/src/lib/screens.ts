import type { Capability } from "./roles";

/**
 * Every screen the navigation can reach, with what it is for. The sidebar, the breadcrumbs, the
 * command palette, and the route-coverage test all read this list, so a link cannot point at a
 * screen that does not exist. A screen that is not built yet renders "coming in the next slice"
 * with its purpose, never a dead link.
 */
export type ScreenGroup = "main" | "agents" | "accounts" | "audit" | "workspace";

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
  agents: "Agents",
  accounts: "Accounts",
  audit: "Audit",
  workspace: "Workspace",
};

export const GROUP_NEEDS: Record<ScreenGroup, Capability> = {
  main: "agents.view",
  agents: "agents.view",
  accounts: "workspace.view",
  audit: "audit.view",
  workspace: "workspace.view",
};

export const SCREENS: readonly Screen[] = [
  { key: "home", href: "/", label: "Home", purpose: "Your account, every agent against its limits, and what needs you.", group: "main", built: true, needs: "agents.view" },
  { key: "approvals", href: "/approvals", label: "Approvals", purpose: "Requests waiting for you, by deadline, and what happened to earlier ones.", group: "main", built: true, needs: "agents.view" },
  { key: "alerts", href: "/alerts", label: "Alerts", purpose: "Data, deployment, and agent conditions that change what agents may do.", group: "main", built: true, needs: "agents.view" },
  { key: "agents", href: "/agents", label: "All agents", purpose: "Each agent trading on paper within its own confirmed mandate.", group: "agents", built: true, needs: "agents.view" },
  {
    key: "agents-new",
    href: "/agents/new",
    label: "New agent",
    purpose: "Describe an agent in your own words, review every drafted field with where it came from, and confirm each section before it deploys to paper.",
    group: "agents",
    built: false,
    needs: "agents.view",
  },
  {
    key: "connections",
    href: "/connections",
    label: "Connections",
    purpose: "Broker accounts this workspace trades through, their environment, and when each last answered.",
    group: "accounts",
    built: false,
    needs: "workspace.view",
  },
  { key: "audit-trace", href: "/audit/trace", label: "Trace", purpose: "Follow any order back through the gate decision, the proposal, and the data that caused it.", group: "audit", built: false, needs: "audit.view" },
  { key: "audit-decisions", href: "/audit/decisions", label: "Gate decisions", purpose: "Every allow and deny from the gate, across agents, with each check it ran.", group: "audit", built: true, needs: "audit.view" },
  { key: "audit-timeline", href: "/audit/timeline", label: "Timeline", purpose: "Mode changes, orders, and reconciliation events across agents, newest first.", group: "audit", built: true, needs: "audit.view" },
  { key: "audit-surveillance", href: "/audit/surveillance", label: "Surveillance", purpose: "Patterns across agents that a reviewer should look at: wash risk, concentration, unusual timing.", group: "audit", built: false, needs: "audit.view" },
  { key: "audit-export", href: "/audit/export", label: "Export", purpose: "Download the journal for a period in a format an auditor can check.", group: "audit", built: false, needs: "audit.view" },
  { key: "audit-verify", href: "/audit/verify", label: "Verify chain", purpose: "Check that the journal's hash chain is unbroken from its first entry to now.", group: "audit", built: false, needs: "audit.view" },
  { key: "settings-policies", href: "/settings/policies", label: "Policies", purpose: "Workspace rules every mandate inherits: approvals, instruments, and limits no agent may exceed.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-members", href: "/settings/members", label: "Members", purpose: "Who is in this workspace and what each role may do.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-notifications", href: "/settings/notifications", label: "Notifications", purpose: "Where approvals and alerts reach you, and your quiet hours.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-clients", href: "/settings/clients", label: "Connected clients", purpose: "Devices and tools signed in to this workspace, and when each was last used.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-disclosures", href: "/settings/disclosures", label: "Disclosures", purpose: "The disclosures you have been shown and accepted, with dates.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-billing", href: "/settings/billing", label: "Billing", purpose: "Your plan and invoices.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-deployment", href: "/settings/deployment", label: "Deployment", purpose: "Where your agents run, its version, and when it last answered.", group: "workspace", built: false, needs: "workspace.view" },
  { key: "settings-profile", href: "/settings/profile", label: "Profile", purpose: "Your name, passkeys, and sign-in sessions.", group: "workspace", built: false, needs: "workspace.view" },
];

/** Index pages for the two sections whose children live in the sidebar. */
export const SECTION_INDEX = {
  audit: { href: "/audit", label: "Audit", purpose: "The journal, read back: decisions, timelines, traces, and proof it is intact." },
  workspace: { href: "/settings", label: "Workspace", purpose: "Rules, people, and the plumbing behind this workspace." },
} as const;

export type AgentSectionKey =
  | "overview"
  | "positions"
  | "orders"
  | "decisions"
  | "approvals"
  | "mandate"
  | "mandate/versions"
  | "mandate/edit"
  | "prove"
  | "prove/backtests"
  | "prove/paper"
  | "prove/live"
  | "activity";

export interface AgentSection {
  key: AgentSectionKey;
  label: string;
  purpose: string;
  built: boolean;
  parent?: "mandate" | "prove";
}

export const AGENT_SECTIONS: readonly AgentSection[] = [
  { key: "overview", label: "Overview", purpose: "Mode, equity, limits, and what needs you.", built: true },
  { key: "positions", label: "Positions", purpose: "What this agent holds, marked with the time of the mark.", built: true },
  { key: "orders", label: "Orders", purpose: "Every order this agent placed, in every state.", built: true },
  { key: "decisions", label: "Decisions", purpose: "Each allow and deny from the gate for this agent.", built: true },
  { key: "approvals", label: "Approvals", purpose: "Requests from this agent that wait for you, and earlier ones.", built: true },
  { key: "mandate", label: "Mandate", purpose: "The confirmed limits this agent trades within, with where each came from.", built: true },
  { key: "mandate/versions", label: "Versions", purpose: "Each confirmed version of this mandate and what changed.", built: false, parent: "mandate" },
  { key: "mandate/edit", label: "Edit", purpose: "Propose a change to the mandate; it takes effect only after you confirm each changed field.", built: false, parent: "mandate" },
  { key: "prove", label: "Prove", purpose: "Evidence before money: backtests, a paper run, then going live.", built: true },
  { key: "prove/backtests", label: "Backtests", purpose: "Hypothetical runs of this mandate on past data, labelled as hypothetical.", built: false, parent: "prove" },
  { key: "prove/paper", label: "Paper run", purpose: "How this agent has done on paper against its mandate.", built: false, parent: "prove" },
  { key: "prove/live", label: "Go live", purpose: "The checks and confirmations before an agent may use real funds.", built: false, parent: "prove" },
  { key: "activity", label: "Activity", purpose: "Mode changes, orders, and reconciliation events for this agent.", built: true },
];

export function agentHref(agentId: string, key: AgentSectionKey): string {
  return key === "overview" ? `/agents/${agentId}` : `/agents/${agentId}/${key}`;
}

export function findAgentSection(segments: readonly string[]): AgentSection | undefined {
  const key = segments.join("/");
  return AGENT_SECTIONS.find((s) => s.key === key && s.key !== "overview");
}

export function findScreen(href: string): Screen | undefined {
  return SCREENS.find((s) => s.href === href);
}

export function screensIn(prefix: string): Screen[] {
  return SCREENS.filter((s) => s.href.startsWith(`${prefix}/`));
}

export interface Crumb {
  href: string;
  label: string;
}

/**
 * The trail for a path. Agent labels come from the workspace; a label is the owner's name for the
 * agent, never model output. Record IDs stay out of the trail: a record is named by its kind.
 */
export function crumbsFor(pathname: string, agentLabel: (id: string) => string | undefined): Crumb[] {
  const trail: Crumb[] = [{ href: "/", label: "Home" }];
  if (pathname === "/") return trail;
  const parts = pathname.split("/").filter(Boolean);
  const [head, second, ...rest] = parts;
  switch (head) {
    case "agents": {
      trail.push({ href: "/agents", label: "Agents" });
      if (!second) return trail;
      if (second === "new") return [...trail, { href: "/agents/new", label: "New agent" }];
      trail.push({ href: `/agents/${second}`, label: agentLabel(second) ?? "Agent" });
      if (rest.length === 0) return trail;
      const [first, sub] = rest;
      const top = AGENT_SECTIONS.find((s) => s.key === first);
      if (top) trail.push({ href: agentHref(second, top.key), label: top.label });
      const child = sub ? AGENT_SECTIONS.find((s) => s.key === `${first}/${sub}`) : undefined;
      if (child) trail.push({ href: agentHref(second, child.key), label: child.label });
      else if (sub) trail.push({ href: pathname, label: RECORD_LABEL[first] ?? "Record" });
      return trail;
    }
    case "approvals":
      trail.push({ href: "/approvals", label: "Approvals" });
      if (second) trail.push({ href: pathname, label: "Request" });
      return trail;
    case "audit":
    case "settings": {
      const index = head === "audit" ? SECTION_INDEX.audit : SECTION_INDEX.workspace;
      trail.push({ href: index.href, label: index.label });
      const screen = second ? findScreen(`/${head}/${second}`) : undefined;
      if (screen) trail.push({ href: screen.href, label: screen.label });
      if (screen && rest.length > 0) trail.push({ href: pathname, label: RECORD_LABEL[second] ?? "Record" });
      return trail;
    }
    default: {
      const screen = findScreen(`/${head}`);
      if (screen) trail.push({ href: screen.href, label: screen.label });
      if (screen && second) trail.push({ href: pathname, label: RECORD_LABEL[head] ?? "Record" });
      return trail;
    }
  }
}

const RECORD_LABEL: Record<string, string> = {
  positions: "Position",
  orders: "Order",
  decisions: "Decision",
  approvals: "Request",
};
