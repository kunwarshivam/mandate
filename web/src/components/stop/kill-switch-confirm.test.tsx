import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { StopRecordScreen } from "./record-screen";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const AGENT = "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";

/**
 * The kill switch's confirmation (D10) as the specs set it (DEC-2410): a kill switch at any scope
 * is never refused and needs no step-up for its stop and flatten (identity spec ID-5, workspace API
 * API-7, mandate spec §6.1, `AGENTS.md` rule 13), so the screen asks for a plain confirm, never a
 * passkey; and it announces only what happened: recorded, then applied by the runtime.
 */
function mount(options: { held?: Promise<void>; authenticator?: null } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = async (input, init) => {
    const method = init.method ?? "GET";
    const path = new URL(input).pathname;
    if (method === "POST") posts.push({ path, body: JSON.parse(typeof init.body === "string" ? init.body : "{}") as Record<string, unknown> });
    if (method === "GET" && path.includes("/commands/") && options.held) await options.held;
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5} {...("authenticator" in options ? { authenticator: options.authenticator } : {})}>
        <StopRecordScreen kind="kill" targetId={AGENT} />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts, switches: () => posts.filter((p) => p.path.endsWith("/kill-switch")) };
}

async function activate() {
  fireEvent.click(await screen.findByRole("button", { name: /^Activate the kill switch/ }));
  const dialog = await screen.findByRole("dialog", { name: "Confirm the kill switch" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Activate" }));
}

afterEach(() => {
  cleanup();
});

describe("the kill switch's confirmation (DEC-2410)", () => {
  it("asks for a plain confirm, never a passkey, and says so", async () => {
    mount();
    const button = await screen.findByRole("button", { name: /^Activate the kill switch/ });
    expect(button.textContent).not.toMatch(/Needs your passkey/);
    expect(button.textContent).toMatch(/No passkey needed/);
    fireEvent.click(button);
    const dialog = await screen.findByRole("dialog", { name: "Confirm the kill switch" });
    expect(within(dialog).queryByRole("button", { name: "Use passkey" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Confirm it is you" })).toBeNull();
  });

  it("is sent once on Activate, with no step-up, even where the browser has no passkey at all", async () => {
    const { switches } = mount({ authenticator: null });
    await activate();
    await waitFor(() => expect(switches()).toHaveLength(1), { timeout: 5000 });
    expect(switches()[0].body).toMatchObject({ scope: { kind: "agent", id: AGENT } });
    expect(switches()[0].body.step_up ?? null).toBeNull();
  });

  it("canceled is not sent, and says so in words", async () => {
    const { switches } = mount();
    fireEvent.click(await screen.findByRole("button", { name: /^Activate the kill switch/ }));
    const dialog = await screen.findByRole("dialog", { name: "Confirm the kill switch" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Canceled. Nothing was sent.")).toBeTruthy();
    expect(switches()).toHaveLength(0);
  });

  it("says it is being applied once recorded, and that orders are canceled and positions sold only once the runtime applied it", async () => {
    let release = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    mount({ held });
    await activate();
    expect(await screen.findByText(/Waiting for the runtime to apply it\.$/, {}, { timeout: 5000 })).toBeTruthy();
    expect(screen.queryByText(/positions sold/)).toBeNull();
    release();
    expect(await screen.findByText(/orders canceled, positions sold, agent stopped\.$/, {}, { timeout: 5000 })).toBeTruthy();
  });
});
