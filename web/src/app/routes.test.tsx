import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as agentSection from "@/app/agents/[agentId]/[...section]/page";
import * as agent from "@/app/agents/[agentId]/page";
import * as agentsNew from "@/app/agents/new/page";
import * as agents from "@/app/agents/page";
import * as alerts from "@/app/alerts/page";
import * as approval from "@/app/approvals/[approvalId]/page";
import * as approvals from "@/app/approvals/page";
import * as auditScreen from "@/app/audit/[screen]/page";
import * as audit from "@/app/audit/page";
import * as connections from "@/app/connections/page";
import * as design from "@/app/design/page";
import * as dashboard from "@/app/page";
import * as positions from "@/app/positions/page";
import * as settingsScreen from "@/app/settings/[screen]/page";
import * as settings from "@/app/settings/page";
import { AppShell } from "@/components/shell/app-shell";
import { StopControl } from "@/components/shell/stop-control";
import { recordHref } from "@/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { canOpen, homeFor, routeNeeds } from "@/lib/access";
import { ROLES, type Role, can } from "@/lib/roles";
import { AGENT_SECTIONS, RECORD_TITLE, SCREENS, SECTION_INDEX, agentHref, decisionHref, orderHref, positionHref, screensIn } from "@/lib/screens";
import { isDisabled, renderWithRuntime } from "@/test/harness";
import { pageFor } from "@/test/app-routes";
import { setPathname } from "@/test/navigation";

const AGENT = AGENT_IDS.swing;
const CONNECTION = buildWorkspace("normal").connection.connection_id;

/** The kill-switch and release record screens (D10, D11). */
const STOP_RECORDS = [recordHref("kill", AGENT_IDS.btc), recordHref("release", AGENT_IDS.btc), recordHref("stop_all", CONNECTION), recordHref("close_all", CONNECTION)];

const WS = buildWorkspace("normal");
const SWING = WS.agents.find((a) => a.agent_id === AGENT)!;
const BTC = WS.agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
const XYZ = SWING.positions.find((p) => p.instrument.symbol === "XYZ")!;

/** One of each record: a stock and a crypto position, the close page, a working and a finished order, and a decision. */
const RECORDS = [
  positionHref(AGENT, XYZ.instrument.asset_id),
  `${positionHref(AGENT, XYZ.instrument.asset_id)}/close`,
  positionHref(AGENT_IDS.btc, BTC.positions[0].instrument.asset_id),
  orderHref(AGENT, SWING.orders[0].client_order_id),
  orderHref(AGENT, SWING.past_orders[0].client_order_id),
  decisionHref(AGENT, WS.decisions.find((d) => d.agent_id === AGENT)!.event_id),
];

const PATHS = [
  ...SCREENS.map((s) => s.href),
  SECTION_INDEX.audit.href,
  SECTION_INDEX.workspace.href,
  ...AGENT_SECTIONS.map((s) => agentHref(AGENT, s.key)),
  `/approvals/${APPROVAL_IDS.swingXyz}`,
  ...RECORDS,
  "/design",
  ...STOP_RECORDS,
];

async function renderPath(path: string) {
  setPathname(path);
  const page = await pageFor(path);
  return renderWithRuntime(<AppShell>{page}</AppShell>);
}

beforeEach(() => setPathname("/"));

describe("route coverage", () => {
  it.each(PATHS)("%s resolves to a page with the paper badge and an enabled Stop", async (path) => {
    await renderPath(path);
    const [header] = screen.getAllByRole("banner");
    expect(within(header).getByText("PAPER")).toBeInTheDocument();
    expect(isDisabled(within(header).getByRole("button", { name: "Stop" }))).toBe(false);
    expect(within(screen.getByRole("main")).getAllByRole("heading", { level: 1 }).length).toBeGreaterThan(0);
  });

  it.each(PATHS)("%s renders no link to a path without a page", async (path) => {
    await renderPath(path);
    const hrefs = new Set(
      Array.from(document.querySelectorAll("a[href]"))
        .map((a) => a.getAttribute("href") ?? "")
        .filter((h) => h.startsWith("/") && !h.startsWith("//"))
        .map((h) => h.split("#")[0].split("?")[0]),
    );
    expect(hrefs).toContain("/settings/profile");
    for (const href of hrefs) await expect(pageFor(href), href).resolves.toBeDefined();
  });

  it("gives every screen that is not built its purpose, never a dead end", async () => {
    for (const s of SCREENS.filter((x) => !x.built)) {
      const { unmount } = await renderPath(s.href);
      const soon = document.querySelector("[data-slot=coming-soon]");
      expect(soon, s.href).not.toBeNull();
      expect(soon).toHaveTextContent("Coming in the next slice");
      expect(soon).toHaveTextContent(s.purpose);
      unmount();
    }
  });

  it("lists every audit and workspace screen as a static param", () => {
    expect(auditScreen.generateStaticParams().map((p) => `/audit/${p.screen}`)).toEqual(screensIn("/audit").map((s) => s.href));
    expect(settingsScreen.generateStaticParams().map((p) => `/settings/${p.screen}`)).toEqual(screensIn("/settings").map((s) => s.href));
  });

  it("sends unknown screens and sections to not found", async () => {
    await expect(pageFor("/audit/nope")).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor("/settings/nope")).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/nope`)).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/overview`)).rejects.toThrow("NEXT_NOT_FOUND");
  });

  it("sends a record whose ID has the wrong shape to not found", async () => {
    await expect(pageFor(`/agents/${AGENT}/positions/XYZ`)).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/positions/${XYZ.instrument.asset_id}/sell`)).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/orders/${SWING.orders[0].client_order_id.slice(4)}`)).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/decisions/cid_01JB5GQ`)).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/approvals/${APPROVAL_IDS.swingXyz}`)).rejects.toThrow("NEXT_NOT_FOUND");
  });
});

