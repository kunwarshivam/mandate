import type { ReactNode } from "react";
import { screen, within } from "@testing-library/react";
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
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { AGENT_SECTIONS, RECORD_TITLE, SCREENS, SECTION_INDEX, agentHref, decisionHref, orderHref, positionHref, screensIn } from "@/lib/screens";
import { isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

/** What Next would render for a path, resolved the same way the app directory does. */
async function pageFor(path: string): Promise<ReactNode> {
  const parts = path.split("/").filter(Boolean);
  const [head, second, ...rest] = parts;
  const params = <T,>(p: T) => ({ params: Promise.resolve(p) });
  switch (head) {
    case undefined:
      return <dashboard.default />;
    case "agents":
      if (!second) return <agents.default />;
      if (second === "new" && rest.length === 0) return <agentsNew.default />;
      if (rest.length === 0) return agent.default(params({ agentId: second }));
      return agentSection.default(params({ agentId: second, section: rest }));
    case "approvals":
      if (!second) return <approvals.default />;
      return approval.default(params({ approvalId: second }));
    case "alerts":
      return <alerts.default />;
    case "positions":
      if (second) throw new Error(`no page for ${path}`);
      return <positions.default />;
    case "connections":
      return <connections.default />;
    case "audit":
      if (!second) return <audit.default />;
      return auditScreen.default(params({ screen: second }));
    case "settings":
      if (!second) return <settings.default />;
      return settingsScreen.default(params({ screen: second }));
    case "design":
      return <design.default />;
    default:
      throw new Error(`no page for ${path}`);
  }
}

const AGENT = AGENT_IDS.swing;

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
