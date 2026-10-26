import { act, fireEvent, screen, within } from "@testing-library/react";
import {
  type MockInstance,
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { KEY } from "@/components/kumo/key";
import {
  PASSKEY_ANSWER_MS,
  type Passkey,
} from "@/components/stop/step-up-dialog";
import type { Scenario, Workspace } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { dec, fromInt, mul } from "@/lib/decimal";
import { mandateVersion } from "@/lib/fixture-journey";
import type { SetUp } from "@/lib/mock-runtime";
import { confirmStepUpDigest } from "@/api/confirm";
import { BACKTEST_IDS } from "@/fixtures/backtests";
import { VERSION } from "@/fixtures/versions";
import type { Role } from "@/lib/roles";
import { RECORD_AFTER_MS, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { RuntimeProbe, probed } from "@/test/runtime-probe";
import { setUpThrough } from "@/test/set-up";
import { failingPasskey, press, stepUpDialog } from "@/test/step-up";
import {
  ADVICE_REPLY,
  type Compiler,
  type CompilerInput,
  fixtureCompiler,
} from "./compiler";
import { INTRO, READY } from "./conversation";
import {
  MODELS,
  type Read,
  compile,
  mandateFrom,
  readAnswers,
  readLoss,
  readStrategy,
  unaskedUsd,
} from "./draft";
import { NewAgentFlow } from "./new-agent-flow";
import { DeployDetails, StepStatus, deployLines } from "./set-up-steps";
import { GAP_NOTE, confirmedPaths, summaryLines } from "./summary";

const main = () => screen.getByRole("main");
const heading = () => screen.getByRole("heading", { level: 1 });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const composer = () => screen.getByRole("textbox", { name: "Your answer" });
const log = () => screen.getByRole("log", { name: "Conversation" });
const thinking = () =>
  main().querySelector<HTMLElement>("[data-slot=thinking]")!;

const setUps = (): SetUp[] => probed().setUps;

const INSTANT = fixtureCompiler({ latencyMs: 0 });

function renderFlow({
  role = "owner",
  passkey,
  compiler = INSTANT,
  workspace,
  scenario = "normal",
}: {
  role?: Role;
  passkey?: Passkey;
  compiler?: Compiler;
  workspace?: (ws: Workspace) => Workspace;
  scenario?: Scenario;
} = {}) {
  setPathname("/agents/new");
  return renderWithRuntime(
    <main>
      <NewAgentFlow compiler={compiler} />
      <RuntimeProbe />
    </main>,
    scenario,
    {
      role,
      ...(passkey ? { passkey } : {}),
      ...(workspace ? { workspace } : {}),
    },
  );
}

/** Lets the compiler's promise and the state it sets land. */
async function settle() {
  for (let i = 0; i < 4; i++) await act(async () => {});
}

async function send(text: string) {
  fireEvent.change(composer(), { target: { value: text } });
  fireEvent.click(button("Send"));
  await settle();
}

const replies = () => [
  ...log().querySelectorAll<HTMLElement>("[data-slot=said]"),
];
/** The platform's last reply, its sentences one per entry. */
const lastReply = () =>
  [...replies().at(-1)!.querySelectorAll("p")].map((p) => p.textContent);
const summaries = () => [
  ...log().querySelectorAll<HTMLElement>("[data-slot=summary]"),
];
const summary = () => summaries().at(-1)!;
const row = (label: string) =>
  [...summary().querySelectorAll<HTMLElement>("[data-slot=summary-row]")].find(
    (r) => r.querySelector("dt")?.textContent === label,
  )!;
const sectionRows = (key: string) => [
  ...summary().querySelectorAll<HTMLElement>(
    `[data-slot=summary-section][data-section=${key}] [data-slot=summary-row]`,
  ),
];

function chooseModel(name: RegExp) {
  fireEvent.click(
    within(screen.getByRole("group", { name: "Models" })).getByRole("button", {
      name,
    }),
  );
}

/** The fixture account has $3,478.36 that no agent uses, so a deployment asks for less. */
async function toSummary({
  money = "$3,000",
  goal = "Grow it steadily",
  loss = "$300",
  symbols = "MSFT",
} = {}) {
  await send(money);
  await send(goal);
  await send(loss);
  await send(symbols);
  chooseModel(/^Momentum/);
  await send("20");
}

/** One of the three steps under the summary (DEC-2401), and its phase once sent. */
const step = (key: "version" | "confirm" | "deploy") =>
  main().querySelector<HTMLElement>(
    `[data-slot=set-up-steps] [data-step=${key}]`,
  );
const phase = (key: "version" | "confirm" | "deploy") =>
  step(key)?.getAttribute("data-phase") ?? null;

function withPasskey(name: string) {
  fireEvent.click(button(name));
  press("Use passkey");
  act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
}

/** Step 1, then step 2 with a passkey once step 1 is recorded: the agent's version, confirmed. Fake timers. */
function createAndConfirm() {
  fireEvent.click(button("Create the version"));
  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
  withPasskey("Confirm version 1");
  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
}

/** All three steps, each once the one before it is recorded; leaves the deployment sent. Fake timers. */
function setUpWithPasskeys() {
  createAndConfirm();
  withPasskey("Deploy to paper");
}

let fetchSpy: MockInstance;
let setItem: MockInstance;
let getItem: MockInstance;

beforeEach(() => {
  fetchSpy = vi
    .spyOn(globalThis, "fetch")
    .mockRejectedValue(new Error("no network in the prototype"));
  setItem = vi.spyOn(Storage.prototype, "setItem");
  getItem = vi.spyOn(Storage.prototype, "getItem");
});

afterEach(() => vi.restoreAllMocks());

const EXAMPLES = /\$\s?\d|\d\s?%|e\.g\.|for example/i;
const CEILING = / This workspace allows at most .*$/;

describe("one conversation, as in any chat", () => {
  it("opens with one plain question, an empty composer, and no suggested amounts", () => {
    renderFlow();
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(lastReply()).toEqual([INTRO]);
    expect(composer()).toHaveValue("");
    expect(composer()).not.toHaveAttribute("placeholder");
    expect(main().textContent).not.toMatch(EXAMPLES);
    expect(main().querySelectorAll("input, textarea")).toHaveLength(1);
    expect(button("Send")).toBeDisabled();
  });

  it("names the composer by a label written above it, not by an attribute alone (C-4)", () => {
    renderFlow();
    const field = composer();
    expect(field).not.toHaveAttribute("aria-label");
    const labels = [...(field as HTMLTextAreaElement).labels!];
    expect(labels.map((l) => l.textContent)).toEqual(["Your answer"]);
    expect(
      labels[0].compareDocumentPosition(field) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(labels[0].textContent).not.toMatch(EXAMPLES);
    expect(button("Send")).toHaveAccessibleName("Send");
  });

  it("says what it understood in a sentence, then asks the next thing, with no cards to confirm", async () => {
    renderFlow();
    await send("$3,000");
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use.",
      "What's it for, in your own words?",
    ]);
    expect(composer()).toHaveValue("");
    expect(composer()).toHaveFocus();

    await send("Grow it steadily");
    expect(lastReply()).toEqual([
      "Got it: the goal “Grow it steadily”.",
      "How much could you stand to lose, in total? In dollars, or as a share of the money. This workspace allows at most 20% of the money, $600.00.",
    ]);
    await send("$300");
    expect(lastReply()).toEqual([
      "Got it: a loss limit of $300.00 (10% of the money).",
      "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.",
    ]);
    for (const reply of replies())
      expect(within(reply).queryByRole("button")).toBeNull();
    for (const line of replies().flatMap((r) =>
      [...r.querySelectorAll("p")].map((p) => p.textContent!),
    )) {
      if (!line.startsWith("Got it"))
        expect(line.replace(CEILING, "")).not.toMatch(EXAMPLES);
    }
  });

  it("reads everything said at once, and asks only what is still missing", async () => {
    renderFlow();
    await send(
      "Use $3,000 on big tech. Stop when it is up 10%. I can stand to lose 10%. Avoid oil companies.",
    );
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use, a loss limit of $300.00 (10% of the money), and the goal “Stop when it is up 10%”.",
      "No limit can check “Avoid oil companies”, so it isn't enforced. The agent gets it as a note.",
      "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.",
    ]);
    await send("MSFT");
    chooseModel(/^Momentum/);
    await send("20");
    expect(row("Goal")).toHaveTextContent(
      "Stops when its equity reaches $3,300.00",
    );
    expect(
      within(summary()).getByText("“Avoid oil companies”"),
    ).toBeInTheDocument();
  });

  it("reads one run-on sentence the way a person means it, and repeats no part of the goal as a note", async () => {
    renderFlow();
    await send(
      "I want it to use about three grand to grow it steadily, avoiding oil companies. I could stand to lose $300.",
    );
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use, a loss limit of $300.00 (10% of the money), and the goal “grow it steadily”.",
      "I read “about three grand” as $3,000.00.",
      "No limit can check “avoiding oil companies”, so it isn't enforced. The agent gets it as a note.",
      "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.",
    ]);
  });

  it("reads an amount written in words, and says how it read it", async () => {
    renderFlow();
    await send("about three grand");
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use.",
      "I read “about three grand” as $3,000.00.",
      "What's it for, in your own words?",
    ]);
    expect(within(log()).queryByRole("button")).toBeNull();
  });

  it("shows that it is reading, and takes no second message until it has", async () => {
    let answer: (v: unknown) => void = () => {};
    const seen: CompilerInput[] = [];
    const slow: Compiler = (input) => {
      seen.push(input);
      return new Promise((resolve) => (answer = resolve));
    };
    renderFlow({ compiler: slow });
    fireEvent.change(composer(), { target: { value: "$3,000" } });
    fireEvent.click(button("Send"));
    await settle();
    expect(thinking()).toHaveTextContent("Reading your message…");
    expect(thinking()).toHaveAttribute("role", "status");
    fireEvent.change(composer(), { target: { value: "more" } });
    expect(button("Send")).toBeDisabled();
    expect(seen).toEqual([
      {
        messages: [{ id: expect.any(String), text: "$3,000" }],
        asked: "money",
        models: [
          expect.objectContaining({ name: "Mean reversion" }),
          expect.objectContaining({ name: "Momentum" }),
        ],
      },
    ]);

    const raw = await INSTANT(seen[0]);
    await act(async () => answer(raw));
    await settle();
    expect(thinking()).toHaveTextContent("");
    expect(lastReply()).toContain("What's it for, in your own words?");
  });

  it("says what it could not find instead of guessing", async () => {
    renderFlow();
    await send("Something calm for the long run");
    expect(lastReply()).toEqual([
      "No limit can check “Something calm for the long run”, so it isn't enforced. The agent gets it as a note.",
      "I couldn't find an amount in that. How much money can it use, in dollars?",
    ]);
  });

  it("says what shape of answer would be read on a second miss in a row, never a value, instead of the same sentence again", async () => {
    renderFlow();
    await send("yes");
    expect(lastReply()).toEqual([
      "I couldn't find an amount in that. How much money can it use, in dollars?",
    ]);
    await send("yes");
    expect(lastReply()).toEqual([
      "I still couldn't find an amount. Write the figure on its own, in dollars, with nothing else in the message.",
    ]);
    expect(lastReply().join(" ")).not.toMatch(EXAMPLES);
    await send("$3,000");
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use.",
      "What's it for, in your own words?",
    ]);
    await send("Grow it");
    await send("yes");
    expect(lastReply()).toEqual([
      "I couldn't find a loss in that. Write it in dollars, or as a percentage of the money.",
    ]);
  });

  it("names the range a model setting must fall in when it asks, so the owner never guesses, and still proposes no value", async () => {
    for (const m of MODELS)
      for (const p of m.params)
        expect(p.question).toMatch(/from [\d.]+ to [\d.]+/i);
  });

  it("refuses a loss above the workspace's ceiling, never moving the answer to fit", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it steadily");
    await send("$2,000");
    expect(lastReply()).toEqual([
      "That is more than this workspace allows: at most 20% of the money, $600.00. Change your answer to continue.",
    ]);
    await send("$600");
    expect(lastReply()[0]).toBe(
      "Got it: a loss limit of $600.00 (20% of the money).",
    );
  });

  it("refuses more money than the account has free when it is said (V-002), and says how much is free", async () => {
    renderFlow();
    await send("$5,000");
    expect(lastReply()).toEqual([
      "Your paper account has $3,478.36 that no agent uses, less than the $5,000.00 this agent would use. Write a smaller amount.",
    ]);
  });

  it("refuses a symbol another agent already trades on the account (V-006), and takes another", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("XYZ");
    expect(lastReply()).toEqual([
      "XYZ is already traded by Agent 2. One agent trades an instrument on an account; choose another.",
    ]);
    await send("msft, aapl");
    expect(lastReply()).toEqual([
      "An agent trades one instrument for now. Name one symbol; another instrument needs an agent of its own.",
    ]);
    await send("msft");
    expect(lastReply()[0]).toBe("Got it: MSFT to trade.");
    expect(screen.getByRole("group", { name: "Models" })).toBeInTheDocument();
  });

  it("proposes one instrument at most in the version, whatever was asked (V-048, DEC-2809)", async () => {
    renderFlow();
    await toSummary();
    expect(
      row("Most instruments at once").querySelector("dd")?.textContent,
    ).toMatch(/^1(?!\d)/);
  });
});

