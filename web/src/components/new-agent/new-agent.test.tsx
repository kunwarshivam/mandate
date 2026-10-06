import { act, fireEvent, screen, within } from "@testing-library/react";
import { type MockInstance, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { KEY } from "@/components/kumo/key";
import { PASSKEY_ANSWER_MS, type Passkey } from "@/components/stop/step-up-dialog";
import { buildWorkspace } from "@/fixtures/workspace";
import { dec, fromInt, mul } from "@/lib/decimal";
import { mandateVersion } from "@/lib/fixture-journey";
import type { Deployment } from "@/lib/mock-runtime";
import type { Role } from "@/lib/roles";
import { RECORD_AFTER_MS, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { RuntimeProbe, probed } from "@/test/runtime-probe";
import { failingPasskey, press, stepUpDialog } from "@/test/step-up";
import { ADVICE_REPLY, type Compiler, type CompilerInput, fixtureCompiler } from "./compiler";
import { confirmationLines } from "./confirmation";
import { type Read, compile, mandateFrom, readAnswers, readLoss, readStrategy, unaskedUsd } from "./draft";
import { NewAgentFlow } from "./new-agent-flow";

const main = () => screen.getByRole("main");
const heading = () => screen.getByRole("heading", { level: 1 });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const composer = () => screen.getByRole("textbox", { name: "Your message" });
const log = () => screen.getByRole("log", { name: "Conversation" });
const thinking = () => main().querySelector<HTMLElement>("[data-slot=thinking]")!;

const deployments = (): Deployment[] => probed().deployments;

const INSTANT = fixtureCompiler({ latencyMs: 0 });

function renderFlow({ role = "owner", passkey, compiler = INSTANT }: { role?: Role; passkey?: Passkey; compiler?: Compiler } = {}) {
  setPathname("/agents/new");
  return renderWithRuntime(
    <main>
      <NewAgentFlow compiler={compiler} />
      <RuntimeProbe />
    </main>,
    "normal",
    { role, ...(passkey ? { passkey } : {}) },
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

const asks = () => [...log().querySelectorAll<HTMLElement>("[data-slot=ask]")];
const lastAsk = () => asks().at(-1)!;
const lastOf = (slot: string) => [...log().querySelectorAll<HTMLElement>(`[data-slot=${slot}]`)].at(-1);
const section = (key: string) => [...main().querySelectorAll<HTMLElement>(`[data-slot=review-section][data-section=${key}]`)].at(-1)!;
const fieldAt = (path: string) => [...main().querySelectorAll<HTMLElement>(`[data-slot=draft-field][data-path="${path}"]`)].at(-1)!;
const confirmButton = () => screen.getByRole("button", { name: /^Confirm section/ });

function chooseModel(name: RegExp) {
  fireEvent.click(within(screen.getByRole("group", { name: "Models" })).getByRole("button", { name }));
}

/** The fixture account has $3,478.36 that no agent uses, so a deployment asks for less. */
async function toSections({ money = "$3,000", goal = "Grow it steadily", loss = "$300", symbols = "MSFT" } = {}) {
  await send(money);
  await send(goal);
  await send(loss);
  await send(symbols);
  chooseModel(/^Momentum/);
  await send("20");
}

const SECTIONS = ["money", "limits", "strategy", "autonomy", "universe"];

function confirmAll() {
  for (const key of SECTIONS) {
    expect(confirmButton()).toHaveAccessibleName(expect.stringContaining(section(key).querySelector("h2")!.textContent!));
    fireEvent.click(confirmButton());
  }
}

/** From the conversation to A5, every section confirmed. */
async function toConfirm(answers?: Parameters<typeof toSections>[0]) {
  await toSections(answers);
  confirmAll();
  fireEvent.click(button("Review and confirm"));
}

function deployWithPasskey() {
  fireEvent.click(button("Confirm and deploy to paper"));
  press("Use passkey");
  act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
}

let fetchSpy: MockInstance;
let setItem: MockInstance;
let getItem: MockInstance;

beforeEach(() => {
  fetchSpy = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("no network in the prototype"));
  setItem = vi.spyOn(Storage.prototype, "setItem");
  getItem = vi.spyOn(Storage.prototype, "getItem");
});

afterEach(() => vi.restoreAllMocks());

const EXAMPLES = /\$\s?\d|\d\s?%|e\.g\.|for example/i;

describe("A0 and A1, one conversation", () => {
  it("opens with what it does and the first question, with an empty composer and no suggested amounts", () => {
    renderFlow();
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(log().querySelector("[data-slot=intro]")).toHaveTextContent("Say it all at once, or answer one question at a time.");
    expect(lastAsk()).toHaveAttribute("data-step", "money");
    expect(lastAsk()).toHaveTextContent("How much money may this agent use?");
    expect(composer()).toHaveValue("");
    expect(composer()).not.toHaveAttribute("placeholder");
    expect(main().textContent).not.toMatch(EXAMPLES);
    expect(main().querySelectorAll("input, textarea")).toHaveLength(1);
    expect(button("Send")).toBeDisabled();
  });

  it("asks one question at a time, quoting back what it read, with no example amounts", async () => {
    renderFlow();
    await send("$3,000");
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
    expect(lastOf("noted")).toHaveTextContent("Money it may use$3,000.00");
    expect(lastOf("noted")).toHaveTextContent("You said “$3,000”");
    expect(lastAsk()).toHaveTextContent("What is it for?");
    expect(composer()).toHaveValue("");
    expect(composer()).toHaveFocus();

    await send("Grow it steadily");
    expect(lastAsk()).toHaveTextContent("How much could you stand to lose, in total?");
    await send("$300");
    expect(lastOf("noted")).toHaveTextContent("$300.00, 10% of the money");
    expect(lastAsk()).toHaveTextContent("Which stocks or ETFs may it trade?");
    for (const ask of asks()) expect(ask.textContent!.replace(/This workspace's limit.*$/, "")).not.toMatch(EXAMPLES);
  });

  it("reads everything said at once, and asks only what is still missing", async () => {
    renderFlow();
    await send("Use $3,000 on big tech. Stop when it is up 10%. I can stand to lose 10%. Avoid oil companies.");
    const noted = lastOf("noted")!;
    expect([...noted.querySelectorAll("[data-slot=noted-item] dt")].map((d) => d.textContent)).toEqual(["Money it may use", "Most it may lose, in total", "Goal"]);
    expect(noted).toHaveTextContent("You said “Use $3,000 on big tech.”");
    expect(noted).toHaveTextContent("You said “I can stand to lose 10%.”");
    expect(lastAsk()).toHaveAttribute("data-step", "symbols");
    await send("MSFT");
    chooseModel(/^Momentum/);
    await send("20");
    expect(fieldAt("/goal/profit_level")).toHaveTextContent("Stops when its equity reaches $3,300.00");
    expect(within(screen.getByRole("region", { name: "Not enforced" })).getByText("“Avoid oil companies.”")).toBeInTheDocument();
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
    expect(thinking()).toHaveTextContent("Reading your words…");
    expect(thinking()).toHaveAttribute("role", "status");
    fireEvent.change(composer(), { target: { value: "more" } });
    expect(button("Send")).toBeDisabled();
    expect(seen).toEqual([{ messages: [{ id: expect.any(String), text: "$3,000" }], asked: "money", models: [expect.objectContaining({ name: "Mean reversion" }), expect.objectContaining({ name: "Momentum" })] }]);

    const raw = await INSTANT(seen[0]);
    await act(async () => answer(raw));
    await settle();
    expect(thinking()).toHaveTextContent("");
    expect(lastAsk()).toHaveAttribute("data-step", "goal");
  });

  it("says what it could not find instead of guessing, and asks again", async () => {
    renderFlow();
    await send("Something calm for the long run");
    expect(lastOf("unread")).toHaveTextContent("We could not find an amount in that.");
    expect(lastAsk()).toHaveAttribute("data-step", "money");
    expect(log().querySelector("[data-slot=noted]")).toBeNull();
  });

  it("states the workspace's loss ceiling and refuses a loss above it, never moving the answer to fit", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it steadily");
    expect(lastAsk()).toHaveTextContent("This workspace's limit: at most 20% of the money, $600.00.");
    await send("$2,000");
    expect(lastOf("refused")).toHaveTextContent("That is more than this workspace allows: at most 20% of the money, $600.00.");
    expect(lastAsk()).toHaveAttribute("data-step", "loss");
    expect(log().querySelectorAll("[data-slot=noted]")).toHaveLength(2);
  });

  it("refuses more money than the account has free when it is said (V-002), and says how much is free", async () => {
    renderFlow();
    await send("$5,000");
    expect(lastOf("refused")).toHaveTextContent("Your paper account has $3,478.36 that no agent uses, less than the $5,000.00 this agent would use. Write a smaller amount.");
    expect(lastAsk()).toHaveAttribute("data-step", "money");
  });

  it("refuses a symbol another agent already trades on the account (V-006), and takes another", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("XYZ");
    expect(lastOf("refused")).toHaveTextContent("XYZ is already traded by Agent 2. One agent trades an instrument on an account; choose another.");
    expect(lastAsk()).toHaveAttribute("data-step", "symbols");
    await send("msft, aapl");
    expect(lastOf("noted")).toHaveTextContent("What it may tradeAAPL, MSFT");
    expect(lastAsk()).toHaveAttribute("data-step", "model");
  });

  it("asks before keeping an amount written in words, and drops it on a no", async () => {
    renderFlow();
    await send("about three grand");
    const check = lastAsk();
    expect(check).toHaveAttribute("data-step", "check");
    expect(check).toHaveTextContent("We read “about three grand” as $3,000.00, the money it may use. Is that right?");
    expect(log().querySelector("[data-slot=noted]")).toBeNull();
    fireEvent.click(within(check).getByRole("button", { name: "No" }));
    expect(lastOf("unread")).toHaveTextContent("Then write it in figures, and it is read exactly as written.");
    expect(lastAsk()).toHaveAttribute("data-step", "money");

    await send("three grand");
    fireEvent.click(within(lastAsk()).getByRole("button", { name: "Yes, $3,000.00" }));
    expect(lastOf("noted")).toHaveTextContent("You said “three grand”");
    expect(lastAsk()).toHaveAttribute("data-step", "goal");
    expect(within(asks().at(-2)!).queryByRole("button")).toBeNull();
  });
});

describe("the strategy, the owner's choice", () => {
  it("lists the models in alphabetical order, none chosen, and no word ranks them (brief A3)", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("MSFT");
    const choices = within(screen.getByRole("group", { name: "Models" })).getAllByRole("button");
    expect(choices.map((c) => c.querySelector("span")!.textContent)).toEqual(["Mean reversion", "Momentum"]);
    for (const c of choices) {
      expect(c).not.toHaveAttribute("aria-pressed");
      expect(c).not.toHaveFocus();
    }
    expect(lastAsk().textContent).not.toMatch(/recommended|popular|best|return|sharpe|win rate/i);
  });

  it("takes a model by name, asks each setting with no value filled in, and refuses one out of range", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("MSFT");
    await send("mean reversion");
    expect(lastOf("noted")).toHaveTextContent("How it decidesMean reversion (quant.mean_reversion 1.0.0)");
    expect(lastAsk()).toHaveTextContent("How many bars should the model read back over?");
    await send("20");
    expect(lastAsk()).toHaveTextContent("How far below its average, in standard deviations, must a price be before the model scores a buy?");
    await send("9");
    expect(lastOf("refused")).toHaveTextContent("Entry z-score: a number from 0.5 to 4, with at most two decimals.");
    await send("1.5");
    expect(lastOf("noted")).toHaveTextContent("Entry z-score1.5");
    expect(section("money")).toHaveAttribute("data-confirmed", "false");
    fireEvent.click(confirmButton());
    fireEvent.click(confirmButton());
    expect(fieldAt("/behavior/signal_models/0/params")).toHaveTextContent("Lookback, in bars: 20; Entry z-score: 1.5");
    expect(within(fieldAt("/behavior/signal_models/0/params")).getByText("You entered")).toBeInTheDocument();
  });

  it("explains a model from its methodology and refuses to choose for the owner, as quoted text with no buttons", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("Which stocks should I buy?");
    const advice = lastOf("model-reply")!;
    expect(advice).toHaveTextContent(`The model said${ADVICE_REPLY}`);
    expect(advice.querySelector("blockquote")).not.toBeNull();
    expect(lastAsk()).toHaveAttribute("data-step", "symbols");
    await send("MSFT");
    await send("What is momentum?");
    expect(lastOf("model-reply")).toHaveTextContent("Momentum:");
    expect(lastAsk()).toHaveAttribute("data-step", "model");
    for (const reply of log().querySelectorAll("[data-slot=model-reply]")) expect(within(reply as HTMLElement).queryByRole("button")).toBeNull();
  });

  it("withholds a reply that reads as advice, whatever the model wrote", async () => {
    renderFlow({ compiler: async () => ({ readings: [], not_enforced: [], intent: "other", reply: "You should buy MSFT, it will double." }) });
    await send("anything");
    expect(log().querySelector("[data-slot=model-reply]")).toBeNull();
    expect(lastOf("withheld")).toHaveTextContent("The model's reply was withheld: it read as advice, and the platform gives none. Nothing changed.");
    expect(main()).not.toHaveTextContent("You should buy MSFT");
  });
});

