import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiConnection, ApiConnections } from "./connections";
import { type Fetch } from "@/api/client";
import { MOCK_AUTHORIZATION_ORIGIN, MOCK_PASSKEY_ID, MOCK_PRINCIPAL, createMockServer } from "@/api/mock-server";
import { type Authenticator } from "@/api/passkey";
import { connectStepUpDigest } from "@/api/step-up";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { type Role, RoleProvider } from "@/lib/roles";
import { mockAuthenticator } from "@/test/set-up";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(node: React.ReactNode, options: { hostConfirms?: boolean; role?: Role; authenticator?: Authenticator | null; passkeys?: Array<{ passkey_id: string; cool_off_ends: string | null }> } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal", hostConfirms: options.hostConfirms ?? true, ...(options.passkeys ? { passkeys: options.passkeys } : {}) });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = async (input, init) => {
    if (init.method === "POST") posts.push({ path: new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length), body: JSON.parse(String(init.body)) as Record<string, unknown> });
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial={options.role ?? "owner"}>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5} authenticator={options.authenticator ?? null}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts, server };
}

afterEach(() => cleanup());

async function toHostConfirm() {
  fireEvent.click(await screen.findByRole("button", { name: "Connect Alpaca paper" }));
  const disclosure = screen.getByRole("region", { name: "Before you connect Alpaca paper trading" });
  expect(disclosure.getAttribute("data-document")).toBe("alpaca_oauth_token_breadth");
  expect(disclosure.getAttribute("data-version")).toBe("1");
  fireEvent.click(screen.getByRole("button", { name: "Connect with Alpaca" }));
  return screen.findByRole("region", { name: "Confirm on the host" });
}