describe("the strategy, the owner's choice", () => {
  it("lists the models in alphabetical order, none chosen, and no word ranks them (brief A3)", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("MSFT");
    const choices = within(
      screen.getByRole("group", { name: "Models" }),
    ).getAllByRole("button");
    expect(choices.map((c) => c.querySelector("span")!.textContent)).toEqual([
      "Mean reversion",
      "Momentum",
    ]);
    for (const c of choices) {
      expect(c).not.toHaveAttribute("aria-pressed");
      expect(c).not.toHaveFocus();
    }
    expect(replies().at(-1)!.textContent).not.toMatch(
      /recommended|popular|best|return|sharpe|win rate/i,
    );
  });

  it("takes a model by name, asks each setting with no value filled in, and refuses one out of range", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("MSFT");
    await send("mean reversion");
    expect(lastReply()).toEqual([
      "Got it: the Mean reversion model.",
      "How many bars should the model read back over? A whole number from 2 to 500.",
    ]);
    await send("20");
    expect(lastReply()).toEqual([
      "Got it: lookback of 20.",
      "How far below its average, in standard deviations, must a price be before the model scores a buy? From 0.5 to 4, with at most two decimals.",
    ]);
    await send("9");
    expect(lastReply()).toEqual([
      "Entry z-score: a number from 0.5 to 4, with at most two decimals.",
    ]);
    await send("1.5");
    expect(lastReply()).toEqual(["Got it: entry z-score of 1.5.", READY]);
    expect(row("Its settings")).toHaveTextContent(
      "Lookback, in bars: 20; Entry z-score: 1.5",
    );
    expect(within(row("Its settings")).queryByText("Proposed")).toBeNull();
  });

  it("explains a model and refuses to choose for the owner, in plain text with no buttons", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("Which stocks should I buy?");
    expect(lastReply()).toEqual([
      ADVICE_REPLY,
      "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.",
    ]);
    expect(within(replies().at(-1)!).queryByRole("button")).toBeNull();
    await send("MSFT");
    await send("What is momentum?");
    expect(lastReply()[0]).toMatch(/^Momentum: /);
    expect(within(replies().at(-1)!).getAllByRole("button")).toEqual(
      within(screen.getByRole("group", { name: "Models" })).getAllByRole(
        "button",
      ),
    );
  });

  it("withholds a reply that reads as advice, whatever the model wrote", async () => {
    renderFlow({
      compiler: async () => ({
        readings: [],
        not_enforced: [],
        intent: "other",
        reply: "You should buy MSFT, it will double.",
      }),
    });
    await send("anything");
    expect(lastReply()).toEqual([
      "I left out a reply that read like advice. Owlhead doesn't give any.",
      "How much money can it use, in dollars? It trades on paper, with simulated money.",
    ]);
    expect(main()).not.toHaveTextContent("You should buy MSFT");
  });
});

