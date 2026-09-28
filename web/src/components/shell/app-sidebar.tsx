"use client";

import type { ComponentType } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Sidebar } from "@cloudflare/kumo/components/sidebar";
import {
  ArrowLeft,
  Bell,
  BellRinging,
  Buildings,
  ChartLine,
  ClockCounterClockwise,
  Export,
  Eye,
  FileText,
  Flask,
  Gavel,
  GitCommit,
  HardDrives,
  House,
  ListBullets,
  Notepad,
  Path,
  PencilSimple,
  Plug,
  Plugs,
  Plus,
  Pulse,
  Receipt,
  Robot,
  Scroll,
  SealCheck,
  SquaresFour,
  Stack,
  Tray,
  Broadcast,
  UserCircle,
  Users,
} from "@phosphor-icons/react";
import type { Agent } from "@/fixtures/types";
import { homeFor } from "@/lib/access";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { can, useRole } from "@/lib/roles";
import { AGENT_SECTIONS, type AgentSection, type AgentSectionKey, GROUP_LABEL, GROUP_NEEDS, SCREENS, type ScreenGroup, agentHref } from "@/lib/screens";
import { SidebarBrand } from "./brand";
import { agentIdFrom } from "./stop-control";

type IconType = ComponentType<{ className?: string }>;

const SCREEN_ICON: Record<string, IconType> = {
  home: House,
  approvals: Tray,
  alerts: Bell,
  agents: Robot,
  positions: Stack,
  "agents-new": Plus,
  connections: Plugs,
  "audit-trace": Path,
  "audit-decisions": Gavel,
  "audit-timeline": ClockCounterClockwise,
  "audit-surveillance": Eye,
  "audit-export": Export,
  "audit-verify": SealCheck,
  "settings-policies": Scroll,
  "settings-members": Users,
  "settings-notifications": BellRinging,
  "settings-clients": Plug,
  "settings-disclosures": FileText,
  "settings-billing": Receipt,
  "settings-deployment": HardDrives,
  "settings-profile": UserCircle,
};

const SECTION_ICON: Record<AgentSectionKey, IconType> = {
  overview: SquaresFour,
  positions: Stack,
  orders: ListBullets,
  decisions: Gavel,
  approvals: Tray,
  mandate: Scroll,
  "mandate/versions": GitCommit,
  "mandate/edit": PencilSimple,
  prove: Flask,
  "prove/backtests": ChartLine,
  "prove/paper": Notepad,
  "prove/live": Broadcast,
  activity: Pulse,
};

const GROUPS: ScreenGroup[] = ["main", "agents", "accounts", "audit", "workspace"];

function Icon({ icon: I }: { icon: IconType }) {
  return <I className="size-4 shrink-0" aria-hidden />;
}

function isActive(pathname: string, href: string): boolean {
  if (href === "/") return pathname === "/";
  if (href === "/agents") return pathname === "/agents";
  return pathname === href || pathname.startsWith(`${href}/`);
}

/** The approvals count carries a number and nothing else. */
export function ApprovalsCount({ n, className }: { n: number; className?: string }) {
  if (n === 0) return null;
  return (
    <span
      data-slot="approvals-count"
      className={`inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-lapis px-1.5 font-mono text-xs font-semibold text-lapis-foreground tabular ${className ?? ""}`}
    >
      {n}
      <span className="sr-only"> open</span>
    </span>
  );
}

function AccountView({ pathname, openApprovals }: { pathname: string; openApprovals: number }) {
  const { role } = useRole();
  return (
    <>
      {GROUPS.filter((g) => can(role, GROUP_NEEDS[g])).map((group) => {
        const screens = SCREENS.filter((s) => s.group === group && can(role, s.needs));
        if (screens.length === 0) return null;
        const label = GROUP_LABEL[group];
        return (
          <Sidebar.Group key={group}>
            {label ? <Sidebar.GroupLabel>{label}</Sidebar.GroupLabel> : null}
            <Sidebar.Menu>
              {screens.map((s) => {
                const active = isActive(pathname, s.href);
                return (
                  <Sidebar.MenuButton
                    key={s.key}
                    href={s.href}
                    icon={<Icon icon={SCREEN_ICON[s.key] ?? House} />}
                    active={active}
                    aria-current={active ? "page" : undefined}
                    tooltip={s.label}
                  >
                    {s.label}
                    {s.key === "approvals" ? <ApprovalsCount n={openApprovals} className="ml-auto" /> : null}
                  </Sidebar.MenuButton>
                );
              })}
            </Sidebar.Menu>
          </Sidebar.Group>
        );
      })}
    </>
  );
}

function SectionButton({ agent, section, pathname }: { agent: Agent; section: AgentSection; pathname: string }) {
  const href = agentHref(agent.agent_id, section.key);
  const active = pathname === href;
  return (
    <Sidebar.MenuButton
      href={href}
      icon={<Icon icon={SECTION_ICON[section.key]} />}
      active={active}
      aria-current={active ? "page" : undefined}
      tooltip={section.label}
    >
      {section.label}
    </Sidebar.MenuButton>
  );
}