describe("A2, the review in the conversation", () => {
  it("drafts the mandate, then puts one section at a time to the owner, with its provenance", async () => {
    renderFlow();
    await toSections();
    const card = main().querySelector<HTMLElement>("[data-slot=contract-card]")!;
    expect(within(card).getByRole("heading", { name: "Your mandate in plain words" })).toBeInTheDocument();
    expect(card).toHaveTextContent("“$3,000”");
    expect(card).toHaveTextContent("“Grow it steadily”");
    expect(card).toHaveTextContent("“$300”");
    expect(main().querySelectorAll("[data-slot=review-section]")).toHaveLength(1);
    expect(within(fieldAt("/capital/allocation_usd")).getByText("You said “$3,000”")).toBeInTheDocument();
    expect(within(fieldAt("/capital/max_loss_from_allocation")).getByText("You said “$300”")).toBeInTheDocument();
    expect(within(fieldAt("/environment")).getByText("Platform default")).toHaveAttribute("data-provenance", "platform_default");

    fireEvent.click(confirmButton());
    expect(section("money")).toHaveAttribute("data-confirmed", "true");
    expect(within(section("money")).queryByRole("button")).toBeNull();
    const limits = section("limits");
    const proposed = [...limits.querySelectorAll<HTMLElement>("[data-slot=draft-field][data-provenance^=platform_]")];
    expect(proposed.length).toBeGreaterThan(5);
    for (const f of proposed) {
      expect(f).toHaveAttribute("data-active", "false");
      expect(f).toHaveTextContent("not active until you confirm this section");
    }
    expect(within(fieldAt("/risk/max_daily_loss")).getByText("Proposed by the platform")).toBeInTheDocument();
    fireEvent.click(confirmButton());
    for (const f of section("limits").querySelectorAll("[data-slot=draft-field]")) expect(f).toHaveAttribute("data-active", "true");
    expect([...log().querySelectorAll("[data-slot=owner-message]")].map((m) => m.textContent)).toEqual([
      "You: $3,000",
      "You: Grow it steadily",
      "You: $300",
      "You: MSFT",
      "You: Momentum",
      "You: 20",
      "You: Confirm money and goal",
      "You: Confirm limits",
    ]);
    expect(screen.queryByRole("button", { name: /confirm all|accept all/i })).toBeNull();
    expect(composer()).toHaveFocus();
  });

  it("states the unasked dollars and the losses in dollars on the contract card", async () => {
    renderFlow();
    await toSections();
    const card = main().querySelector<HTMLElement>("[data-slot=contract-card]")!;
    const unasked = card.querySelector<HTMLElement>("[data-slot=unasked]")!;
    expect(unasked).toHaveTextContent("Unasked dollars: what could trade without asking you once you confirm");
    expect(unasked).toHaveTextContent("$0.00");
    expect(unasked).toHaveTextContent("No buy runs without your answer");
    const figures = card.querySelector<HTMLElement>("[data-slot=loss-figures]")!;
    expect(figures).toHaveTextContent("Most it may lose, in total$300.00");
    expect(card).toHaveTextContent("Price gaps and exit prices can make any of these losses larger.");
    expect(card).toHaveTextContent("of simulated money, on paper.");
  });

  it("keeps a goal no limit can check under Not enforced, apart from the fields", async () => {
    renderFlow();
    await toSections({ goal: "Grow it steadily for my retirement" });
    const list = screen.getByRole("region", { name: "Not enforced" });
    expect(within(list).getByText("“Grow it steadily for my retirement”")).toBeInTheDocument();
    expect(list).toHaveTextContent("They reach the agent's models only as description text.");
    expect(main().querySelector("[data-slot=draft-field] [data-slot=not-enforced-item]")).toBeNull();
    expect(fieldAt("/goal/end_date")).toHaveTextContent("Runs until you stop it.");
  });

  it("unconfirms what a change touches, drafts again, and asks the changed sections again", async () => {
    renderFlow();
    await toSections();
    fireEvent.click(confirmButton());
    fireEvent.click(confirmButton());
    fireEvent.click(confirmButton());
    expect(confirmButton()).toHaveAccessibleName("Confirm section: When it asks you");
    await send("Make it $2,000");
    expect(lastOf("noted")).toHaveTextContent("Money it may use$2,000.00");
    expect(log().querySelectorAll("[data-slot=earlier-draft]")).toHaveLength(1);
    expect(log().querySelectorAll("[data-slot=contract-card]")).toHaveLength(1);
    expect(main().querySelector("[data-slot=contract-card]")).toHaveTextContent("“Make it $2,000”");
    expect(section("money")).toHaveAttribute("data-confirmed", "false");
    expect(within(section("money")).getByRole("button", { name: "Confirm section: Money and goal" })).toBeInTheDocument();
    expect(fieldAt("/capital/allocation_usd")).toHaveTextContent("$2,000.00");
    fireEvent.click(confirmButton());
    expect(section("limits")).toHaveAttribute("data-confirmed", "false");
    fireEvent.click(confirmButton());
    expect(section("strategy")).toHaveAttribute("data-confirmed", "true");
    expect(confirmButton()).toHaveAccessibleName("Confirm section: When it asks you");
  });

  it("offers the record only once every section is confirmed", async () => {
    renderFlow();
    await toSections();
    expect(screen.queryByRole("button", { name: "Review and confirm" })).toBeNull();
    confirmAll();
    expect(lastAsk()).toHaveAttribute("data-step", "ready");
    expect(button("Review and confirm")).toBeEnabled();
  });

  it("draws every action as the app's one key, so no button shouts over another (DEC-469)", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("about ten percent");
    await toSectionsFromSymbols();
    expect(main().querySelectorAll("button[data-variant=primary], button[class*='kumo-button-emphasis']")).toHaveLength(0);
    const actions = [...main().querySelectorAll<HTMLButtonElement>("button[data-kumo-component=Button], button[data-variant]")];
    expect(actions.length).toBeGreaterThan(1);
    for (const b of actions) for (const c of KEY.split(" ")) expect(b, b.textContent ?? "").toHaveClass(c);
  });
});