describe("the agent, shown once, then set up in three recorded steps", () => {
  it("shows every value once nothing is missing, marks what Owlhead drafted, and sends nothing yet", async () => {
    renderFlow();
    await toSummary();
    expect(summaries()).toHaveLength(1);
    expect(
      within(summary()).getByRole("heading", { name: "Your agent" }),
    ).toBeInTheDocument();
    expect(within(summary()).getByText("PAPER")).toBeInTheDocument();
    expect(summary()).toHaveTextContent(
      "This agent may use $3,000.00 of simulated money, on paper.",
    );
    expect(summary()).toHaveTextContent("It trades only MSFT.");
    expect(
      summary().querySelector("[data-slot=mandate-version]")!.textContent,
    ).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(row("Money it may use")).toHaveTextContent("$3,000.00");
    expect(row("Money it may use").querySelector("[data-slot=tag]")).toBeNull();
    expect(
      row("Where it runs").querySelector("[data-slot=tag]"),
    ).toHaveTextContent("Default");
    const limits = sectionRows("limits");
    expect(limits.length).toBeGreaterThan(5);
    for (const r of limits)
      expect(r.querySelector("[data-slot=tag]")).toHaveTextContent("Proposed");
    expect(
      summary().querySelector("details, [aria-expanded=false]"),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: /confirm section|accept all/i }),
    ).toBeNull();
    expect(button("Create the version")).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Confirm version 1" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Deploy to paper" }),
    ).toBeNull();
    expect(step("confirm")).toBeNull();
    expect(step("deploy")).toBeNull();
    expect(setUps()).toEqual([]);
  });

  it("states the losses in dollars and what could buy without asking", async () => {
    renderFlow();
    await toSummary();
    expect(row("Most it may lose, in total")).toHaveTextContent("$300.00");
    expect(
      row("Fall at which it closes everything").querySelector(
        "[data-slot=tag]",
      ),
    ).toHaveTextContent("Proposed");
    expect(row("Could buy without asking you")).toHaveTextContent("$0.00");
    expect(summary()).toHaveTextContent(GAP_NOTE);
    expect(summary()).toHaveTextContent("It asks you before every buy.");
  });

  it("keeps a goal no limit can check as a note, marked not enforced", async () => {
    renderFlow();
    await toSummary({ goal: "Grow it steadily for my retirement" });
    expect(
      within(
        summary().querySelector<HTMLElement>("[data-slot=not-enforced]")!,
      ).getByText("“Grow it steadily for my retirement”"),
    ).toBeInTheDocument();
    expect(row("Goal")).toHaveTextContent("Runs until you stop it.");
  });

  it("takes a change by saying so, and shows the agent again, with only the new one to create", async () => {
    renderFlow();
    await toSummary();
    await send("Make it $2,000");
    expect(lastReply()).toEqual(["Got it: $2,000.00 to use.", READY]);
    expect(summaries()).toHaveLength(1);
    expect(log().querySelectorAll("[data-slot=replaced]")).toHaveLength(1);
    expect(row("Money it may use")).toHaveTextContent("$2,000.00");
    expect(
      screen.getAllByRole("button", { name: "Create the version" }),
    ).toHaveLength(1);
  });

  it("asks again for a loss a change no longer fits, and offers nothing to create until it has one", async () => {
    renderFlow();
    await toSummary();
    await send("Make it $1,000");
    expect(lastReply()[1]).toBe(
      "With $1,000.00, the loss you gave no longer fits. That is more than this workspace allows: at most 20% of the money, $200.00. Change your answer to continue.",
    );
    expect(lastReply()[2]).toMatch(
      /^How much could you stand to lose, in total\?/,
    );
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    await send("$100");
    expect(button("Create the version")).toBeEnabled();
  });

  it("draws every action as the app's one key, so no button shouts over another (DEC-469)", async () => {
    renderFlow();
    await toSummary();
    expect(
      main().querySelectorAll(
        "button[data-variant=primary], button[class*='kumo-button-emphasis']",
      ),
    ).toHaveLength(0);
    const actions = [
      ...main().querySelectorAll<HTMLButtonElement>(
        "button[data-kumo-component=Button], button[data-variant]",
      ),
    ];
    expect(actions.length).toBeGreaterThan(0);
    for (const b of actions)
      for (const c of KEY.split(" "))
        expect(b, b.textContent ?? "").toHaveClass(c);
  });
});

