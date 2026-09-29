import type { ReactNode } from "react";
import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as agentSection from "@/app/(app)/agents/[agentId]/[...section]/page";
import * as agentPage from "@/app/(app)/agents/[agentId]/page";
import * as approvalPage from "@/app/(app)/approvals/[approvalId]/page";
import * as auditScreen from "@/app/(app)/audit/[screen]/page";
import * as dashboard from "@/app/(app)/page";
import * as positions from "@/app/(app)/positions/page";
import { AppShell } from "@/components/shell/app-shell";
import type { Scenario } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import type { Role } from "@/lib/roles";
import { agentHref, crumbsFor, decisionHref, orderHref, positionHref } from "@/lib/screens";
import { liveCharts } from "@/test/chart-mock";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

const WS = buildWorkspace("normal");
const SWING = WS.agents.find((a) => a.agent_id === AGENT_IDS.swing)!;
const BTC = WS.agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
const XYZ = SWING.positions.find((p) => p.instrument.symbol === "XYZ")!;
const XYZ_HREF = positionHref(SWING.agent_id, XYZ.instrument.asset_id);
const UNKNOWN_ORDER = "cid_01JBZB0K9FMMXQ5CSAFT42YMAV";

async function pageFor(path: string): Promise<ReactNode> {
  const [head, second, ...rest] = path.split("/").filter(Boolean);
  const params = <T,>(p: T) => ({ params: Promise.resolve(p) });
  if (head === undefined) return <dashboard.default />;
  if (head === "positions") return <positions.default />;
  if (head === "approvals" && second) return approvalPage.default(params({ approvalId: second }));
  if (head === "audit" && second) return auditScreen.default(params({ screen: second }));
  if (head === "agents" && second && rest.length === 0) return agentPage.default(params({ agentId: second }));
  if (head === "agents" && second) return agentSection.default(params({ agentId: second, section: rest }));
  throw new Error(`no page for ${path}`);
}

async function open(path: string, scenario: Scenario = "normal", role: Role = "owner") {
  setPathname(path);
  const page = await pageFor(path);
  return renderWithRuntime(<AppShell>{page}</AppShell>, scenario, { role });
}

const main = () => screen.getByRole("main");
const linkTo = (href: string) => main().querySelector<HTMLAnchorElement>(`a[href="${href}"]`);
const steps = () => [...main().querySelectorAll("[data-slot=lifecycle] [data-step]")].map((li) => li.getAttribute("data-step"));
const check = (key: string) => main().querySelector(`[data-slot=gate-checks] [data-check="${key}"]`);

beforeEach(() => setPathname("/"));

