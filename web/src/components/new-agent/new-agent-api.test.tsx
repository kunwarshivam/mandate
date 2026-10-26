import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
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
import { MOCK_MODEL } from "@/api/mock-server";
import { INSTRUMENTS } from "@/fixtures/mandates";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(wrap: (send: Fetch) => Fetch = (send) => send) {
  setPathname("/agents/new");
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: string[] = [];
  const bodies: Array<Record<string, unknown>> = [];
  const fetch: Fetch = wrap((input, init) => {
    if (init.method === "POST") {
      posts.push(new URL(input).pathname);
      bodies.push(JSON.parse(String(init.body)) as Record<string, unknown>);
    }
    return server.fetch(input, init);
  });
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider
        source={{ origin: ORIGIN, workspaceId: WS }}
        fetch={fetch}
        refreshMs={60_000}
        statusPollMs={5}
        retryMs={5}
        authenticator={mockAuthenticator(MOCK_PASSKEY_ID).get}
      >
        <main>
          <NewAgentPage />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts, bodies };
}

async function send(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Your answer" }), {
    target: { value: text },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  for (let i = 0; i < 4; i++) await act(async () => {});
}

afterEach(() => cleanup());

const DEPLOY_REFUSED: Record<string, string> = {
  second_agent_on_account:
    "Not recorded: another agent that has not stopped trades on this account. Stop it first.",
  second_instrument:
    "Not recorded: an agent trades one instrument for now, and this version allows more.",
  connection_unrecorded:
    "Not recorded: the account this version names is not connected, or its connection was revoked. Connect it again in Connections.",
};