describe("when the model fails", () => {
  it("keeps the owner's words when the model does not answer, and tries again or reads figures only", async () => {
    let up = false;
    const flaky: Compiler = (input) =>
      up ? INSTANT(input) : Promise.reject(new Error("timeout"));
    renderFlow({ compiler: flaky });
    await send("$3,000");
    const failed = () =>
      [...log().querySelectorAll<HTMLElement>("[data-slot=failed]")].at(-1)!;
    expect(failed().querySelector("[data-reason]")).toHaveAttribute(
      "data-reason",
      "unreachable",
    );
    expect(failed()).toHaveTextContent(
      "The model didn't answer, so nothing was read. Your message is kept.",
    );
    expect(log().querySelector("[data-slot=owner-message]")).toHaveTextContent(
      "$3,000",
    );
    fireEvent.click(
      within(failed()).getByRole("button", { name: "Try again" }),
    );
    await settle();
    expect(log().querySelectorAll("[data-slot=failed]")).toHaveLength(2);

    fireEvent.click(
      within(failed()).getByRole("button", {
        name: "Continue without the model",
      }),
    );
    await settle();
    expect(lastReply()[0]).toBe("Got it: $3,000.00 to use.");
    expect(main().querySelector("[data-slot=model-note]")).toHaveTextContent(
      "Reading without the model now: figures only, exactly as written.",
    );
    expect(within(failed()).queryByRole("button")).toBeNull();
    up = true;
    await send("Grow it");
    await send("about ten percent");
    expect(lastReply()).toEqual([
      "I couldn't find a loss in that. Write it in dollars, or as a percentage of the money.",
    ]);
  });

  it("uses none of an answer outside the compiler's schema", async () => {
    renderFlow({
      compiler: async () => ({
        readings: [{ field: "money", quote: "$3,000", value: "3000" }],
        intent: "answer",
      }),
    });
    await send("$3,000");
    expect(log().querySelector("[data-reason]")).toHaveAttribute(
      "data-reason",
      "invalid",
    );
    expect(log().querySelector("[data-slot=failed]")).toHaveTextContent(
      "The model's answer didn't match the compiler's schema, so I used none of it. Your message is kept.",
    );
    expect(lastReply()).toEqual([INTRO]);
  });

  it("never lets the model set a field the owner did not state", async () => {
    renderFlow({
      compiler: async () => ({
        readings: [
          { field: "money", quote: "Use $3,000", value: "30000" },
          { field: "autonomy", quote: "without asking", value: "auto" },
          { field: "symbols", quote: "Use $3,000", symbols: ["NVDA"] },
        ],
        not_enforced: [],
        intent: "answer",
        reply: null,
      }),
    });
    await send("Use $3,000, buy without asking");
    expect(lastReply()).toEqual([
      "I couldn't find an amount in that. How much money can it use, in dollars?",
    ]);
  });
});

