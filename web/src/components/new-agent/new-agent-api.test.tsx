import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import NewAgentPage from "@/app/(app)/agents/new/page";
import { type Fetch } from "@/api/client";
import { MOCK_PASSKEY_ID, createMockServer } from "@/api/mock-server";
import type { SetUp } from "@/lib/mock-runtime";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { setPathname } from "@/test/navigation";
import { mockAuthenticator } from "@/test/set-up";
import { renderWithRuntime } from "@/test/harness";
import { API_NOTE } from "./chat";
import { StepStatus } from "./set-up-steps";
import { CATALOGUE_NOT_SERVED } from "./new-agent-flow";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount() {
  setPathname("/agents/new");
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: string[] = [];
  const fetch: Fetch = (input, init) => {
    if (init.method === "POST") posts.push(new URL(input).pathname);
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5} authenticator={mockAuthenticator(MOCK_PASSKEY_ID).get}>
        <main>
          <NewAgentPage />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts };
}

async function send(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Your answer" }), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  for (let i = 0; i < 4; i++) await act(async () => {});
}

afterEach(() => cleanup());

describe("setting up an agent over the workspace API (DEC-975, DEC-2128)", () => {
  it("is served over the API: the conversation, never the not-yet notice, read without a model and saying so", async () => {
    mount();
    await screen.findByRole("textbox", { name: "Your answer" });
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
    expect(screen.getByRole("main")).toHaveTextContent(API_NOTE);
    expect(screen.getByRole("main")).not.toHaveTextContent(/fixture stands in/i);
  });

  it("reads the owner's figures exactly as written, and creates nothing while the deployment does not list its instruments and models", async () => {
    const { posts } = mount();
    await screen.findByRole("textbox", { name: "Your answer" });
    await send("$3,000");
    await send("Grow it steadily");
    await send("$300");
    await send("MSFT");
    fireEvent.click(within(screen.getByRole("group", { name: "Models" })).getByRole("button", { name: /^Momentum/ }));
    await send("20");
    await waitFor(() => expect(document.querySelector("[data-slot=set-up-steps] [data-slot=blocked]")?.textContent).toBe(CATALOGUE_NOT_SERVED));
    expect(screen.queryByRole("button", { name: "Create the version" })).toBeNull();
    expect(posts.filter((p) => p.endsWith("/mandate-versions"))).toEqual([]);
  });
});

describe("each passkey refusal's words: nothing was sent, and what exists", () => {
  const AT = "2026-10-26T10:00:00.000Z";
  const V = `sha256:${"1f".repeat(32)}` as const;
  const base: SetUp = {
    id: "set-1",
    agentId: "",
    version: V,
    created: { phase: "recorded", sentAt: AT, recordedAt: AT, base: null, classification: "risk_increasing", validated: true },
    confirmed: { phase: "undelivered", sentAt: AT, record: { screen: "A5", environment: "paper", shown: [], paths: [], classification: "risk_increasing" }, stepUp: null },
    deployed: null,
  };
  const words = (reason: string) => {
    const view = renderWithRuntime(<StepStatus which="confirm" setUp={{ ...base, confirmed: { ...base.confirmed!, reason } }} />);
    const text = view.container.textContent!;
    view.unmount();
    return text;
  };
  const STANDS = " Version 1 was created and is not confirmed, and no agent exists.";

  it("names each reason in words, never by its code", () => {
    expect(words("passkey_unavailable")).toBe(`Not sent: this browser can't use a passkey.${STANDS}`);
    expect(words("no_passkey")).toBe(`Not sent: you have no passkey on your deployment yet. Add one, then try again.${STANDS}`);
    expect(words("passkey_cooling_off")).toBe(`Not sent: your passkey is new, and confirms only once its first 24 hours have passed.${STANDS}`);
    expect(words("passkey_canceled")).toBe(`Not sent: the passkey check was canceled or timed out.${STANDS}`);
    expect(words("passkey_failed")).toBe(`Not sent: your passkey's answer could not be read.${STANDS}`);
    for (const code of ["step_up_missing", "step_up_stale", "step_up_method"]) expect(words(code)).toBe(`Not recorded: your deployment did not accept the passkey check.${STANDS}`);
    expect(words("step_up_required")).toBe(`Not recorded: your deployment asked for a passkey check that was not sent.${STANDS}`);
    for (const code of ["passkey_canceled", "step_up_missing", "no_passkey"]) expect(words(code)).not.toContain(code);
  });
});