async function toSectionsFromSymbols() {
  fireEvent.click(within(lastAsk()).getByRole("button", { name: /^Yes/ }));
  await send("MSFT");
  chooseModel(/^Momentum/);
  await send("20");
}

describe("when the model fails", () => {
  it("keeps the owner's words when the model does not answer, and tries again or reads figures only", async () => {
    let up = false;
    const flaky: Compiler = (input) => (up ? INSTANT(input) : Promise.reject(new Error("timeout")));
    renderFlow({ compiler: flaky });
    await send("$3,000");
    const failed = lastOf("failed")!;
    expect(failed).toHaveAttribute("data-reason", "unreachable");
    expect(failed).toHaveTextContent("The model did not answer.Nothing was read and nothing changed. Your words are kept.");
    expect(log().querySelector("[data-slot=owner-message]")).toHaveTextContent("$3,000");
    fireEvent.click(within(failed).getByRole("button", { name: "Try again" }));
    await settle();
    expect(lastOf("failed")).toBeDefined();
    expect(log().querySelectorAll("[data-slot=failed]")).toHaveLength(2);

    fireEvent.click(within(lastOf("failed")!).getByRole("button", { name: "Read it without the model" }));
    await settle();
    expect(lastOf("noted")).toHaveTextContent("Money it may use$3,000.00");
    expect(main().querySelector("[data-slot=model-note]")).toHaveTextContent("Reading without the model now: figures only, read exactly as written.");
    expect(within(lastOf("failed")!).queryByRole("button")).toBeNull();
    up = true;
    await send("Grow it");
    await send("about ten percent");
    expect(lastOf("unread")).toHaveTextContent("We could not find a loss in that.");
    expect(log().querySelector("[data-step=check]")).toBeNull();
  });

  it("uses none of an answer outside the compiler's schema", async () => {
    renderFlow({ compiler: async () => ({ readings: [{ field: "money", quote: "$3,000", value: "3000" }], intent: "answer" }) });
    await send("$3,000");
    expect(lastOf("failed")).toHaveAttribute("data-reason", "invalid");
    expect(lastOf("failed")).toHaveTextContent("We could not compile this.The model's answer did not match the compiler's schema, so none of it was used.");
    expect(log().querySelector("[data-slot=noted]")).toBeNull();
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
    expect(log().querySelector("[data-slot=noted]")).toBeNull();
    expect(lastOf("unread")).toHaveTextContent("We could not find an amount in that.");
  });
});