describe("setting it up: create the version, confirm it, deploy it (DEC-2401)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("creates the version first, and shows its hash, classification and validation only once it is recorded", async () => {
    renderFlow();
    await toSummary();
    const version = summary().querySelector(
      "[data-slot=mandate-version]",
    )!.textContent!;
    fireEvent.click(button("Create the version"));
    expect(setUps()).toHaveLength(1);
    expect(setUps()[0].version).toBe(version);
    expect(phase("version")).toBe("sent");
    expect(step("version")).toHaveTextContent(
      "Creating version 1… Sent at 14:05:20 ET. Nothing is recorded until the journal has it.",
    );
    expect(
      step("version")!.querySelector("[data-slot=created-version]"),
    ).toBeNull();
    expect(
      screen.getByRole("heading", { name: "1. Create the version" }),
    ).toHaveFocus();
    expect(step("confirm")).toBeNull();
    expect(composer()).toBeEnabled();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase("version")).toBe("recorded");
    expect(step("version")).toHaveTextContent(
      "Version 1 created at 14:05:20 ET. Its validation passed.",
    );
    expect(
      step("version")!.querySelector("[data-slot=created-version]")!
        .textContent,
    ).toBe(version);
    const classification = step("version")!.querySelector<HTMLElement>(
      "[data-slot=classification]",
    )!;
    expect(classification).toHaveAttribute(
      "data-classification",
      "risk_increasing",
    );
    expect(classification).toHaveTextContent(
      "A first version has no earlier version to compare with, so it counts as risk-increasing: confirming it needs your passkey.",
    );
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    expect(button("Confirm version 1")).toBeEnabled();
    expect(step("deploy")).toBeNull();
    expect(probed().ws.agents.map((a) => a.label)).toEqual([
      "Agent 1",
      "Agent 2",
      "Agent 3",
    ]);
  });

  it("confirms it with a passkey bound to the version, keeping every line it shows in the record", async () => {
    renderFlow();
    await toSummary();
    const version = summary().querySelector("[data-slot=mandate-version]")!
      .textContent! as SetUp["version"];
    const text = summary().textContent!.replace(/\s+/g, " ");
    fireEvent.click(button("Create the version"));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    fireEvent.click(button("Confirm version 1"));
    expect(stepUpDialog()).toHaveTextContent(
      "Confirm version 1 of this agent: $3,000.00 of simulated money on paper, trading MSFT.",
    );
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    const confirmed = setUps()[0].confirmed!;
    expect(confirmed.phase).toBe("sent");
    expect(confirmed.stepUp).toBe(confirmStepUpDigest(version, null, null));
    expect(confirmed.record.screen).toBe("A5");
    expect(confirmed.record.environment).toBe("paper");
    expect(confirmed.record.classification).toBe("risk_increasing");
    for (const line of confirmed.record.shown)
      for (const part of line.split(/: | \(|\)$/).filter(Boolean))
        expect(text, line).toContain(part.replace(/\s+/g, " "));
    expect(confirmed.record.shown).toContain(`Version 1: ${version}`);
    expect(confirmed.record.paths).toContain("/capital/allocation_usd");
    expect(confirmed.record.paths).toContain("/universe/pinned_instruments");
    expect(new Set(confirmed.record.paths).size).toBe(
      confirmed.record.paths.length,
    );
    for (const path of confirmed.record.paths)
      expect(path).toMatch(/^(\/([^/~]|~[01])*)+$/);
    expect(step("confirm")).toHaveTextContent(
      "Confirming version 1… Sent at 14:05:20 ET. Nothing is confirmed until the journal records it.",
    );
    expect(
      screen.getByRole("heading", { name: "2. Confirm it" }),
    ).toHaveFocus();
    expect(composer()).toBeDisabled();
    expect(step("deploy")).toBeNull();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase("confirm")).toBe("recorded");
    expect(step("confirm")).toHaveTextContent(
      "Confirmed by you at 14:05:20 ET, with your passkey. Version 1 is confirmed; no agent exists until you deploy it.",
    );
    expect(button("Deploy to paper")).toBeEnabled();
    expect(probed().ws.agents.map((a) => a.label)).toEqual([
      "Agent 1",
      "Agent 2",
      "Agent 3",
    ]);
  });

  it("deploys it to paper only, showing the version and that no backtest of it has run, and keeps that in the record", async () => {
    renderFlow();
    await toSummary();
    const version = summary().querySelector(
      "[data-slot=mandate-version]",
    )!.textContent!;
    createAndConfirm();
    const deploy = step("deploy")!;
    expect(
      deploy.querySelector("[data-slot=deploy-version]")!.textContent,
    ).toBe(version);
    expect(
      deploy.querySelector("[data-slot=backtest-shown]"),
    ).toHaveTextContent("None: no backtest of this version has run.");
    expect(
      within(deploy).queryByRole("link", { name: /backtest/i }),
    ).toBeNull();
    expect(deploy).toHaveTextContent(
      "Paper only: simulated funds. Going live is not available.",
    );
    expect(main()).not.toHaveTextContent(/go live/i);
    expect(screen.queryByRole("button", { name: /live/i })).toBeNull();
    fireEvent.click(button("Deploy to paper"));
    expect(stepUpDialog()).toHaveTextContent(
      "Deploy version 1 to paper with $3,000.00 of simulated money, trading MSFT.",
    );
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    const deployed = setUps()[0].deployed!;
    expect(deployed.record).toEqual({
      screen: "B3",
      environment: "paper",
      shown: deployLines(version as SetUp["version"], null),
      backtestId: null,
    });
    const text = deploy.textContent!.replace(/\s+/g, " ");
    for (const line of deployed.record.shown)
      for (const part of line.split(/: /).filter(Boolean))
        expect(text, line).toContain(part);
  });

  it("is sent, then recorded as a running agent with its owl, then shows the first request", async () => {
    renderFlow();
    await toSummary();
    setUpWithPasskeys();
    expect(phase("deploy")).toBe("sent");
    expect(step("deploy")).toHaveTextContent(
      "Deploying it to paper… Sent at 14:05:20 ET. Nothing is active until the runtime records it.",
    );
    expect(
      screen.queryByRole("button", { name: "Deploy to paper" }),
    ).toBeNull();
    expect(
      screen.getByRole("heading", { name: "3. Deploy to paper" }),
    ).toHaveFocus();
    expect(composer()).toBeDisabled();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const [s] = setUps();
    const done = step("deploy")!;
    expect(phase("deploy")).toBe("recorded");
    expect(done).toHaveTextContent("Agent 4 is running on paper.");
    expect(done).toHaveTextContent(
      "Recorded in the journal at 14:05:20 ET, as version 1.",
    );
    expect(done.querySelector("[data-slot=owl]")).toHaveAttribute(
      "data-mood",
      "awake",
    );
    expect(
      done.querySelector("[data-slot=hatch] [data-slot=egg]"),
      "the owl hatches once, answering the owner's act (DEC-514)",
    ).not.toBeNull();
    expect(
      within(done).getByRole("link", { name: "Open Agent 4" }),
    ).toHaveAttribute("href", `/agents/${s.agentId}`);

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(done.querySelector("[data-slot=first-ask]")).toHaveTextContent(
      "Its first check asks you: buy 6 MSFT at $44.62. If you do not answer by 14:10:20 ET, it is skipped.",
    );
    expect(
      within(done)
        .getByRole("link", { name: "Review the request" })
        .getAttribute("href"),
    ).toMatch(/^\/approvals\/apr_[0-9A-HJKMNP-TV-Z]{26}$/);
  });

  it("asks for the passkey at each step that needs one; a cancel or a failure sends nothing", async () => {
    renderFlow({ passkey: failingPasskey });
    await toSummary();
    fireEvent.click(button("Create the version"));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    fireEvent.click(button("Confirm version 1"));
    press("Cancel");
    expect(main()).toHaveTextContent(
      "Passkey check canceled. Nothing was confirmed or sent.",
    );
    withPasskey("Confirm version 1");
    expect(main()).toHaveTextContent(
      "Passkey check failed. Nothing was confirmed or sent.",
    );
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 4));
    expect(
      setUps().map((s) => [s.created.phase, s.confirmed, s.deployed]),
    ).toEqual([["recorded", null, null]]);
    expect(step("confirm")!.hasAttribute("data-phase")).toBe(false);
    expect(step("deploy")).toBeNull();
    expect(button("Confirm version 1")).toBeEnabled();
  });

  it("sends nothing to deploy when its passkey check is canceled", async () => {
    renderFlow();
    await toSummary();
    createAndConfirm();
    fireEvent.click(button("Deploy to paper"));
    press("Cancel");
    expect(main()).toHaveTextContent(
      "Passkey check canceled. Nothing was deployed or sent.",
    );
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(setUps()[0].deployed).toBeNull();
    expect(probed().ws.agents.map((a) => a.label)).toEqual([
      "Agent 1",
      "Agent 2",
      "Agent 3",
    ]);
  });

  /** Another agent takes $1,000 of the account's free money through its own three steps, its deployment left sent. */
  function anotherAgentTakesMoney() {
    const strategy = readStrategy({
      model: "quant.momentum",
      params: { lookback_bars: "20" },
    });
    const other = readAnswers("$1,000", "Grow it", "$100");
    if (!strategy.ok || !other.ok) throw new Error("fixture answers");
    setUpThrough(
      mandateFrom(
        compile(other.value, ["AAPL"], strategy.value),
        probed().ws.connection.connection_id,
      )!,
    );
  }

  it("will not create the version when the account changed after the money was said (V-002 again), and takes a change", async () => {
    renderFlow();
    await toSummary();
    anotherAgentTakesMoney();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(main().querySelector("[data-slot=blocked]")).toHaveTextContent(
      "Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent would use. Write a smaller amount.",
    );
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    await send("Make it $2,000");
    expect(row("Money it may use")).toHaveTextContent("$2,000.00");
    expect(button("Create the version")).toBeEnabled();
  });

  it("says what was and was not recorded when the runtime refuses the deployment after the passkey, and takes a change", async () => {
    renderFlow();
    await toSummary();
    createAndConfirm();
    anotherAgentTakesMoney();
    withPasskey("Deploy to paper");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(setUps().map((s) => s.deployed?.phase)).toEqual([
      "rejected",
      "recorded",
    ]);
    const rejected = step("deploy")!;
    expect(rejected).toHaveAttribute("data-phase", "rejected");
    expect(rejected).toHaveTextContent(
      "Not deployed. Version 1 was created and confirmed, and no agent exists.",
    );
    expect(rejected).toHaveTextContent(
      "Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent asks for.",
    );
    expect(rejected).toHaveTextContent("Tell me what to change.");
    expect(probed().ws.agents.map((a) => a.label)).toEqual([
      "Agent 1",
      "Agent 2",
      "Agent 3",
      "Agent 4",
    ]);
    expect(composer()).toBeEnabled();
    await send("Make it $2,000");
    expect(log().querySelector("[data-slot=replaced]")).toHaveTextContent(
      "An earlier version of your agent, replaced after you changed it. Its version 1 was created and confirmed, and not deployed; no agent exists for it.",
    );
    expect(button("Create the version")).toBeEnabled();
  });

  it("says a created version was not confirmed when a change replaces it", async () => {
    renderFlow();
    await toSummary();
    fireEvent.click(button("Create the version"));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    await send("Make it $2,000");
    expect(log().querySelector("[data-slot=replaced]")).toHaveTextContent(
      "An earlier version of your agent, replaced after you changed it. Its version 1 was created and not confirmed; no agent exists for it.",
    );
    expect(summaries()).toHaveLength(1);
    expect(button("Create the version")).toBeEnabled();
    expect(step("confirm")).toBeNull();
  });

  it("says the result is unknown, never to try again, when the journal does not answer", async () => {
    renderFlow({ scenario: "result-unknown" });
    await toSummary();
    fireEvent.click(button("Create the version"));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(phase("version")).toBe("unknown");
    expect(step("version")).toHaveTextContent(
      "The result is unknown; we are checking. Whether version 1 was created is not known yet; nothing was confirmed, and no agent exists.",
    );
    expect(main()).not.toHaveTextContent(/try again/i);
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    expect(step("confirm")).toBeNull();
  });

  it("says nothing was recorded, and offers the step again, when the deployment does not answer", async () => {
    renderFlow({ scenario: "unreachable" });
    await toSummary();
    fireEvent.click(button("Create the version"));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase("version")).toBe("undelivered");
    expect(step("version")).toHaveTextContent(
      "Not delivered: your deployment did not answer. No version was created, nothing was confirmed, and no agent exists. Try again once it is reachable.",
    );
    expect(button("Create the version")).toBeEnabled();
    expect(step("confirm")).toBeNull();
  });

  it("will not let the next agent claim an instrument the new agent trades", async () => {
    renderFlow();
    await toSummary();
    setUpWithPasskeys();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    fireEvent.click(button("Set up another agent"));
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(heading()).toHaveFocus();
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(0);
    await send("$400");
    await send("Grow it");
    await send("$40");
    await send("MSFT");
    expect(lastReply()[0]).toBe(
      "MSFT is already traded by Agent 4. One agent trades an instrument on an account; choose another.",
    );
  });

  it("is the owner's alone to set up", async () => {
    renderFlow({ role: "operator" });
    await toSummary();
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    expect(main()).toHaveTextContent(
      "Only the workspace owner can create an agent.",
    );
  });
});