describe("setting up an agent over the workspace API (DEC-975, DEC-2128)", () => {
  it("is served over the API: the conversation, never the not-yet notice, read without a model and saying so", async () => {
    mount();
    await screen.findByRole("textbox", { name: "Your answer" });
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
    expect(screen.getByRole("main")).toHaveTextContent(API_NOTE);
    expect(screen.getByRole("main")).not.toHaveTextContent(
      /fixture stands in/i,
    );
  });

  /** The owner's answers up to the model, with `symbol` as the instrument. */
  async function toModel(symbol: string) {
    await screen.findByRole("textbox", { name: "Your answer" });
    await send("$3,000");
    await send("Grow it steadily");
    await send("$300");
    await send(symbol);
  }

  it("refuses a symbol the deployment has not registered, naming the ones it lists", async () => {
    mount();
    await toModel("MSFT");
    const listed = Object.values(INSTRUMENTS)
      .map((i) => i.symbol)
      .sort()
      .join(", ");
    expect(screen.getByRole("main")).toHaveTextContent(
      `MSFT is not registered on your deployment. It lists ${listed}.`,
    );
  });

  it("offers only the models the deployment registered, and creates the version with their registered asset id, version and content hash", async () => {
    const { posts, bodies } = mount();
    await toModel("XYZ");
    const choices = within(
      screen.getByRole("group", { name: "Models" }),
    ).getAllByRole("button");
    expect(choices.map((c) => c.querySelector("span")?.textContent)).toEqual([
      "Moving-average crossover",
    ]);
    expect(choices[0]).toHaveTextContent(MOCK_MODEL.content.methodology);
    fireEvent.click(choices[0]);
    await send("5");
    await send("20");
    fireEvent.click(
      await screen.findByRole("button", { name: "Create the version" }),
    );
    await waitFor(() =>
      expect(posts.filter((p) => p.endsWith("/mandate-versions"))).toHaveLength(
        1,
      ),
    );
    const mandate = bodies[
      posts.findIndex((p) => p.endsWith("/mandate-versions"))
    ] as {
      universe: { pinned_instruments: unknown[] };
      behavior: { signal_models: Array<Record<string, unknown>> };
    };
    expect(mandate.universe.pinned_instruments).toEqual([
      {
        asset_id: INSTRUMENTS.XYZ.asset_id,
        symbol: "XYZ",
        asset_class: "us_equity",
      },
    ]);
    expect(mandate.behavior.signal_models[0]).toMatchObject({
      id: MOCK_MODEL.model_id,
      version: MOCK_MODEL.model_version,
      content_hash: MOCK_MODEL.content_hash,
      params: [
        { key: "fast_periods", value: "5" },
        { key: "slow_periods", value: "20" },
      ],
    });
  });

  it("holds the create while the model's own constraint does not hold, saying which", async () => {
    const { posts } = mount();
    await toModel("XYZ");
    fireEvent.click(
      within(screen.getByRole("group", { name: "Models" })).getAllByRole(
        "button",
      )[0],
    );
    await send("20");
    await send("5");
    await waitFor(() =>
      expect(screen.getByRole("main")).toHaveTextContent(
        "Fast periods must be less than slow periods.",
      ),
    );
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    expect(posts.filter((p) => p.endsWith("/mandate-versions"))).toEqual([]);
  });

  it("refuses a registered instrument of another class, saying why, rather than set the envelope's asset classes for the owner", async () => {
    mount((send) => async (input, init) => {
      const answer = await send(input, init);
      if (!new URL(input).pathname.endsWith("/instruments")) return answer;
      const body = (await answer.json()) as { instruments: unknown[] };
      const eth = {
        instrument_id: "0b4a1c2e-6666-4a2b-9c3d-000000000006",
        symbol: "ETH",
        asset_class: "crypto",
        snapshot: `sha256:${"66".repeat(32)}`,
        registered_at: "2026-09-28T13:00:00.000000000Z",
      };
      return new Response(
        JSON.stringify({ ...body, instruments: [...body.instruments, eth] }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    });
    await toModel("ETH");
    expect(screen.getByRole("main")).toHaveTextContent(
      "ETH is registered as crypto, which this set-up does not offer yet",
    );
    expect(screen.queryByRole("group", { name: "Models" })).toBeNull();
  });

  it("names a deploy refused for its one-instrument or one-agent rule in words, and no agent exists (DEC-2809, DEC-2133)", async () => {
    for (const code of [
      "second_agent_on_account",
      "second_instrument",
      "connection_unrecorded",
    ]) {
      mount(
        (send) => (input, init) =>
          init.method === "POST" && new URL(input).pathname.endsWith("/agents")
            ? Promise.resolve(
                new Response(
                  JSON.stringify({
                    type: "about:blank",
                    status: 422,
                    title: "Request refused",
                    code: "invalid",
                    effect: "none",
                    event_id: null,
                    retryable: false,
                    violations: [
                      { path: "/mandate_version", code, message: "refused" },
                    ],
                  }),
                  {
                    status: 422,
                    headers: { "Content-Type": "application/problem+json" },
                  },
                ),
              )
            : send(input, init),
      );
      await toModel("XYZ");
      fireEvent.click(
        within(screen.getByRole("group", { name: "Models" })).getAllByRole(
          "button",
        )[0],
      );
      await send("5");
      await send("20");
      fireEvent.click(
        await screen.findByRole("button", { name: "Create the version" }),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "Confirm version 1" }),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "Deploy to paper" }),
      );
      await waitFor(() =>
        expect(
          document
            .querySelector("[data-slot=set-up-steps] [data-step=deploy]")
            ?.getAttribute("data-phase"),
        ).toBe("undelivered"),
      );
      const said = document.querySelector(
        "[data-slot=set-up-steps] [data-step=deploy]",
      )!.textContent!;
      expect(said).toContain(
        `${DEPLOY_REFUSED[code]} Version 1 was created and confirmed, and no agent exists.`,
      );
      expect(said).not.toContain(code);
      cleanup();
    }
  });

  it("says so when the deployment lists no model this page can set up", async () => {
    mount((send) => async (input, init) => {
      const answer = await send(input, init);
      if (!new URL(input).pathname.endsWith("/signal-models")) return answer;
      const body = (await answer.json()) as Record<string, unknown>;
      return new Response(JSON.stringify({ ...body, models: [] }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    });
    await toModel("XYZ");
    expect(screen.getByRole("main")).toHaveTextContent(
      "Your deployment lists no model this page can set up yet.",
    );
  });

  it("offers nothing when the deployment does not report its catalogue, and says so", async () => {
    mount(
      (send) => (input, init) =>
        new URL(input).pathname.endsWith("/signal-models")
          ? Promise.reject(new TypeError("down"))
          : send(input, init),
    );
    await waitFor(() =>
      expect(
        document.querySelector("[data-slot=catalogue-unreported]"),
      ).not.toBeNull(),
    );
    expect(screen.queryByRole("textbox", { name: "Your answer" })).toBeNull();
  });
});