describe("A5, the confirmation record", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const record = () => main().querySelector<HTMLElement>("[data-slot=record]")!;
  const progress = () => main().querySelector<HTMLElement>("[data-slot=after-confirm]");

  it("shows the whole mandate expanded, with the version its confirmation binds, and sends nothing yet", async () => {
    renderFlow();
    await toConfirm();
    expect(heading()).toHaveTextContent("Confirm your mandate");
    expect(heading()).toHaveFocus();
    expect(within(heading()).getByText("PAPER")).toBeInTheDocument();
    expect(record().querySelector("[data-slot=mandate-version]")!.textContent).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(record()).toHaveTextContent("5 of 5, each by you, one at a time.");
    expect(record().querySelectorAll("[data-slot=record-section]")).toHaveLength(5);
    for (const f of record().querySelectorAll("[data-slot=draft-field]")) expect(f).toHaveAttribute("data-active", "true");
    expect(record()).toHaveTextContent("It decides with momentum (quant.momentum), with the settings you chose.");
    expect(record()).toHaveTextContent("It trades only MSFT.");
    expect(record()).toHaveTextContent("Your words");
    expect(record().querySelector("details, [aria-expanded=false]")).toBeNull();
    expect(main().querySelector("[data-slot=owl]")).toBeNull();
    expect(deployments()).toEqual([]);
  });

  it("keeps every line it shows in the record it sends, bound to the same version", async () => {
    renderFlow();
    await toConfirm();
    const version = record().querySelector("[data-slot=mandate-version]")!.textContent!;
    deployWithPasskey();
    expect(deployments()).toHaveLength(1);
    const [d] = deployments();
    expect(d.version).toBe(version);
    expect(d.record.screen).toBe("A5");
    expect(d.record.environment).toBe("paper");
    const text = record().textContent!.replace(/\s+/g, " ");
    for (const line of d.record.shown) for (const part of line.split(/: | \(|\)$/).filter(Boolean)) expect(text, line).toContain(part.replace(/\s+/g, " "));
    expect(d.record.shown).toContain(`Version 1: ${version}`);
  });

  it("asks for the passkey; a cancel or a failure sends nothing", async () => {
    renderFlow({ passkey: failingPasskey });
    await toConfirm();
    fireEvent.click(button("Confirm and deploy to paper"));
    expect(stepUpDialog()).toHaveTextContent("Confirm this mandate and deploy a new agent to paper with $3,000.00 of simulated money, trading MSFT.");
    press("Cancel");
    expect(main()).toHaveTextContent("Passkey check canceled. Nothing was confirmed or sent.");
    fireEvent.click(button("Confirm and deploy to paper"));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(main()).toHaveTextContent("Passkey check failed. Nothing was confirmed or sent.");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 4));
    expect(deployments()).toEqual([]);
    expect(progress()).toBeNull();
  });

  it("is sent, then recorded as a running agent with its owl, then shows the first request", async () => {
    renderFlow();
    await toConfirm();
    deployWithPasskey();
    expect(progress()!.querySelector("[data-phase]")).toHaveAttribute("data-phase", "sent");
    expect(progress()).toHaveTextContent("Sent at 14:05:20 ET; waiting for the runtime to record it. Nothing is active until it does.");
    expect(main().querySelector("[data-slot=confirmed]")).toHaveTextContent("You confirmed the record above.");
    expect(screen.queryByRole("button", { name: "Confirm and deploy to paper" })).toBeNull();
    expect(screen.getByRole("heading", { name: "After you confirmed" })).toHaveFocus();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const [d] = deployments();
    expect(progress()!.querySelector("[data-phase]")).toHaveAttribute("data-phase", "recorded");
    expect(progress()).toHaveTextContent("Agent 4 is running on paper");
    expect(progress()).toHaveTextContent("Recorded in the journal at 14:05:20 ET, as version 1.");
    expect(progress()!.querySelector("[data-slot=owl]")).toHaveAttribute("data-mood", "awake");
    expect(within(progress()!).getByRole("link", { name: "Open Agent 4" })).toHaveAttribute("href", `/agents/${d.agentId}`);

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(progress()!.querySelector("[data-slot=first-ask]")).toHaveTextContent("Its first check asks you: buy 6 MSFT at $44.62. If you do not answer by 14:10:20 ET, it is skipped.");
    expect(within(progress()!).getByRole("link", { name: "Review the request" }).getAttribute("href")).toMatch(/^\/approvals\/apr_[0-9A-HJKMNP-TV-Z]{26}$/);
  });

  it("says nothing was confirmed when the deployment rejects it at application (V-002 again), and returns to the conversation", async () => {
    renderFlow();
    await toConfirm();
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    const other = readAnswers("$1,000", "Grow it", "$100");
    if (!strategy.ok || !other.ok) throw new Error("fixture answers");
    act(() => {
      probed().deploy(mandateFrom(compile(other.value, ["AAPL"], strategy.value), probed().ws.connection.connection_id)!, { screen: "A5", environment: "paper", shown: [] });
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    deployWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const rejected = progress()!.querySelector<HTMLElement>("[data-phase=rejected]")!;
    expect(rejected).toHaveTextContent("Not deployed. Nothing was confirmed, and version 1 does not exist.");
    expect(rejected).toHaveTextContent("Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent asks for.");
    expect(probed().ws.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2", "Agent 3", "Agent 4"]);
    expect(button("Confirm and deploy to paper")).toBeEnabled();
    fireEvent.click(within(rejected).getByRole("button", { name: "Change your answers" }));
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(heading()).toHaveFocus();
    await send("Make it $2,000");
    expect(section("money")).toHaveAttribute("data-confirmed", "false");
  });

  it("will not let the next agent claim an instrument the new agent trades", async () => {
    renderFlow();
    await toConfirm();
    deployWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    fireEvent.click(button("Set up another agent"));
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(0);
    await send("$400");
    await send("Grow it");
    await send("$40");
    await send("MSFT");
    expect(lastOf("refused")).toHaveTextContent("MSFT is already traded by Agent 4.");
  });

  it("is the owner's alone to confirm", async () => {
    renderFlow({ role: "operator" });
    await toConfirm();
    expect(screen.queryByRole("button", { name: "Confirm and deploy to paper" })).toBeNull();
    expect(main()).toHaveTextContent("Only the workspace owner confirms a new mandate.");
  });
});

describe("nothing leaves the page", () => {
  it("fetches nothing and stores nothing through the whole conversation, and reaches the runtime only after the passkey", async () => {
    vi.useFakeTimers();
    const cookie = document.cookie;
    renderFlow();
    await toConfirm();
    expect(deployments()).toEqual([]);
    deployWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(deployments()).toHaveLength(1);
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
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    if (!strategy.ok) throw new Error(strategy.error);
    const connection = buildWorkspace("normal").connection.connection_id;
    const version = (loss: string) => mandateVersion(mandateFrom(compile(readQuestions("$5,000", "up 10%", loss), ["MSFT"], strategy.value), connection)!.mandate);
    expect(version("$500")).toBe(version("$500"));
    expect(version("$400")).not.toBe(version("$500"));
  });

  it("drafts no mandate without a model or an instrument", () => {
    const connection = buildWorkspace("normal").connection.connection_id;
    expect(mandateFrom(compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]), connection)).toBeNull();
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    if (!strategy.ok) throw new Error(strategy.error);
    expect(mandateFrom(compile(readQuestions("$5,000", "up 10%", "$500"), [], strategy.value), connection)).toBeNull();
  });

  it("keeps the record's lines in step with the draft", () => {
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    if (!strategy.ok) throw new Error(strategy.error);
    const lines = confirmationLines(compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"], strategy.value), "sha256:abc");
    expect(lines.slice(0, 4)).toEqual(["Confirm your mandate", "Version 1: sha256:abc", "Environment: Paper: simulated funds, no real money.", "Sections confirmed: 5 of 5, each by you, one at a time."]);
    expect(lines).toContain("Money it may use: $5,000.00 (You said)");
    expect(lines).toContain("You said “$5,000”");
    expect(lines).toContain("Model: Momentum (quant.momentum 1.0.0) (You entered)");
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
      expect(d.figures.positionLossAtStop <= d.figures.dailyLossBudget, `${money} ${loss}`).toBe(true);
      expect(d.terms.maxOrderUsd <= d.terms.maxPositionUsd).toBe(true);
      expect(d.figures.flattenLoss <= d.figures.floorLoss).toBe(true);
    }
  });

  it("counts no unasked dollars without an auto path, and bounds them by orders and exposure with one", () => {
    const d = compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]);
    expect(d.figures.unasked).toBe(0n);
    expect(d.terms.autonomyDefault).toBe("ask");
    expect(d.terms.autonomyRules).toEqual([]);
    const withAuto = { ...d.terms, autonomyRules: [{ id: "small-buys", then: "auto" as const }] };
    expect(unaskedUsd(withAuto)).toBe(mul(fromInt(10), dec("500")));
    expect(unaskedUsd({ ...withAuto, maxGrossExposureUsd: dec("1200") })).toBe(dec("1200"));
  });

  it("reads a loss in dollars or as a percentage, and refuses one above the ceiling or the money", () => {
    const usd = readLoss("$500", dec("5000"));
    const pct = readLoss("10%", dec("5000"));
    expect(usd.ok && pct.ok && usd.value.usd === pct.value.usd && usd.value.fraction === pct.value.fraction).toBe(true);
    expect(readLoss("21%", dec("5000"))).toMatchObject({ ok: false });
    expect(readLoss("$6,000", dec("5000"))).toEqual({ ok: false, error: "That is more than the $5,000.00 this agent may use." });
  });

  it("never proposes the instrument list or any auto, and defaults the environment to paper", () => {
    const d = compile(readQuestions("$5,000", "up 10%", "$500"), []);
    const fields = d.sections.flatMap((s) => s.fields);
    expect(fields.find((f) => f.path === "/universe/pinned_instruments")?.provenance).toBeNull();
    expect(fields.find((f) => f.path === "/environment")).toMatchObject({ provenance: "platform_default", value: "Paper: simulated funds, no real money." });
    expect(fields.filter((f) => /auto\b|without asking/i.test(f.value))).toEqual([]);
  });
});
