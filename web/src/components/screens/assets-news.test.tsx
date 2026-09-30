import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { buildWorkspace } from "@/fixtures/workspace";
import { positionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { AssetsSection } from "./assets-section";

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