function AgentView({ agent, pathname }: { agent: Agent; pathname: string }) {
  const top = AGENT_SECTIONS.filter((s) => !s.parent);
  return (
    <>
      <Sidebar.Group>
        <Sidebar.Menu>
          <Sidebar.MenuButton href="/agents" icon={<Icon icon={ArrowLeft} />} tooltip="All agents">
            All agents
          </Sidebar.MenuButton>
        </Sidebar.Menu>
      </Sidebar.Group>
      <Sidebar.Group>
        <Sidebar.GroupLabel>{agent.label}</Sidebar.GroupLabel>
        <Sidebar.Menu>
          {top.map((section) => {
            const children = AGENT_SECTIONS.filter((s) => s.parent === section.key);
            if (children.length === 0) return <SectionButton key={section.key} agent={agent} section={section} pathname={pathname} />;
            const base = agentHref(agent.agent_id, section.key);
            const within = pathname === base || pathname.startsWith(`${base}/`);
            return (
              <Sidebar.Collapsible key={section.key} defaultOpen={within}>
                <Sidebar.CollapsibleTrigger
                  render={
                    <Sidebar.MenuButton icon={<Icon icon={SECTION_ICON[section.key]} />} tooltip={section.label}>
                      {section.label}
                      <Sidebar.MenuChevron />
                    </Sidebar.MenuButton>
                  }
                />
                <Sidebar.CollapsibleContent>
                  <Sidebar.MenuSub>
                    {[section, ...children].map((child) => {
                      const href = agentHref(agent.agent_id, child.key);
                      const active = pathname === href;
                      return (
                        <Sidebar.MenuSubItem key={child.key}>
                          <Sidebar.MenuSubButton href={href} active={active} aria-current={active ? "page" : undefined}>
                            {child === section ? `${section.label} summary` : child.label}
                          </Sidebar.MenuSubButton>
                        </Sidebar.MenuSubItem>
                      );
                    })}
                  </Sidebar.MenuSub>
                </Sidebar.CollapsibleContent>
              </Sidebar.Collapsible>
            );
          })}
        </Sidebar.Menu>
      </Sidebar.Group>
    </>
  );
}

/**
 * Kumo's Sidebar, quiet: the page's slate tone with no rule, so the content column carries the
 * weight. The header carries the navy Owlhead brand and the account. Inside an agent, the sidebar slides to that agent's sections; loading agent data never
 * holds back the header or the Stop control.
 */
export function AppSidebar() {
  const pathname = usePathname();
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const agentId = agentIdFrom(pathname);
  const agent = agentId ? ws.agents.find((a) => a.agent_id === agentId) : undefined;
  const openApprovals = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  const agentScoped = agentId !== null && can(role, "agents.view");
  const home = homeFor(role);

  return (
    <Sidebar aria-label="Main" className="border-r-0 bg-background">
      <Sidebar.Header data-slot="brand" className="h-auto flex-col items-stretch gap-0 bg-background px-0">
        <div className="flex h-16 shrink-0 items-center px-4 group-data-[state=collapsed]/sidebar:justify-center group-data-[state=collapsed]/sidebar:px-0">
          <Link
            href={home.href}
            className="inline-flex min-h-11 items-center outline-none focus-visible:ring-3 focus-visible:ring-ring"
            aria-label={`Owlhead, ${home.label}`}
          >
            <SidebarBrand />
          </Link>
        </div>
        <div data-slot="account" className="flex min-w-0 items-center gap-2.5 px-4 pb-4 group-data-[state=collapsed]/sidebar:justify-center group-data-[state=collapsed]/sidebar:px-0">
          <Buildings className="size-5 shrink-0 text-muted-foreground" aria-hidden />
          <div className="grid min-w-0 group-data-[state=collapsed]/sidebar:hidden">
            <span className="field-label text-muted-foreground">Account</span>
            <span className="truncate text-sm font-medium">{ws.connection.broker}</span>
          </div>
        </div>
      </Sidebar.Header>
      <Sidebar.Content>
        <Sidebar.SlidingViews activeKey={agentScoped ? "agent" : "account"}>
          <Sidebar.SlidingView value="account">
            <AccountView pathname={pathname} openApprovals={openApprovals} />
          </Sidebar.SlidingView>
          <Sidebar.SlidingView value="agent">
            {agentScoped ? (
              agent ? (
                <AgentView agent={agent} pathname={pathname} />
              ) : ws.status === "loading" ? (
                <Sidebar.Loading label="Loading agent sections" />
              ) : null
            ) : null}
          </Sidebar.SlidingView>
        </Sidebar.SlidingViews>
      </Sidebar.Content>
      <Sidebar.Footer>
        <Sidebar.Trigger />
      </Sidebar.Footer>
    </Sidebar>
  );
}
