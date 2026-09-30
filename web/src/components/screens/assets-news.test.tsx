import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { HEADLINES } from "@/fixtures/news";
import { buildWorkspace } from "@/fixtures/workspace";
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

describe("the dashboard's news", () => {
  it("shows sample headlines about what the agents hold or may trade, newest first, and links nowhere", () => {
    const ws = buildWorkspace("normal");
    renderWithRuntime(<NewsSection ws={ws} now={NOW} />);
    const region = screen.getByRole("region", { name: "News" });
    const list = within(region).getByRole("list", { name: "Sample headlines" });
    const times = [...list.querySelectorAll("time")].map((t) => Date.parse(t.getAttribute("datetime")!));
    expect(times.length).toBeGreaterThan(0);
    expect(times).toEqual([...times].sort((a, b) => b - a));
    expect(region.querySelectorAll("a")).toHaveLength(0);
    const symbols = new Set([
      ...ws.agents.flatMap((a) => [...a.positions.map((p) => p.instrument.symbol), ...a.mandate.universe.pinned_instruments.map((i) => i.symbol)]),
      ...ws.external_positions.map((e) => e.instrument.symbol),
    ]);
    for (const li of list.querySelectorAll("[data-slot=headline]")) {
      const h = HEADLINES.find((x) => li.textContent?.includes(x.title))!;
      expect(h.symbols.some((s) => symbols.has(s)), h.title).toBe(true);
    }
  });

  it("says so when nothing on the account has a headline", () => {
    const ws = { ...buildWorkspace("normal"), agents: [], external_positions: [] };
    renderWithRuntime(<NewsSection ws={ws} now={NOW} />);
    expect(screen.getByRole("region", { name: "News" })).toHaveTextContent("No headlines about what your agents hold or may trade.");
  });
});