describe("Connections over the workspace API (DEC-2620, DEC-2623)", () => {
  it("lists every connection the API serves, with its state", async () => {
    mount(<ApiConnections go={vi.fn()} />);
    const list = await screen.findByRole("list", { name: "Broker accounts" });
    expect(within(list).getByText("Connected.")).toBeTruthy();
    expect(within(list).getByRole("link", { name: "Alpaca" }).getAttribute("href")).toMatch(/^\/connections\/con_/);
  });

  it("shows the disclosure, asks for a code bound to it, and sends nothing to Alpaca before the host confirms", async () => {
    const go = vi.fn();
    const { posts } = mount(<ApiConnections go={go} />);
    const panel = await toHostConfirm();
    expect(posts).toEqual([{ path: "/step-up/challenges", body: { action: { kind: "connection", digest: connectStepUpDigest(WS) } } }]);
    const command = panel.querySelector("[data-slot=host-confirm-command]")?.textContent ?? "";
    expect(command).toContain(`mandate step-up confirm-connect --workspace ${WS} --principal ${MOCK_PRINCIPAL} --challenge `);
    expect(go).not.toHaveBeenCalled();
  });

  it("starts the connect with the challenge's evidence and the disclosure shown, then sends the browser to the authorization page", async () => {
    const go = vi.fn();
    const { posts } = mount(<ApiConnections go={go} />);
    const panel = await toHostConfirm();
    fireEvent.click(within(panel).getByRole("button", { name: "Connect with Alpaca" }));
    await waitFor(() => expect(go).toHaveBeenCalledTimes(1));
    expect(String(go.mock.calls[0][0]).startsWith(`${MOCK_AUTHORIZATION_ORIGIN}/`)).toBe(true);
    const start = posts.find((p) => p.path === "/connections/oauth/start");
    expect(start?.body).toMatchObject({ broker: "alpaca", environment: "paper", disclosure: { document: "alpaca_oauth_token_breadth", version: "1" }, step_up: { method: "cli_confirm" } });
    expect(screen.getByText("Taking you to Alpaca.")).toBeTruthy();
  });

  it("says nothing was started while the host has not confirmed, and never leaves the app", async () => {
    const go = vi.fn();
    mount(<ApiConnections go={go} />, { hostConfirms: false });
    const panel = await toHostConfirm();
    fireEvent.click(within(panel).getByRole("button", { name: "Connect with Alpaca" }));
    expect(await screen.findByText(/^Not started: your deployment found no confirmation of this code on the host/)).toBeTruthy();
    expect(go).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Connect Alpaca paper" })).toBeTruthy();
  });

  it("Cancel on the disclosure asks for nothing", async () => {
    const { posts } = mount(<ApiConnections go={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect Alpaca paper" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("region", { name: "Before you connect Alpaca paper trading" })).toBeNull();
    expect(posts).toEqual([]);
  });

  it("connects with the member's passkey: one challenge bound to the disclosure, the start carries its evidence and assertion, and no host command is shown", async () => {
    const go = vi.fn();
    const authenticator = mockAuthenticator(MOCK_PASSKEY_ID);
    const { posts } = mount(<ApiConnections go={go} />, { authenticator: authenticator.get });
    fireEvent.click(await screen.findByRole("button", { name: "Connect Alpaca paper" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect with Alpaca" }));
    await waitFor(() => expect(go).toHaveBeenCalledTimes(1));
    expect(String(go.mock.calls[0][0]).startsWith(`${MOCK_AUTHORIZATION_ORIGIN}/`)).toBe(true);
    expect(posts.filter((p) => p.path === "/step-up/challenges").map((p) => p.body)).toEqual([{ action: { kind: "connection", digest: connectStepUpDigest(WS) } }]);
    const start = posts.find((p) => p.path === "/connections/oauth/start");
    expect(Object.keys(start?.body ?? {}).sort()).toEqual(["broker", "disclosure", "environment", "step_up", "step_up_assertion"]);
    expect(start?.body).toMatchObject({ broker: "alpaca", environment: "paper", disclosure: { document: "alpaca_oauth_token_breadth", version: "1" }, step_up: { method: "passkey" }, step_up_assertion: { passkey_id: MOCK_PASSKEY_ID } });
    expect(authenticator.calls).toHaveLength(1);
    expect(screen.queryByRole("region", { name: "Confirm on the host" })).toBeNull();
  });

  it("falls back to the host's code while the member's only passkey is still in its first 24 hours, and says why", async () => {
    const go = vi.fn();
    const authenticator = mockAuthenticator(MOCK_PASSKEY_ID);
    const { posts } = mount(<ApiConnections go={go} />, { authenticator: authenticator.get, passkeys: [{ passkey_id: MOCK_PASSKEY_ID, cool_off_ends: "2099-01-01T00:00:00.000000000Z" }] });
    fireEvent.click(await screen.findByRole("button", { name: "Connect Alpaca paper" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect with Alpaca" }));
    expect(await screen.findByRole("region", { name: "Confirm on the host" })).toBeTruthy();
    expect(screen.getByText(/your passkey is new, and confirms only once its first 24 hours have passed/)).toBeTruthy();
    expect(authenticator.calls).toHaveLength(0);
    expect(posts.map((p) => p.path)).toEqual(["/step-up/challenges"]);
    expect(go).not.toHaveBeenCalled();
  });

  it("starts nothing when the passkey check is canceled, and offers the connect again", async () => {
    const go = vi.fn();
    const canceled: Authenticator = async () => {
      throw new DOMException("canceled", "NotAllowedError");
    };
    const { posts } = mount(<ApiConnections go={go} />, { authenticator: canceled });
    fireEvent.click(await screen.findByRole("button", { name: "Connect Alpaca paper" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect with Alpaca" }));
    expect(await screen.findByText("Not started: the passkey check was canceled or timed out.")).toBeTruthy();
    expect(posts.some((p) => p.path === "/connections/oauth/start")).toBe(false);
    expect(go).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Connect Alpaca paper" })).toBeTruthy();
  });

  it.each<Role>(["operator", "approver", "viewer"])("offers no connect to a %s", async (role) => {
    mount(<ApiConnections go={vi.fn()} />, { role });
    await screen.findByRole("list", { name: "Broker accounts" });
    expect(screen.queryByRole("button", { name: "Connect Alpaca paper" })).toBeNull();
  });
});

describe("one connection, where the callback lands (DEC-2626)", () => {
  it("reads it again while it is connecting, until it settles", async () => {
    const go = vi.fn();
    const { server } = mount(<ApiConnections go={go} />);
    const panel = await toHostConfirm();
    fireEvent.click(within(panel).getByRole("button", { name: "Connect with Alpaca" }));
    await waitFor(() => expect(server.started()).toHaveLength(1));
    const id = server.started()[0].connection_id;
    cleanup();
    const fetch: Fetch = (input, init) => server.fetch(input, init);
    render(
      <RoleProvider>
        <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000}>
          <ApiConnection connectionId={id} pollMs={10} />
        </ApiRuntimeProvider>
      </RoleProvider>,
    );
    expect(await screen.findByText("Connected.")).toBeTruthy();
    expect(screen.getByText(/^Access granted:/)).toBeTruthy();
  });

  it("says a connection the API does not know is not found", async () => {
    mount(<ApiConnection connectionId="conn_01JBR2C4D6E8F0G2H4J6K8M0N2" />);
    expect(await screen.findByText("Connection not found.")).toBeTruthy();
  });
});
