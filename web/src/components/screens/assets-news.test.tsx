import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { HEADLINES } from "@/fixtures/news";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { positionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { AssetsSection } from "./assets-section";
import { NewsSection } from "./news-section";

const NOW = "2026-09-28T14:05:18-04:00";

describe("the dashboard's assets", () => {
  const ws = buildWorkspace("normal");

  it("lists every held instrument across the account, then the agents' cash and what no agent manages", () => {
    renderWithRuntime(<AssetsSection ws={ws} />);
    const rows = [...document.querySelectorAll("[data-slot=account-assets] [data-slot=asset-row]")];
    const held = new Set(ws.agents.flatMap((a) => a.positions.map((p) => p.instrument.symbol)));
    expect(rows.filter((r) => r.getAttribute("data-kind") === "asset")).toHaveLength(held.size);
    expect(rows.at(-1)).toHaveAttribute("data-kind", "unmanaged");
    for (const bar of document.querySelectorAll("[data-slot=share-bar]")) expect(bar).toHaveAttribute("aria-hidden", "true");
  });

  it("gives each agent its owl, a link to it, and a link to each position it holds", () => {
    renderWithRuntime(<AssetsSection ws={ws} />);
    const agents = [...document.querySelectorAll<HTMLElement>("[data-slot=agent-assets]")];
    expect(agents).toHaveLength(ws.agents.length);
    ws.agents.forEach((agent, i) => {
      const row = agents[i];
      expect(row.querySelector("svg[data-slot=owl]")).not.toBeNull();
      expect(within(row).getByRole("link", { name: agent.label })).toHaveAttribute("href", `/agents/${agent.agent_id}`);
      const links = [...row.querySelectorAll("[data-slot=mandate-universe] a")].map((a) => a.getAttribute("href"));
      expect(links.sort()).toEqual(agent.positions.map((p) => positionHref(agent.agent_id, p.instrument.asset_id)).sort());
    });
  });

  it("shows unrealized P&L only with its performance disclosure", () => {
    renderWithRuntime(<AssetsSection ws={ws} />);
    const region = screen.getByRole("region", { name: "Assets" });
    expect(region.querySelector("[data-direction]")).not.toBeNull();
    expect(region.querySelector("[data-placeholder=performance]")).not.toBeNull();
  });
});

describe("an agent's news", () => {
  const ws = buildWorkspace("normal");

  it.each(ws.agents.map((a) => [a.label, a] as const))("shows %s only sample headlines about what it holds or may trade, newest first, and links nowhere", (_, agent) => {
    renderWithRuntime(<NewsSection ws={ws} agent={agent} now={NOW} />);
    const region = screen.getByRole("region", { name: "News" });
    const symbols = new Set([...agent.positions.map((p) => p.instrument.symbol), ...agent.mandate.universe.pinned_instruments.map((i) => i.symbol)]);
    const expected = HEADLINES.filter((h) => h.symbols.some((s) => symbols.has(s)));
    expect(region.querySelectorAll("a")).toHaveLength(0);
    if (expected.length === 0) {
      expect(region).toHaveTextContent("No headlines about what this agent holds or may trade.");
      return;
    }
    const list = within(region).getByRole("list", { name: "Sample headlines" });
    const items = [...list.querySelectorAll("[data-slot=headline]")];
    expect(items).toHaveLength(expected.length);
    const times = [...list.querySelectorAll("time")].map((t) => Date.parse(t.getAttribute("datetime")!));
    expect(times).toEqual([...times].sort((a, b) => b - a));
    for (const li of items) {
      const h = HEADLINES.find((x) => li.textContent?.includes(x.title))!;
      expect(h.symbols.some((s) => symbols.has(s)), h.title).toBe(true);
    }
  });

  it("leaves out headlines about what only other agents or the owner hold", () => {
    const swing = ws.agents.find((a) => a.agent_id === AGENT_IDS.swing)!;
    renderWithRuntime(<NewsSection ws={ws} agent={swing} now={NOW} />);
    const region = screen.getByRole("region", { name: "News" });
    const own = new Set([...swing.positions.map((p) => p.instrument.symbol), ...swing.mandate.universe.pinned_instruments.map((i) => i.symbol)]);
    const others = HEADLINES.filter((h) => !h.symbols.some((s) => own.has(s)));
    expect(others.length).toBeGreaterThan(0);
    for (const h of others) expect(region).not.toHaveTextContent(h.title);
  });

  it("says so when the agent holds nothing and pins nothing", () => {
    const agent = { ...ws.agents[0], positions: [], mandate: { ...ws.agents[0].mandate, universe: { ...ws.agents[0].mandate.universe, pinned_instruments: [] } } };
    renderWithRuntime(<NewsSection ws={ws} agent={agent} now={NOW} />);
    expect(screen.getByRole("region", { name: "News" })).toHaveTextContent("No headlines about what this agent holds or may trade.");
  });
});