describe("each passkey refusal's words: nothing was sent, and what exists", () => {
  const AT = "2026-10-26T10:00:00.000Z";
  const V = `sha256:${"1f".repeat(32)}` as const;
  const base: SetUp = {
    id: "set-1",
    agentId: "",
    version: V,
    created: {
      phase: "recorded",
      sentAt: AT,
      recordedAt: AT,
      base: null,
      classification: "risk_increasing",
      validated: true,
    },
    confirmed: {
      phase: "undelivered",
      sentAt: AT,
      record: {
        screen: "A5",
        environment: "paper",
        shown: [],
        paths: [],
        classification: "risk_increasing",
      },
      stepUp: null,
    },
    deployed: null,
  };
  const words = (reason: string) => {
    const view = renderWithRuntime(
      <StepStatus
        which="confirm"
        setUp={{ ...base, confirmed: { ...base.confirmed!, reason } }}
      />,
    );
    const text = view.container.textContent!;
    view.unmount();
    return text;
  };
  const STANDS =
    " Version 1 was created and is not confirmed, and no agent exists.";

  it("says nothing was sent while the passkey check is open, never 'Sent at'", () => {
    const view = renderWithRuntime(
      <StepStatus
        which="confirm"
        setUp={{
          ...base,
          confirmed: {
            ...base.confirmed!,
            phase: "sent",
            awaitingPasskey: true,
          },
        }}
      />,
    );
    expect(view.container.textContent).toBe(
      "Waiting for your passkey. Nothing is sent until your browser's passkey check is done.",
    );
    view.unmount();
  });

  it("names each reason in words, never by its code", () => {
    expect(words("passkey_unavailable")).toBe(
      `Not sent: this browser can't use a passkey.${STANDS}`,
    );
    expect(words("no_passkey")).toBe(
      `Not sent: you have no passkey on your deployment yet. Add one, then try again.${STANDS}`,
    );
    expect(words("passkey_cooling_off")).toBe(
      `Not sent: your passkey is new, and confirms only once its first 24 hours have passed.${STANDS}`,
    );
    expect(words("passkey_canceled")).toBe(
      `Not sent: the passkey check was canceled or timed out.${STANDS}`,
    );
    expect(words("passkey_failed")).toBe(
      `Not sent: your passkey's answer could not be read.${STANDS}`,
    );
    for (const code of ["step_up_missing", "step_up_stale", "step_up_method"])
      expect(words(code)).toBe(
        `Not recorded: your deployment did not accept the passkey check.${STANDS}`,
      );
    expect(words("step_up_required")).toBe(
      `Not recorded: your deployment asked for a passkey check that was not sent.${STANDS}`,
    );
    expect(words("second_instrument")).toBe(
      `Not recorded: an agent trades one instrument for now, and this version allows more.${STANDS}`,
    );
    expect(words("second_agent_on_account")).toBe(
      `Not recorded: another agent that has not stopped trades on this account. Stop it first.${STANDS}`,
    );
    expect(words("connection_unrecorded")).toBe(
      `Not recorded: the account this version names is not connected, or its connection was revoked. Connect it again in Connections.${STANDS}`,
    );
    for (const code of ["passkey_canceled", "step_up_missing", "no_passkey"])
      expect(words(code)).not.toContain(code);
  });
});