describe("drill-down", () => {
  it("dashboard, position, close, and back again, with a breadcrumb trail at every step", async () => {
    const home = await open("/");
    expect(linkTo(XYZ_HREF), "dashboard links the XYZ position").not.toBeNull();
    home.unmount();

    const position = await open(XYZ_HREF);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("XYZ position");
    expect(crumbsFor(XYZ_HREF, () => SWING.label)).toEqual([
      { href: "/", label: "Home" },
      { href: "/agents", label: "Agents" },
      { href: agentHref(SWING.agent_id, "overview"), label: SWING.label },
      { href: agentHref(SWING.agent_id, "positions"), label: "Positions" },
      { href: XYZ_HREF, label: "Position" },
    ]);
    const trail = within(screen.getAllByRole("banner")[0].querySelector<HTMLElement>("nav[aria-label=breadcrumb]")!);
    for (const href of [agentHref(SWING.agent_id, "overview"), agentHref(SWING.agent_id, "positions")]) {
      expect(trail.getAllByRole("link").some((a) => a.getAttribute("href") === href), href).toBe(true);
    }
    expect(trail.getAllByText("Position").length).toBeGreaterThan(0);
    expect(liveCharts().some((c) => c.series[0]?.type === "Candlestick")).toBe(true);
    const close = screen.getByRole("link", { name: "Close position…" });
    expect(close).toHaveAttribute("href", `${XYZ_HREF}/close`);
    position.unmount();

    await open(`${XYZ_HREF}/close`);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Close XYZ position");
    expect(screen.getByRole("link", { name: /Back to the position/ })).toHaveAttribute("href", XYZ_HREF);
    expect(crumbsFor(`${XYZ_HREF}/close`, () => SWING.label).at(-1)).toEqual({ href: `${XYZ_HREF}/close`, label: "Close position" });
  });

  it("dashboard agent bands open the agent, whose overview draws its mandate", async () => {
    const home = await open("/");
    const bands = main().querySelectorAll("[data-slot=agent-band]");
    expect(bands).toHaveLength(WS.agents.length);
    for (const a of WS.agents) expect(linkTo(agentHref(a.agent_id, "overview")), a.label).not.toBeNull();
    for (const band of bands) expect(band.querySelector("[data-slot=sparkline]")).not.toBeNull();
    home.unmount();

    await open(agentHref(SWING.agent_id, "overview"));
    expect(main().querySelector("[data-slot=agent-equity] [data-slot=chart-canvas]")).not.toBeNull();
    for (const p of SWING.positions) expect(linkTo(positionHref(SWING.agent_id, p.instrument.asset_id)), p.instrument.symbol).not.toBeNull();
  });

  it("the positions list links every agent's holdings and names the owner's own", async () => {
    await open("/positions");
    for (const a of WS.agents) for (const p of a.positions) expect(linkTo(positionHref(a.agent_id, p.instrument.asset_id)), `${a.label} ${p.instrument.symbol}`).not.toBeNull();
    expect(main()).toHaveTextContent("Your own holdings");
    expect(main()).toHaveTextContent("ABC");
  });

  it("a position's protective legs and fills link to their orders", async () => {
    await open(XYZ_HREF);
    const legs = [...SWING.orders].filter((o) => o.instrument.asset_id === XYZ.instrument.asset_id && o.purpose === "protective");
    expect(legs.length).toBeGreaterThan(0);
    for (const o of legs) expect(linkTo(orderHref(SWING.agent_id, o.client_order_id)), o.client_order_id).not.toBeNull();
    const fills = main().querySelector("[data-slot=fills]")!;
    for (const f of SWING.fills.filter((x) => x.instrument.asset_id === XYZ.instrument.asset_id)) {
      expect(fills.querySelector(`a[href="${orderHref(SWING.agent_id, f.client_order_id)}"]`), f.client_order_id).not.toBeNull();
    }
    expect(main().querySelector("[data-slot=protection]")).toHaveTextContent(/\$133\.00/);
  });
});

describe("orders", () => {
  it("a filled order walks from intent to filled", async () => {
    const filled = SWING.past_orders.find((o) => o.state === "Filled")!;
    await open(orderHref(SWING.agent_id, filled.client_order_id));
    const trail = steps();
    expect(trail.slice(0, 3)).toEqual(["Intent", "Submitting", "Accepted"]);
    expect(trail.at(-1)).toBe("Filled");
    expect(main().querySelector("[data-slot=order-state]")).toHaveTextContent(/filled/i);
  });

  it("an order the broker has not answered ends Unknown and stops sending in its symbol", async () => {
    await open(orderHref(SWING.agent_id, UNKNOWN_ORDER), "unknown-order");
    expect(steps()).toEqual(["Intent", "Submitting", "Unknown"]);
    expect(main().querySelector("[data-slot=sending-stopped]")).toHaveTextContent("Sending stopped in QRS");
    expect(main().querySelector("[data-slot=unknown-order]")).not.toBeNull();
  });

  it("the orders section lists working and past orders, each linked", async () => {
    await open(agentHref(SWING.agent_id, "orders"));
    for (const o of [...SWING.orders, ...SWING.past_orders]) expect(linkTo(orderHref(SWING.agent_id, o.client_order_id)), o.client_order_id).not.toBeNull();
  });
});