describe("each step's words, over either data source", () => {
  const V = `sha256:${"1f".repeat(32)}` as const;
  const AT = "2026-09-28T14:05:20-04:00";
  const recorded = { phase: "recorded" as const, sentAt: AT, recordedAt: AT };
  const confirmedSetUp = (): SetUp => ({
    id: "set-1",
    agentId: "",
    version: V,
    created: {
      ...recorded,
      base: null,
      classification: "risk_increasing",
      validated: true,
    },
    confirmed: {
      ...recorded,
      record: {
        screen: "A5",
        environment: "paper",
        shown: [],
        paths: [],
        classification: "risk_increasing",
      },
      stepUp: confirmStepUpDigest(V, null, null),
    },
    deployed: null,
  });
  const words = (which: "version" | "confirm" | "deploy", setUp: SetUp) => {
    const view = renderWithRuntime(<StepStatus which={which} setUp={setUp} />);
    const text = view.container.textContent!;
    view.unmount();
    return text;
  };

  it("says a step the API does not take from here yet was not sent, and what exists", () => {
    const base = confirmedSetUp();
    const notSent = {
      phase: "undelivered" as const,
      sentAt: AT,
      reason: "not_sent",
    };
    expect(
      words("version", {
        ...base,
        created: {
          ...notSent,
          base: null,
          classification: null,
          validated: false,
        },
        confirmed: null,
      }),
    ).toBe(
      "Not sent: your deployment does not take this step from here yet. No version was created, nothing was confirmed, and no agent exists.",
    );
    expect(
      words("deploy", {
        ...base,
        deployed: {
          ...notSent,
          record: {
            screen: "B3",
            environment: "paper",
            shown: [],
            backtestId: null,
          },
        },
      }),
    ).toBe(
      "Not sent: your deployment does not take this step from here yet. Version 1 was created and confirmed, and no agent exists.",
    );
  });

  it("says a confirm the deployment refused was not recorded, and why, without a code", () => {
    const base = confirmedSetUp();
    const refused = {
      ...base.confirmed!,
      phase: "undelivered" as const,
      recordedAt: undefined,
      reason: "step_up_required",
    };
    expect(words("confirm", { ...base, confirmed: refused })).toBe(
      "Not recorded: your deployment asked for a passkey check that was not sent. Version 1 was created and is not confirmed, and no agent exists.",
    );
    const other = words("confirm", {
      ...base,
      confirmed: { ...refused, reason: "stale_base" },
    });
    expect(other).toBe(
      "Not recorded: your deployment refused it. Version 1 was created and is not confirmed, and no agent exists.",
    );
    expect(other).not.toContain("stale_base");
  });

  it("says a step refused because the one before it is not recorded stands where that step left things", () => {
    const base = confirmedSetUp();
    const notCreated = {
      ...base.confirmed!,
      phase: "undelivered" as const,
      recordedAt: undefined,
      reason: "not_created",
      stepUp: null,
    };
    expect(
      words("confirm", {
        ...base,
        created: {
          ...base.created,
          phase: "undelivered",
          reason: "not_sent",
          classification: null,
          validated: false,
        },
        confirmed: notCreated,
      }),
    ).toBe(
      "Not sent: version 1 was not created, so there is nothing to confirm. No version was created, nothing was confirmed, and no agent exists.",
    );
    const notConfirmed = {
      phase: "undelivered" as const,
      sentAt: base.created.sentAt,
      reason: "not_confirmed",
      record: {
        screen: "B3" as const,
        environment: "paper" as const,
        shown: [],
        backtestId: null,
      },
    };
    expect(words("deploy", { ...base, deployed: notConfirmed })).toBe(
      "Not sent: version 1 is not confirmed, so there is nothing to deploy. Version 1 was created and is not confirmed, and no agent exists.",
    );
  });

  it("never says to try again when a result is unknown", () => {
    const base = confirmedSetUp();
    const unknown = { phase: "unknown" as const, sentAt: AT };
    expect(
      words("confirm", {
        ...base,
        confirmed: { ...base.confirmed!, ...unknown, recordedAt: undefined },
      }),
    ).toBe(
      "The result is unknown; we are checking. Version 1 was created; whether it is confirmed is not known yet, and no agent exists.",
    );
    const deploy = words("deploy", {
      ...base,
      deployed: {
        ...unknown,
        record: {
          screen: "B3",
          environment: "paper",
          shown: [],
          backtestId: null,
        },
      },
    });
    expect(deploy).toBe(
      "The result is unknown; we are checking. Version 1 was created and confirmed; whether the agent exists is not known yet. Look in All agents before deploying it again, so the same agent is not created twice.",
    );
    expect(deploy).not.toMatch(/try again/i);
  });
});

