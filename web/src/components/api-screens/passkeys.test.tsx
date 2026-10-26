import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { type Fetch } from "@/api/client";
import { MOCK_PASSKEY_ID, MOCK_PRINCIPAL, createMockServer } from "@/api/mock-server";
import { RegistryScreen } from "@/components/screens/account-screens";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { findScreen } from "@/lib/screens";
import { setPathname } from "@/test/navigation";
import { mockAuthenticator, mockCreator } from "@/test/set-up";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(held: Array<{ passkey_id: string; cool_off_ends: string | null }>) {
  setPathname("/settings/profile");
  const server = createMockServer({ workspaceId: WS, scenario: "normal", passkeys: held });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    if (init.method === "POST") posts.push({ path: new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length), body: JSON.parse(String(init.body)) as Record<string, unknown> });
    return server.fetch(input, init);
  };
  const creator = mockCreator();
  const authenticator = mockAuthenticator(MOCK_PASSKEY_ID);
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5} creator={creator.create} authenticator={authenticator.get}>
        <main>
          <RegistryScreen screen={findScreen("/settings/profile")!} />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts, creator, authenticator, server };
}

afterEach(() => cleanup());

describe("your passkeys on the deployment, over the workspace API (§3.6, DEC-2108)", () => {
  it("adds a first passkey with no step-up, and says when it can first confirm", async () => {
    const { posts, creator, authenticator } = mount([]);
    expect(await screen.findByText("You have no passkey on this deployment yet.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add a passkey" }));
    await waitFor(() => expect(document.querySelector("[data-slot=passkey-added]")).not.toBeNull());
    expect(document.querySelector("[data-slot=passkey-added]")!.textContent).toMatch(/can confirm from .* after its first 24 hours/);
    expect(posts.map((p) => p.path)).toEqual(["/passkeys/registration-challenges", "/passkeys"]);
    expect(posts[1].body).toMatchObject({ passkey_id: creator.passkeyId, step_up: null, step_up_assertion: null });
    expect(creator.calls[0].publicKey?.user.name).toBe(MOCK_PRINCIPAL);
    expect(authenticator.calls).toHaveLength(0);
    const list = await screen.findByRole("list", { name: "Your passkeys on this deployment" });
    await waitFor(() => expect(list.querySelector(`[data-passkey="${creator.passkeyId}"]`)).not.toBeNull());
  });

  it("confirms a later passkey with one the member holds, by a credential_enrol step-up bound to the registration", async () => {
    const { posts, authenticator, server } = mount([{ passkey_id: MOCK_PASSKEY_ID, cool_off_ends: null }]);
    await screen.findByRole("list", { name: "Your passkeys on this deployment" });
    fireEvent.click(screen.getByRole("button", { name: "Add a passkey" }));
    await waitFor(() => expect(document.querySelector("[data-slot=passkey-added]")?.textContent).toMatch(/It can confirm now\./));
    const registration = posts.find((p) => p.path === "/passkeys");
    const [challenge] = server.challenges();
    expect(challenge.kind).toBe("credential_enrol");
    expect(challenge.used).toBe(true);
    expect(authenticator.calls).toHaveLength(1);
    expect(registration?.body).toMatchObject({ step_up: { method: "passkey", assertion_id: challenge.challenge_id }, step_up_assertion: { passkey_id: MOCK_PASSKEY_ID } });
  });

  it("adds nothing when the authenticator is dismissed, and says so", async () => {
    setPathname("/settings/profile");
    const server = createMockServer({ workspaceId: WS, scenario: "normal", passkeys: [] });
    const posts: string[] = [];
    const fetch: Fetch = (input, init) => {
      if (init.method === "POST") posts.push(new URL(input).pathname);
      return server.fetch(input, init);
    };
    render(
      <RoleProvider initial="owner">
        <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} creator={async () => Promise.reject(new DOMException("no", "NotAllowedError"))}>
          <main>
            <RegistryScreen screen={findScreen("/settings/profile")!} />
          </main>
        </ApiRuntimeProvider>
      </RoleProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Add a passkey" }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveTextContent("Not added: The passkey check was canceled or timed out."));
    expect(posts.some((p) => p.endsWith("/passkeys"))).toBe(false);
  });
});