describe("gate decisions", () => {
  it("a denial shows the check that failed and the ones that did not run", async () => {
    await open(decisionHref(BTC.agent_id, "01JBWPQ5E6EYCNDY0YP57RCYBV"));
    const failed = check("order-size");
    expect(failed).toHaveAttribute("data-result", "failed");
    expect(failed).toHaveTextContent("Not allowed");
    const all = [...main().querySelectorAll("[data-slot=gate-checks] [data-check]")];
    const after = all.slice(all.indexOf(failed!) + 1);
    expect(after.length).toBeGreaterThan(0);
    for (const li of after) {
      expect(li).toHaveAttribute("data-result", "not_run");
      expect(li).toHaveTextContent("Not run");
    }
    for (const li of all.slice(0, all.indexOf(failed!))) expect(li).toHaveAttribute("data-result", "passed");
  });

  it("an allow passes every check and links the order it placed", async () => {
    await open(decisionHref(BTC.agent_id, "01JB8XE6SZAEAVSNTCPM5Q1NXW"));
    for (const li of main().querySelectorAll("[data-slot=gate-checks] [data-check]")) {
      expect(li).toHaveAttribute("data-result", "passed");
      expect(li).toHaveTextContent("Passed");
    }
    expect(linkTo(orderHref(BTC.agent_id, "cid_01JBH3BV4H15G5E4G7X0NTH82F"))).not.toBeNull();
  });

  it("an approved decision links the order it filled; one that asked links the request", async () => {
    const approved = await open(decisionHref(SWING.agent_id, "01JBYFGCW8T7SWM4FV4DK79Q9G"));
    expect(linkTo(orderHref(SWING.agent_id, "cid_01JCGPZ78Y1223KHAFF3AMAPB9"))).not.toBeNull();
    approved.unmount();
    await open(decisionHref(SWING.agent_id, "01JB5GQAPENECFSECZP11HYGCP"));
    expect(linkTo(`/approvals/${APPROVAL_IDS.swingXyz}`)).not.toBeNull();
  });

  it("an exit held behind an unknown order says Held, not Not allowed", async () => {
    await open(decisionHref(SWING.agent_id, "01JB64G5D6TZWBE8TXF7JJHHQG"), "unknown-order");
    const held = main().querySelector("[data-slot=gate-checks] [data-result=failed]");
    expect(held).toHaveTextContent("Held");
    expect(held).not.toHaveTextContent("Not allowed");
  });

  it("audit decisions link to the record only for roles that can open agents", async () => {
    const href = decisionHref(BTC.agent_id, "01JBWPQ5E6EYCNDY0YP57RCYBV");
    const owner = await open("/audit/decisions", "normal", "owner");
    expect(linkTo(href)).not.toBeNull();
    owner.unmount();
    await open("/audit/decisions", "normal", "auditor");
    expect(main()).toHaveTextContent(/BTC\/USD/);
    expect(main().querySelector('a[href^="/agents/"]')).toBeNull();
  });
});

describe("roles on records", () => {
  it.each<[Role, boolean]>([
    ["owner", true],
    ["operator", true],
    ["approver", false],
  ])("%s sees Close position: %s", async (role, shown) => {
    await open(XYZ_HREF, "normal", role);
    expect(screen.queryByRole("link", { name: "Close position…" }) !== null).toBe(shown);
  });
});

describe("chart states", () => {
  it("loading shows a chart-shaped skeleton and draws no line", async () => {
    await open(XYZ_HREF, "loading");
    expect(main().querySelector("[data-slot=skeleton] [data-slot=chart-skeleton]")).not.toBeNull();
    expect(main().querySelector("[data-slot=chart-canvas]")).toBeNull();
    expect(liveCharts()).toHaveLength(0);
  });

  it("the loading dashboard draws no chart either", async () => {
    await open("/", "loading");
    expect(main().querySelector("[data-slot=chart-canvas]")).toBeNull();
    expect(liveCharts()).toHaveLength(0);
  });

  it.each<Scenario>(["unreachable", "empty"])("%s draws no chart and makes up no values", async (scenario) => {
    await open("/", scenario);
    expect(main().querySelector("[data-slot=chart-canvas]")).toBeNull();
    expect(liveCharts()).toHaveLength(0);
    const record = await open(XYZ_HREF, scenario);
    expect(record.container.querySelector("[data-slot=chart-canvas]")).toBeNull();
  });

  it("stale marks say how old they are on the position and its chart's agent", async () => {
    await open(XYZ_HREF, "stale");
    const figures = within(screen.getByRole("region", { name: "Figures" }));
    const age = figures.getAllByText(/ago|as of/i);
    expect(age.length).toBeGreaterThan(0);
    for (const el of age) expect(el.closest("[data-stale]")).toHaveAttribute("data-stale", "true");
    expect(main().querySelector("[data-slot=chart-canvas]")).not.toBeNull();
  });

  it("a paused agent still draws its equity and its mandate", async () => {
    await open(agentHref(SWING.agent_id, "overview"), "paused");
    expect(main().querySelector("[data-slot=agent-equity] [data-slot=chart-canvas]")).not.toBeNull();
    expect(main().querySelector("[data-slot=agent-equity] [data-slot=level-legend]")).not.toBeNull();
  });

  it("an approval request shows its small chart with the proposed limit", async () => {
    await open(`/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(main().querySelector("[data-slot=approval-chart] [data-slot=chart-canvas]")).not.toBeNull();
    const [line] = liveCharts().flatMap((c) => c.series.filter((s) => s.type === "Line"));
    expect(line.priceLines.map((l) => l.title)).toEqual(["Proposed limit"]);
  });
});