describe("the deploy step's backtest (B3)", () => {
  it("links the version's backtest by its id, which the record keeps", () => {
    renderWithRuntime(
      <DeployDetails
        version={VERSION.lmn}
        backtestId={BACKTEST_IDS.lmnCompleted}
      />,
    );
    const shown = document.querySelector<HTMLElement>(
      "[data-slot=backtest-shown]",
    )!;
    expect(
      within(shown).getByRole("link", {
        name: `Backtest ${BACKTEST_IDS.lmnCompleted}`,
      }),
    ).toHaveAttribute("href", `/backtests/${BACKTEST_IDS.lmnCompleted}`);
    expect(
      document.querySelector("[data-slot=deploy-version]")!.textContent,
    ).toBe(VERSION.lmn);
    expect(deployLines(VERSION.lmn, BACKTEST_IDS.lmnCompleted)).toContain(
      `Backtest shown: ${BACKTEST_IDS.lmnCompleted}`,
    );
    expect(deployLines(VERSION.lmn, null)).toContain(
      "Backtest shown: None: no backtest of this version has run.",
    );
    expect(deployLines(VERSION.lmn, null)).toContain(
      `Version 1: ${VERSION.lmn}`,
    );
  });
});

describe("creating it where the workspace requires independent approval (§4.3, V-047; interim)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const policy =
    (value: boolean | null | "absent", approverUsers?: number) =>
    (ws: Workspace): Workspace => {
      const next: Workspace = {
        ...ws,
        approver_users: approverUsers ?? ws.approver_users,
        independent_approval_required: value === "absent" ? null : value,
      };
      if (value === "absent")
        delete (next as Partial<Workspace>).independent_approval_required;
      return next;
    };
  const SECOND_PERSON =
    "This workspace needs a second person to approve a new agent, and that approval can't be asked for here yet, so a passkey alone can't create it.";

  it.each([
    [true, 2],
    [true, 1],
    [null, 2],
    ["absent", 2],
  ] as const)(
    "offers no step when the policy is %s with %s approvers, says why, and creates nothing",
    async (value, approvers) => {
      renderFlow({ workspace: policy(value, approvers) });
      await toSummary();
      expect(summary()).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Create the version" }),
      ).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Confirm version 1" }),
      ).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Deploy to paper" }),
      ).toBeNull();
      expect(main()).not.toHaveTextContent("Uses your passkey.");
      const refusal = main().querySelector<HTMLElement>("[data-slot=blocked]")!;
      expect(refusal).toHaveTextContent(SECOND_PERSON);
      expect(refusal).toHaveAttribute("data-rule", "V-047");
      expect(main()).not.toHaveTextContent("V-047");

      act(() =>
        vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3),
      );
      expect(stepUpDialog()).toBeNull();
      expect(setUps()).toEqual([]);
      expect(probed().ws.agents.map((a) => a.label)).toEqual([
        "Agent 1",
        "Agent 2",
        "Agent 3",
      ]);
      expect(main().querySelector("[data-step] [data-phase]")).toBeNull();
    },
  );

  const NOT_HERE =
    "That's everything. Here is your agent. It can't be created here without a second person's approval.";

  it.each([true, null, "absent"] as const)(
    "ends the chat with a line that does not invite a create when the policy is %s",
    async (value) => {
      renderFlow({ workspace: policy(value) });
      await toSummary();
      expect(lastReply()).toEqual(["Got it: lookback of 20.", NOT_HERE]);
      await send("Make it $2,000");
      expect(lastReply()).toEqual(["Got it: $2,000.00 to use.", NOT_HERE]);
      for (const line of replies().flatMap((r) =>
        [...r.querySelectorAll("p")].map((p) => p.textContent!),
      )) {
        expect(line).not.toMatch(/Create it/);
        expect(line).not.toMatch(/V-047/);
      }
    },
  );

  it("ends the chat inviting a create exactly as before when the policy is off", async () => {
    renderFlow({ workspace: policy(false) });
    await toSummary();
    expect(lastReply()).toEqual(["Got it: lookback of 20.", READY]);
    expect(READY).toBe(
      "That's everything. Here is your agent. Create it, or tell me what to change.",
    );
    await send("Make it $2,000");
    expect(lastReply()).toEqual(["Got it: $2,000.00 to use.", READY]);
  });

  it("still refuses after a change is said, since no draft can satisfy it", async () => {
    renderFlow({ workspace: policy(true) });
    await toSummary();
    await send("Make it $2,000");
    expect(row("Money it may use")).toHaveTextContent("$2,000.00");
    expect(
      screen.queryByRole("button", { name: "Create the version" }),
    ).toBeNull();
    expect(main().querySelector("[data-slot=blocked]")).toHaveAttribute(
      "data-rule",
      "V-047",
    );
  });

  it("takes the three steps and creates the agent exactly as before when the policy is off, even with one approver", async () => {
    renderFlow({ workspace: policy(false, 1) });
    await toSummary();
    expect(main().querySelector("[data-slot=blocked]")).toBeNull();
    setUpWithPasskeys();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(
      setUps().map((s) => [
        s.created.phase,
        s.confirmed?.phase,
        s.deployed?.phase,
      ]),
    ).toEqual([["recorded", "recorded", "recorded"]]);
    expect(probed().ws.agents.at(-1)!.label).toBe("Agent 4");
  });

  it("shows the rule only as an attribute when the runtime refuses a deployment under the policy", async () => {
    renderFlow({ workspace: policy(false) });
    await toSummary();
    createAndConfirm();
    fireEvent.click(button("Deploy to paper"));
    press("Use passkey");
    act(() => {
      // Deliberate direct mutation: the policy changes between the owner's click and the runtime applying it.
      probed().ws.independent_approval_required = null;
      vi.advanceTimersByTime(PASSKEY_ANSWER_MS);
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const rejected = main().querySelector<HTMLElement>(
      "[data-step=deploy][data-phase=rejected]",
    )!;
    expect(rejected).toHaveTextContent(
      "Not deployed. Version 1 was created and confirmed, and no agent exists.",
    );
    expect(within(rejected).getByText(SECOND_PERSON)).toHaveAttribute(
      "data-rule",
      "V-047",
    );
    expect(rejected).not.toHaveTextContent("V-047");
    expect(probed().ws.agents.map((a) => a.label)).toEqual([
      "Agent 1",
      "Agent 2",
      "Agent 3",
    ]);
  });
});

