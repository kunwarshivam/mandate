import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { GoLive } from "./go-live";
import { ApiConnections } from "./connections";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { buildWorkspace } from "@/fixtures/workspace";
import { RuntimeProvider } from "@/lib/mock-runtime";
import { PENDING_COUNSEL, PENDING_POLICY, RISK_DISCLOSURES } from "@/lib/pending-counsel";
import { type Role, RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(node: React.ReactNode, role: Role = "owner") {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: string[] = [];
  const fetch: Fetch = async (input, init) => {
    if (init.method === "POST") posts.push(new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length));
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial={role}>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { server, posts };
}

afterEach(() => cleanup());

describe("Going live (DEC-2134; compliance doc, risk disclosures before going live)", () => {
  it("marks every disclosure and every platform limit as pending counsel", async () => {
    mount(<GoLive />);
    const disclosures = await screen.findByRole("region", { name: "Risks of an automated agent" });
    expect(within(disclosures).getByText(PENDING_COUNSEL)).toBeTruthy();
    for (const document of RISK_DISCLOSURES) {
      expect(within(disclosures).getByRole("article", { name: document.title }).getAttribute("data-document")).toBe(document.id);
      for (const paragraph of document.paragraphs) expect(paragraph.startsWith("Placeholder.")).toBe(true);
    }
    const limits = screen.getByRole("region", { name: "Platform limits" });
    expect(within(limits).getByText(PENDING_COUNSEL)).toBeTruthy();
    for (const item of PENDING_POLICY) expect(within(limits).getByText(item.label)).toBeTruthy();
  });

  it("names the conservative values the specs already set", () => {
    const values = Object.fromEntries(PENDING_POLICY.map((p) => [p.label, p.value]));
    expect(values).toEqual({
      "Lifetime-loss ceiling": "At most 20% of an agent's allocation",
      "Approval timeout, minimum": "At least 120 seconds",
      "Allowed jurisdictions": "United States only",
      Environments: "Paper only",
    });
  });

  it("asks the deployment for real money, shows its refusal, and says the agent stays on paper", async () => {
    const { server, posts } = mount(<GoLive />);
    const region = await screen.findByRole("region", { name: "Real money" });
    expect(within(region).getByText("off")).toBeTruthy();
    const agent = (within(region).getByRole("combobox", { name: "Agent" }) as HTMLSelectElement).value;
    fireEvent.click(within(region).getByRole("button", { name: "Ask to trade real money" }));
    const status = await within(region).findByText(/^Refused: real-money trading is off for your workspace until counsel signs off\./);
    expect(status.textContent).toContain("The agent keeps trading on paper.");
    expect(server.goLives()).toEqual([agent]);
    expect(posts).toEqual([`/agents/${agent}/go-live`]);
  });

  it.each<Role>(["approver", "viewer"])("offers a %s no request for real money", async (role) => {
    const { posts } = mount(<GoLive />, role);
    const region = await screen.findByRole("region", { name: "Real money" });
    expect(within(region).queryByRole("button", { name: "Ask to trade real money" })).toBeNull();
    expect(posts).toEqual([]);
  });

  it("over the fixtures, with no deployment, shows the same text and asks nothing", async () => {
    render(
      <RoleProvider initial="owner">
        <RuntimeProvider initial={buildWorkspace("normal")} tick={false}>
          <GoLive />
        </RuntimeProvider>
      </RoleProvider>,
    );
    const region = await screen.findByRole("region", { name: "Real money" });
    expect(within(region).getByText("Connect a deployment to ask for real money.")).toBeTruthy();
    expect(within(region).queryByRole("button")).toBeNull();
  });
});

describe("Robinhood on the connections screen (DEC-2134)", () => {
  it("is shown as real money, off, with no connect offered", async () => {
    mount(<ApiConnections go={() => undefined} />);
    const card = await screen.findByText(/Robinhood has no paper environment/);
    expect(card.textContent).toContain("Real money: off.");
    expect(screen.queryByRole("button", { name: /Robinhood/ })).toBeNull();
    expect(screen.getByRole("link", { name: "Going live" }).getAttribute("href")).toBe("/go-live");
  });
});