describe("titles", () => {
  const generic = /^[A-Z][a-z]+( [a-z]+)*$/;

  it("keeps static titles generic", () => {
    for (const mod of [dashboard, agents, agentsNew, agent, approvals, approval, alerts, connections, audit, settings, design, positions]) {
      const title = mod.metadata.title;
      const text = typeof title === "object" && title && "absolute" in title ? title.absolute.replace(/ · Owlhead$/, "") : String(title);
      expect(text).toMatch(generic);
    }
  });

  it("names agent sections by section, never by agent", async () => {
    for (const s of AGENT_SECTIONS.filter((x) => x.key !== "overview")) {
      const meta = await agentSection.generateMetadata({ params: Promise.resolve({ agentId: AGENT, section: s.key.split("/") }) });
      expect(meta.title).toBe(s.label);
    }
  });

  it("names records by kind, never by agent, instrument or ID", async () => {
    const expected = [RECORD_TITLE.position, RECORD_TITLE["close-position"], RECORD_TITLE.position, RECORD_TITLE.order, RECORD_TITLE.order, RECORD_TITLE.decision];
    for (const [i, path] of RECORDS.entries()) {
      const [, , agentId, ...section] = path.split("/");
      const meta = await agentSection.generateMetadata({ params: Promise.resolve({ agentId, section }) });
      expect(meta.title, path).toBe(expected[i]);
      expect(String(meta.title)).toMatch(generic);
    }
  });
});

/** PX-11, restated here without `routeNeeds`, so the two can disagree and fail. */
function mayOpen(role: Role, path: string): boolean {
  const stop = /\/(kill-switch|release|stop-all|close-all)$/.test(path) || /^\/agents\/[^/]+\/positions\/[^/]+\/close$/.test(path);
  const auditPath = path === "/audit" || path.startsWith("/audit/");
  switch (role) {
    case "owner":
    case "operator":
      return true;
    case "approver":
      return !stop;
    case "viewer":
      return !stop && !auditPath;
    case "auditor":
      return auditPath;
    default: {
      const unhandled: never = role;
      throw new Error(`unhandled role ${String(unhandled)}`);
    }
  }
}

function internalHrefs(root: ParentNode = document): string[] {
  return Array.from(root.querySelectorAll("a[href]"))
    .map((a) => a.getAttribute("href") ?? "")
    .filter((h) => h.startsWith("/") && !h.startsWith("//"))
    .map((h) => h.split("#")[0].split("?")[0]);
}

const ROLE_IDS = ROLES.map((r) => r.id);

describe("role-based route access (PX-11)", () => {
  it("has an access rule for every path with a page", () => {
    for (const path of PATHS) expect(routeNeeds(path), path).not.toBeNull();
  });

  it.each(ROLE_IDS.flatMap((role) => PATHS.map((path) => [role, path] as const)))(
    "as %s, %s renders the page only if the role may open it, and every link on screen goes where the role may go",
    async (role, path) => {
      setPathname(path);
      const page = await pageFor(path);
      renderWithRuntime(<AppShell>{page}</AppShell>, "normal", { role });
      const open = mayOpen(role, path);
      expect(canOpen(role, path)).toBe(open);

      const main = screen.getByRole("main");
      const denied = main.querySelector("[data-slot=access-denied]");
      if (open) {
        expect(denied).toBeNull();
      } else {
        expect(denied).not.toBeNull();
        expect(main.children).toHaveLength(1);
        expect(within(main).getByRole("heading", { level: 1 })).toHaveTextContent("Not available to your role");
        expect(within(denied as HTMLElement).getByRole("link")).toHaveAttribute("href", homeFor(role).href);
      }

      for (const href of internalHrefs()) expect(canOpen(role, href), `${href} shown at ${path}`).toBe(true);
      const [home] = screen.getAllByRole("link", { name: /^Owlhead, / });
      expect(home).toHaveAttribute("href", homeFor(role).href);
    },
  );

  it("tells an auditor the journal and exports are theirs, and sends them there", async () => {
    setPathname("/");
    renderWithRuntime(<AppShell>{await pageFor("/")}</AppShell>, "normal", { role: "auditor" });
    const denied = screen.getByRole("main").querySelector("[data-slot=access-denied]")!;
    expect(denied).toHaveTextContent("you see the journal and its exports, and nothing that acts");
    expect(within(denied as HTMLElement).getByRole("link", { name: "Go to the audit" })).toHaveAttribute("href", "/audit");
    expect(screen.getByRole("link", { name: "Owlhead, audit" })).toHaveAttribute("href", "/audit");
  });

  it.each(ROLE_IDS.filter((role) => can(role, "stop.open")).flatMap((role) => ["/", `/agents/${AGENT_IDS.btc}`].map((path) => [role, path] as const)))(
    "as %s, the Stop sheet at %s links only to record screens the role may open",
    (role, path) => {
      setPathname(path);
      renderWithRuntime(<StopControl />, "normal", { role });
      fireEvent.click(screen.getByRole("button", { name: "Stop" }));
      const links = internalHrefs(screen.getByRole("dialog"));
      if (!can(role, "stop.full")) expect(links).toEqual([]);
      for (const href of links) expect(canOpen(role, href), href).toBe(true);
    },
  );

  it.each(ROLE_IDS)("as %s, the sidebar and command palette list exactly the screens the role may open", (role) => {
    for (const s of SCREENS) expect(can(role, s.needs), s.href).toBe(canOpen(role, s.href));
  });
});