describe("nothing leaves the page", () => {
  it("fetches nothing and stores nothing through the whole conversation and set-up, and reaches the runtime only from the steps", async () => {
    vi.useFakeTimers();
    const cookie = document.cookie;
    renderFlow();
    await toSummary();
    expect(setUps()).toEqual([]);
    setUpWithPasskeys();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(setUps()).toHaveLength(1);
    expect(setUps()[0].deployed?.phase).toBe("recorded");
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
    expect(getItem).not.toHaveBeenCalled();
    expect(document.cookie).toBe(cookie);
    vi.useRealTimers();
  });
});

describe("the fixture compiler's draft", () => {
  const readQuestions = (money: string, goal: string, loss: string): Read => {
    const r = readAnswers(money, goal, loss);
    if (!r.ok) throw new Error(r.error);
    return r.value;
  };

  it("drafts the same mandate, and the same version, from the same words", () => {
    const strategy = readStrategy({
      model: "quant.momentum",
      params: { lookback_bars: "20" },
    });
    if (!strategy.ok) throw new Error(strategy.error);
    const connection = buildWorkspace("normal").connection.connection_id;
    const version = (loss: string) =>
      mandateVersion(
        mandateFrom(
          compile(
            readQuestions("$5,000", "up 10%", loss),
            ["MSFT"],
            strategy.value,
          ),
          connection,
        )!.mandate,
      );
    expect(version("$500")).toBe(version("$500"));
    expect(version("$400")).not.toBe(version("$500"));
  });

  it("drafts no mandate without a model or an instrument", () => {
    const connection = buildWorkspace("normal").connection.connection_id;
    expect(
      mandateFrom(
        compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]),
        connection,
      ),
    ).toBeNull();
    const strategy = readStrategy({
      model: "quant.momentum",
      params: { lookback_bars: "20" },
    });
    if (!strategy.ok) throw new Error(strategy.error);
    expect(
      mandateFrom(
        compile(readQuestions("$5,000", "up 10%", "$500"), [], strategy.value),
        connection,
      ),
    ).toBeNull();
  });

  it("keeps the record's lines in step with the draft", () => {
    const strategy = readStrategy({
      model: "quant.momentum",
      params: { lookback_bars: "20" },
    });
    if (!strategy.ok) throw new Error(strategy.error);
    const lines = summaryLines(
      compile(
        readQuestions("$5,000", "up 10%", "$500"),
        ["MSFT"],
        strategy.value,
      ),
      "sha256:abc",
    );
    expect(lines.slice(0, 2)).toEqual(["Your agent", "PAPER: simulated funds"]);
    expect(lines.at(-1)).toBe("Version 1: sha256:abc");
    expect(lines).toContain("Money it may use: $5,000.00");
    expect(lines).toContain(
      "Where it runs: Paper: simulated funds, no real money. (Default)",
    );
    expect(lines).toContain("Model: Momentum (quant.momentum 1.0.0)");
    expect(lines).toContain("Could buy without asking you: $0.00");
  });

  it("confirms only the paths that hold a value: a row with no provenance is not a confirmed path", () => {
    const strategy = readStrategy({
      model: "quant.momentum",
      params: { lookback_bars: "20" },
    });
    if (!strategy.ok) throw new Error(strategy.error);
    const empty = compile(readQuestions("$5,000", "up 10%", "$500"), []);
    const unset = empty.sections
      .flatMap((s) => s.fields)
      .filter((f) => f.provenance === null)
      .map((f) => f.path);
    expect(unset).toEqual(
      expect.arrayContaining([
        "/behavior/signal_models/0/id",
        "/behavior/signal_models/0/params",
        "/universe/pinned_instruments",
      ]),
    );
    for (const path of unset)
      expect(confirmedPaths(empty), path).not.toContain(path);
    const full = confirmedPaths(
      compile(
        readQuestions("$5,000", "up 10%", "$500"),
        ["MSFT"],
        strategy.value,
      ),
    );
    expect(full).toEqual(
      expect.arrayContaining([
        "/behavior/signal_models/0/id",
        "/behavior/signal_models/0/params",
        "/universe/pinned_instruments",
        "/capital/allocation_usd",
      ]),
    );
    expect(new Set(full).size).toBe(full.length);
  });

  it("keeps one full position's loss at its stop inside the daily budget, so W-002 cannot fire", () => {
    for (const [money, loss] of [
      ["$5,000", "$500"],
      ["$1,000.02", "8%"],
      ["$3,000", "$500"],
      ["$250", "1%"],
      ["$12,345.67", "20%"],
      ["7k", "$33.33"],
    ]) {
      const d = compile(readQuestions(money, "Grow it", loss), []);
      expect(
        d.figures.positionLossAtStop <= d.figures.dailyLossBudget,
        `${money} ${loss}`,
      ).toBe(true);
      expect(d.terms.maxOrderUsd <= d.terms.maxPositionUsd).toBe(true);
      expect(d.figures.flattenLoss <= d.figures.floorLoss).toBe(true);
    }
  });

  it("counts no unasked dollars without an auto path, and bounds them by orders and exposure with one", () => {
    const d = compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]);
    expect(d.figures.unasked).toBe(0n);
    expect(d.terms.autonomyDefault).toBe("ask");
    expect(d.terms.autonomyRules).toEqual([]);
    const withAuto = {
      ...d.terms,
      autonomyRules: [{ id: "small-buys", then: "auto" as const }],
    };
    expect(unaskedUsd(withAuto)).toBe(mul(fromInt(10), dec("500")));
    expect(unaskedUsd({ ...withAuto, maxGrossExposureUsd: dec("1200") })).toBe(
      dec("1200"),
    );
  });

  it("reads a loss in dollars or as a percentage, and refuses one above the ceiling or the money", () => {
    const usd = readLoss("$500", dec("5000"));
    const pct = readLoss("10%", dec("5000"));
    expect(
      usd.ok &&
        pct.ok &&
        usd.value.usd === pct.value.usd &&
        usd.value.fraction === pct.value.fraction,
    ).toBe(true);
    expect(readLoss("21%", dec("5000"))).toMatchObject({ ok: false });
    expect(readLoss("$6,000", dec("5000"))).toEqual({
      ok: false,
      error: "That is more than the $5,000.00 this agent may use.",
    });
  });

  it("never proposes the instrument list or any auto, and defaults the environment to paper", () => {
    const d = compile(readQuestions("$5,000", "up 10%", "$500"), []);
    const fields = d.sections.flatMap((s) => s.fields);
    expect(
      fields.find((f) => f.path === "/universe/pinned_instruments")?.provenance,
    ).toBeNull();
    expect(fields.find((f) => f.path === "/environment")).toMatchObject({
      provenance: "platform_default",
      value: "Paper: simulated funds, no real money.",
    });
    expect(
      fields.filter((f) => /auto\b|without asking/i.test(f.value)),
    ).toEqual([]);
  });
});
