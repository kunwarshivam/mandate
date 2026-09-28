import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as approval from "@/app/approvals/[approvalId]/page";
import { AppShell } from "@/components/shell/app-shell";
import { StopControl } from "@/components/shell/stop-control";
import { AGENT_IDS, APPROVAL_IDS } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { type Capability, ROLES, type Role, can } from "./roles";

const CAPABILITIES: Capability[] = ["stop.open", "stop.pause", "stop.full", "approvals.respond", "agents.view", "audit.view", "workspace.view"];

function allowed(role: Role) {
  return CAPABILITIES.filter((c) => can(role, c));
}

function choiceTitles(section: HTMLElement) {
  return within(section)
    .queryAllByRole("button")
    .map((b) => b.querySelector("span")?.textContent ?? "");
}

function sidebarGroups() {
  const nav = screen.getByRole("navigation", { name: "Main" });
  return within(nav)
    .queryAllByRole("link")
    .map((a) => a.getAttribute("href"));
}

beforeEach(() => setPathname("/"));

describe("what each role may do", () => {
  it("gives owners and operators everything", () => {
    expect(allowed("owner")).toEqual(CAPABILITIES);
    expect(allowed("operator")).toEqual(CAPABILITIES);
  });

  it("lets approvers pause and respond, never stop or kill", () => {
    expect(allowed("approver")).toEqual(["stop.open", "stop.pause", "approvals.respond", "agents.view", "audit.view", "workspace.view"]);
  });

  it("gives viewers and auditors no Stop", () => {
    expect(allowed("viewer")).toEqual(["agents.view", "workspace.view"]);
    expect(allowed("auditor")).toEqual(["audit.view"]);
  });

  it("covers every role", () => {
    expect(ROLES.map((r) => r.id)).toEqual(["owner", "operator", "approver", "viewer", "auditor"]);
  });
});

describe("roles in the shell", () => {
  it("offers an approver Pause only, for the agent and the account", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />, "normal", { role: "approver" });
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const sheet = screen.getByRole("dialog");
    const agent = within(sheet).getByRole("heading", { name: /This agent: Agent 3/ }).closest("section")!;
    expect(choiceTitles(agent)).toEqual(["Pause Agent 3"]);
    const account = within(sheet).getByRole("heading", { name: /Everything on this account/ }).closest("section")!;
    expect(choiceTitles(account)).toEqual(["Pause all agents on this account"]);
    expect(sheet.querySelector("[data-slot=kill-switch]")).toBeNull();
  });

  it.each<Role>(["viewer", "auditor"])("shows a %s no Stop control anywhere", (role) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role });
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows an auditor only the Audit group", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role: "auditor" });
    const hrefs = sidebarGroups();
    expect(hrefs.length).toBeGreaterThan(0);
    for (const href of hrefs) expect(href).toMatch(/^\/audit(\/|$)/);
    expect(screen.queryByRole("link", { name: /^Approvals/ })).toBeNull();
  });

  it("shows a viewer the request read-only, with no Approve or Skip", async () => {
    setPathname(`/approvals/${APPROVAL_IDS.swingXyz}`);
    const page = await approval.default({ params: Promise.resolve({ approvalId: APPROVAL_IDS.swingXyz }) });
    renderWithRuntime(<AppShell>{page}</AppShell>, "normal", { role: "viewer" });
    expect(document.querySelector("[data-slot=read-only]")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /^Approve/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Skip/ })).toBeNull();
  });
});
