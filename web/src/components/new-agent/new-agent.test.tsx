import { act, fireEvent, screen, within } from "@testing-library/react";
import { type MockInstance, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { KEY } from "@/components/kumo/key";
import { PASSKEY_ANSWER_MS, type Passkey } from "@/components/stop/step-up-dialog";
import type { Workspace } from "@/fixtures/types";
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
import { INTRO, READY } from "./conversation";
import { MODELS, type Read, compile, mandateFrom, readAnswers, readLoss, readStrategy, unaskedUsd } from "./draft";
import { NewAgentFlow } from "./new-agent-flow";
import { GAP_NOTE, summaryLines } from "./summary";

const main = () => screen.getByRole("main");
const heading = () => screen.getByRole("heading", { level: 1 });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const composer = () => screen.getByRole("textbox", { name: "Your message" });
const log = () => screen.getByRole("log", { name: "Conversation" });
const thinking = () => main().querySelector<HTMLElement>("[data-slot=thinking]")!;

const deployments = (): Deployment[] => probed().deployments;

const INSTANT = fixtureCompiler({ latencyMs: 0 });

function renderFlow({
  role = "owner",
  passkey,
  compiler = INSTANT,
  workspace,
}: { role?: Role; passkey?: Passkey; compiler?: Compiler; workspace?: (ws: Workspace) => Workspace } = {}) {
  setPathname("/agents/new");
  return renderWithRuntime(
    <main>
      <NewAgentFlow compiler={compiler} />
      <RuntimeProbe />
    </main>,
    "normal",
    { role, ...(passkey ? { passkey } : {}), ...(workspace ? { workspace } : {}) },
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

const replies = () => [...log().querySelectorAll<HTMLElement>("[data-slot=said]")];
/** The platform's last reply, its sentences one per entry. */
const lastReply = () => [...replies().at(-1)!.querySelectorAll("p")].map((p) => p.textContent);
const summaries = () => [...log().querySelectorAll<HTMLElement>("[data-slot=summary]")];
const summary = () => summaries().at(-1)!;
const row = (label: string) => [...summary().querySelectorAll<HTMLElement>("[data-slot=summary-row]")].find((r) => r.querySelector("dt")?.textContent === label)!;
const sectionRows = (key: string) => [...summary().querySelectorAll<HTMLElement>(`[data-slot=summary-section][data-section=${key}] [data-slot=summary-row]`)];

function chooseModel(name: RegExp) {
  fireEvent.click(within(screen.getByRole("group", { name: "Models" })).getByRole("button", { name }));
}

/** The fixture account has $3,478.36 that no agent uses, so a deployment asks for less. */
async function toSummary({ money = "$3,000", goal = "Grow it steadily", loss = "$300", symbols = "MSFT" } = {}) {
  await send(money);
  await send(goal);
  await send(loss);
  await send(symbols);
  chooseModel(/^Momentum/);
  await send("20");
}

function createWithPasskey() {
  fireEvent.click(button("Create agent"));
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

  it("says what it understood in a sentence, then asks the next thing, with no cards to confirm", async () => {
    renderFlow();
    await send("$3,000");
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
    expect(lastReply()).toEqual(["Got it: $3,000.00 to use.", "What's it for, in your own words?"]);
    expect(composer()).toHaveValue("");
    expect(composer()).toHaveFocus();

    await send("Grow it steadily");
    expect(lastReply()).toEqual([
      "Got it: the goal “Grow it steadily”.",
      "How much could you stand to lose, in total? In dollars, or as a share of the money. This workspace allows at most 20% of the money, $600.00.",
    ]);
    await send("$300");
    expect(lastReply()).toEqual(["Got it: a loss limit of $300.00 (10% of the money).", "Which stocks or ETFs can it trade? Write their symbols. Only you choose them."]);
    for (const reply of replies()) expect(within(reply).queryByRole("button")).toBeNull();
    for (const line of replies().flatMap((r) => [...r.querySelectorAll("p")].map((p) => p.textContent!))) {
      if (!line.startsWith("Got it")) expect(line.replace(CEILING, "")).not.toMatch(EXAMPLES);
    }
  });

  it("reads everything said at once, and asks only what is still missing", async () => {
    renderFlow();
    await send("Use $3,000 on big tech. Stop when it is up 10%. I can stand to lose 10%. Avoid oil companies.");
    expect(lastReply()).toEqual([
      "Got it: $3,000.00 to use, a loss limit of $300.00 (10% of the money), and the goal “Stop when it is up 10%”.",
      "No limit can check “Avoid oil companies”, so it isn't enforced. The agent gets it as a note.",
      "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.",
    ]);
    await send("MSFT");
    chooseModel(/^Momentum/);
    await send("20");
    expect(row("Goal")).toHaveTextContent("Stops when its equity reaches $3,300.00");
    expect(within(summary()).getByText("“Avoid oil companies”")).toBeInTheDocument();
  });

  it("reads one run-on sentence the way a person means it, and repeats no part of the goal as a note", async () => {
    renderFlow();
    await send("I want it to use about three grand to grow it steadily, avoiding oil companies. I could stand to lose $300.");
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
    expect(lastReply()).toEqual(["Got it: $3,000.00 to use.", "I read “about three grand” as $3,000.00.", "What's it for, in your own words?"]);
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
    expect(seen).toEqual([{ messages: [{ id: expect.any(String), text: "$3,000" }], asked: "money", models: [expect.objectContaining({ name: "Mean reversion" }), expect.objectContaining({ name: "Momentum" })] }]);

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
    expect(lastReply()).toEqual(["I couldn't find an amount in that. How much money can it use, in dollars?"]);
    await send("yes");
    expect(lastReply()).toEqual(["I still couldn't find an amount. Write the figure on its own, in dollars, with nothing else in the message."]);
    expect(lastReply().join(" ")).not.toMatch(EXAMPLES);
    await send("$3,000");
    expect(lastReply()).toEqual(["Got it: $3,000.00 to use.", "What's it for, in your own words?"]);
    await send("Grow it");
    await send("yes");
    expect(lastReply()).toEqual(["I couldn't find a loss in that. Write it in dollars, or as a percentage of the money."]);
  });

  it("names the range a model setting must fall in when it asks, so the owner never guesses, and still proposes no value", async () => {
    for (const m of MODELS) for (const p of m.params) expect(p.question).toMatch(/from [\d.]+ to [\d.]+/i);
  });

  it("refuses a loss above the workspace's ceiling, never moving the answer to fit", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it steadily");
    await send("$2,000");
    expect(lastReply()).toEqual(["That is more than this workspace allows: at most 20% of the money, $600.00. Change your answer to continue."]);
    await send("$600");
    expect(lastReply()[0]).toBe("Got it: a loss limit of $600.00 (20% of the money).");
  });

  it("refuses more money than the account has free when it is said (V-002), and says how much is free", async () => {
    renderFlow();
    await send("$5,000");
    expect(lastReply()).toEqual(["Your paper account has $3,478.36 that no agent uses, less than the $5,000.00 this agent would use. Write a smaller amount."]);
  });

  it("refuses a symbol another agent already trades on the account (V-006), and takes another", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("XYZ");
    expect(lastReply()).toEqual(["XYZ is already traded by Agent 2. One agent trades an instrument on an account; choose another."]);
    await send("msft, aapl");
    expect(lastReply()[0]).toBe("Got it: AAPL, MSFT to trade.");
    expect(screen.getByRole("group", { name: "Models" })).toBeInTheDocument();
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
    expect(replies().at(-1)!.textContent).not.toMatch(/recommended|popular|best|return|sharpe|win rate/i);
  });

  it("takes a model by name, asks each setting with no value filled in, and refuses one out of range", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("MSFT");
    await send("mean reversion");
    expect(lastReply()).toEqual(["Got it: the Mean reversion model.", "How many bars should the model read back over? A whole number from 2 to 500."]);
    await send("20");
    expect(lastReply()).toEqual(["Got it: lookback of 20.", "How far below its average, in standard deviations, must a price be before the model scores a buy? From 0.5 to 4, with at most two decimals."]);
    await send("9");
    expect(lastReply()).toEqual(["Entry z-score: a number from 0.5 to 4, with at most two decimals."]);
    await send("1.5");
    expect(lastReply()).toEqual(["Got it: entry z-score of 1.5.", READY]);
    expect(row("Its settings")).toHaveTextContent("Lookback, in bars: 20; Entry z-score: 1.5");
    expect(within(row("Its settings")).queryByText("Proposed")).toBeNull();
  });

  it("explains a model and refuses to choose for the owner, in plain text with no buttons", async () => {
    renderFlow();
    await send("$3,000");
    await send("Grow it");
    await send("$300");
    await send("Which stocks should I buy?");
    expect(lastReply()).toEqual([ADVICE_REPLY, "Which stocks or ETFs can it trade? Write their symbols. Only you choose them."]);
    expect(within(replies().at(-1)!).queryByRole("button")).toBeNull();
    await send("MSFT");
    await send("What is momentum?");
    expect(lastReply()[0]).toMatch(/^Momentum: /);
    expect(within(replies().at(-1)!).getAllByRole("button")).toEqual(within(screen.getByRole("group", { name: "Models" })).getAllByRole("button"));
  });

  it("withholds a reply that reads as advice, whatever the model wrote", async () => {
    renderFlow({ compiler: async () => ({ readings: [], not_enforced: [], intent: "other", reply: "You should buy MSFT, it will double." }) });
    await send("anything");
    expect(lastReply()).toEqual(["I left out a reply that read like advice. Owlhead doesn't give any.", "How much money can it use, in dollars? It trades on paper, with simulated money."]);
    expect(main()).not.toHaveTextContent("You should buy MSFT");
  });
});

describe("the agent, shown once and created in one step", () => {
  it("shows every value once nothing is missing, marks what Owlhead drafted, and sends nothing yet", async () => {
    renderFlow();
    await toSummary();
    expect(summaries()).toHaveLength(1);
    expect(within(summary()).getByRole("heading", { name: "Your agent" })).toBeInTheDocument();
    expect(within(summary()).getByText("PAPER")).toBeInTheDocument();
    expect(summary()).toHaveTextContent("This agent may use $3,000.00 of simulated money, on paper.");
    expect(summary()).toHaveTextContent("It trades only MSFT.");
    expect(summary().querySelector("[data-slot=mandate-version]")!.textContent).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(row("Money it may use")).toHaveTextContent("$3,000.00");
    expect(row("Money it may use").querySelector("[data-slot=tag]")).toBeNull();
    expect(row("Where it runs").querySelector("[data-slot=tag]")).toHaveTextContent("Default");
    const limits = sectionRows("limits");
    expect(limits.length).toBeGreaterThan(5);
    for (const r of limits) expect(r.querySelector("[data-slot=tag]")).toHaveTextContent("Proposed");
    expect(summary().querySelector("details, [aria-expanded=false]")).toBeNull();
    expect(screen.queryByRole("button", { name: /confirm section|accept all/i })).toBeNull();
    expect(button("Create agent")).toBeEnabled();
    expect(deployments()).toEqual([]);
  });

  it("states the losses in dollars and what could buy without asking", async () => {
    renderFlow();
    await toSummary();
    expect(row("Most it may lose, in total")).toHaveTextContent("$300.00");
    expect(row("Fall at which it closes everything").querySelector("[data-slot=tag]")).toHaveTextContent("Proposed");
    expect(row("Could buy without asking you")).toHaveTextContent("$0.00");
    expect(summary()).toHaveTextContent(GAP_NOTE);
    expect(summary()).toHaveTextContent("It asks you before every buy.");
  });

  it("keeps a goal no limit can check as a note, marked not enforced", async () => {
    renderFlow();
    await toSummary({ goal: "Grow it steadily for my retirement" });
    expect(within(summary().querySelector<HTMLElement>("[data-slot=not-enforced]")!).getByText("“Grow it steadily for my retirement”")).toBeInTheDocument();
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
    expect(screen.getAllByRole("button", { name: "Create agent" })).toHaveLength(1);
  });

  it("asks again for a loss a change no longer fits, and offers nothing to create until it has one", async () => {
    renderFlow();
    await toSummary();
    await send("Make it $1,000");
    expect(lastReply()[1]).toBe("With $1,000.00, the loss you gave no longer fits. That is more than this workspace allows: at most 20% of the money, $200.00. Change your answer to continue.");
    expect(lastReply()[2]).toMatch(/^How much could you stand to lose, in total\?/);
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    await send("$100");
    expect(button("Create agent")).toBeEnabled();
  });

  it("draws every action as the app's one key, so no button shouts over another (DEC-469)", async () => {
    renderFlow();
    await toSummary();
    expect(main().querySelectorAll("button[data-variant=primary], button[class*='kumo-button-emphasis']")).toHaveLength(0);
    const actions = [...main().querySelectorAll<HTMLButtonElement>("button[data-kumo-component=Button], button[data-variant]")];
    expect(actions.length).toBeGreaterThan(0);
    for (const b of actions) for (const c of KEY.split(" ")) expect(b, b.textContent ?? "").toHaveClass(c);
  });
});

describe("when the model fails", () => {
  it("keeps the owner's words when the model does not answer, and tries again or reads figures only", async () => {
    let up = false;
    const flaky: Compiler = (input) => (up ? INSTANT(input) : Promise.reject(new Error("timeout")));
    renderFlow({ compiler: flaky });
    await send("$3,000");
    const failed = () => [...log().querySelectorAll<HTMLElement>("[data-slot=failed]")].at(-1)!;
    expect(failed().querySelector("[data-reason]")).toHaveAttribute("data-reason", "unreachable");
    expect(failed()).toHaveTextContent("The model didn't answer, so nothing was read. Your message is kept.");
    expect(log().querySelector("[data-slot=owner-message]")).toHaveTextContent("$3,000");
    fireEvent.click(within(failed()).getByRole("button", { name: "Try again" }));
    await settle();
    expect(log().querySelectorAll("[data-slot=failed]")).toHaveLength(2);

    fireEvent.click(within(failed()).getByRole("button", { name: "Continue without the model" }));
    await settle();
    expect(lastReply()[0]).toBe("Got it: $3,000.00 to use.");
    expect(main().querySelector("[data-slot=model-note]")).toHaveTextContent("Reading without the model now: figures only, exactly as written.");
    expect(within(failed()).queryByRole("button")).toBeNull();
    up = true;
    await send("Grow it");
    await send("about ten percent");
    expect(lastReply()).toEqual(["I couldn't find a loss in that. Write it in dollars, or as a percentage of the money."]);
  });

  it("uses none of an answer outside the compiler's schema", async () => {
    renderFlow({ compiler: async () => ({ readings: [{ field: "money", quote: "$3,000", value: "3000" }], intent: "answer" }) });
    await send("$3,000");
    expect(log().querySelector("[data-reason]")).toHaveAttribute("data-reason", "invalid");
    expect(log().querySelector("[data-slot=failed]")).toHaveTextContent("The model's answer didn't match the compiler's schema, so I used none of it. Your message is kept.");
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
    expect(lastReply()).toEqual(["I couldn't find an amount in that. How much money can it use, in dollars?"]);
  });
});

describe("creating it", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const progress = () => main().querySelector<HTMLElement>("[data-slot=after-confirm]");

  it("keeps every line it shows in the record it sends, bound to the same version", async () => {
    renderFlow();
    await toSummary();
    const version = summary().querySelector("[data-slot=mandate-version]")!.textContent!;
    const text = summary().textContent!.replace(/\s+/g, " ");
    createWithPasskey();
    expect(deployments()).toHaveLength(1);
    const [d] = deployments();
    expect(d.version).toBe(version);
    expect(d.record.screen).toBe("A5");
    expect(d.record.environment).toBe("paper");
    for (const line of d.record.shown) for (const part of line.split(/: | \(|\)$/).filter(Boolean)) expect(text, line).toContain(part.replace(/\s+/g, " "));
    expect(d.record.shown).toContain(`Version 1: ${version}`);
  });

  it("asks for the passkey; a cancel or a failure sends nothing", async () => {
    renderFlow({ passkey: failingPasskey });
    await toSummary();
    fireEvent.click(button("Create agent"));
    expect(stepUpDialog()).toHaveTextContent("Create this agent on paper with $3,000.00 of simulated money, trading MSFT.");
    press("Cancel");
    expect(main()).toHaveTextContent("Passkey check canceled. Nothing was created or sent.");
    fireEvent.click(button("Create agent"));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(main()).toHaveTextContent("Passkey check failed. Nothing was created or sent.");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 4));
    expect(deployments()).toEqual([]);
    expect(progress()).toBeNull();
  });

  it("is sent, then recorded as a running agent with its owl, then shows the first request", async () => {
    renderFlow();
    await toSummary();
    createWithPasskey();
    expect(progress()!.querySelector("[data-phase]")).toHaveAttribute("data-phase", "sent");
    expect(progress()).toHaveTextContent("Creating it… Sent at 14:05:20 ET. Nothing is active until the runtime records it.");
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    expect(screen.getByRole("heading", { name: "After you created it" })).toHaveFocus();
    expect(composer()).toBeDisabled();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const [d] = deployments();
    expect(progress()!.querySelector("[data-phase]")).toHaveAttribute("data-phase", "recorded");
    expect(progress()).toHaveTextContent("Agent 4 is running on paper.");
    expect(progress()).toHaveTextContent("Recorded in the journal at 14:05:20 ET, as version 1.");
    expect(progress()!.querySelector("[data-slot=owl]")).toHaveAttribute("data-mood", "awake");
    expect(progress()!.querySelector("[data-slot=hatch] [data-slot=egg]"), "the owl hatches once, answering the owner's act (DEC-514)").not.toBeNull();
    expect(within(progress()!).getByRole("link", { name: "Open Agent 4" })).toHaveAttribute("href", `/agents/${d.agentId}`);

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(progress()!.querySelector("[data-slot=first-ask]")).toHaveTextContent("Its first check asks you: buy 6 MSFT at $44.62. If you do not answer by 14:10:20 ET, it is skipped.");
    expect(within(progress()!).getByRole("link", { name: "Review the request" }).getAttribute("href")).toMatch(/^\/approvals\/apr_[0-9A-HJKMNP-TV-Z]{26}$/);
  });

  /** Another agent takes $1,000 of the account's free money while this one is being set up. */
  function anotherAgentTakesMoney() {
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    const other = readAnswers("$1,000", "Grow it", "$100");
    if (!strategy.ok || !other.ok) throw new Error("fixture answers");
    act(() => {
      probed().deploy(mandateFrom(compile(other.value, ["AAPL"], strategy.value), probed().ws.connection.connection_id)!, { screen: "A5", environment: "paper", shown: [] });
    });
  }

  it("will not create it when the account changed after the money was said (V-002 again), and takes a change", async () => {
    renderFlow();
    await toSummary();
    anotherAgentTakesMoney();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(main().querySelector("[data-slot=blocked]")).toHaveTextContent("Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent would use. Write a smaller amount.");
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    await send("Make it $2,000");
    expect(row("Money it may use")).toHaveTextContent("$2,000.00");
    expect(button("Create agent")).toBeEnabled();
  });

  it("says nothing was created when the runtime refuses it after the passkey, and takes a change", async () => {
    renderFlow();
    await toSummary();
    anotherAgentTakesMoney();
    createWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(deployments().map((d) => d.phase)).toEqual(["recorded", "rejected"]);
    const rejected = progress()!.querySelector<HTMLElement>("[data-phase=rejected]")!;
    expect(rejected).toHaveTextContent("Not created. Nothing was confirmed, and version 1 does not exist.");
    expect(rejected).toHaveTextContent("Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent asks for.");
    expect(probed().ws.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2", "Agent 3", "Agent 4"]);
    expect(composer()).toBeEnabled();
    await send("Make it $2,000");
    expect(button("Create agent")).toBeEnabled();
  });

  it("will not let the next agent claim an instrument the new agent trades", async () => {
    renderFlow();
    await toSummary();
    createWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    fireEvent.click(button("Set up another agent"));
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(heading()).toHaveFocus();
    expect(log().querySelectorAll("[data-slot=owner-message]")).toHaveLength(0);
    await send("$400");
    await send("Grow it");
    await send("$40");
    await send("MSFT");
    expect(lastReply()[0]).toBe("MSFT is already traded by Agent 4. One agent trades an instrument on an account; choose another.");
  });

  it("is the owner's alone to create", async () => {
    renderFlow({ role: "operator" });
    await toSummary();
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    expect(main()).toHaveTextContent("Only the workspace owner can create an agent.");
  });
});

describe("creating it where the workspace requires independent approval (§4.3, V-047; interim)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const policy =
    (value: boolean | null | "absent", approverUsers?: number) =>
    (ws: Workspace): Workspace => {
      const next: Workspace = { ...ws, approver_users: approverUsers ?? ws.approver_users, independent_approval_required: value === "absent" ? null : value };
      if (value === "absent") delete (next as Partial<Workspace>).independent_approval_required;
      return next;
    };
  const SECOND_PERSON = "This workspace needs a second person to approve a new agent, and that approval can't be asked for here yet, so a passkey alone can't create it.";

  it.each([
    [true, 2],
    [true, 1],
    [null, 2],
    ["absent", 2],
  ] as const)("offers no passkey confirm when the policy is %s with %s approvers, says why, and creates nothing", async (value, approvers) => {
    renderFlow({ workspace: policy(value, approvers) });
    await toSummary();
    expect(summary()).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    expect(main()).not.toHaveTextContent("Uses your passkey.");
    const refusal = main().querySelector<HTMLElement>("[data-slot=blocked]")!;
    expect(refusal).toHaveTextContent(SECOND_PERSON);
    expect(refusal).toHaveAttribute("data-rule", "V-047");
    expect(main()).not.toHaveTextContent("V-047");

    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
    expect(stepUpDialog()).toBeNull();
    expect(deployments()).toEqual([]);
    expect(probed().ws.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2", "Agent 3"]);
    expect(main().querySelector("[data-slot=after-confirm]")).toBeNull();
  });

  const NOT_HERE = "That's everything. Here is your agent. It can't be created here without a second person's approval.";

  it.each([true, null, "absent"] as const)("ends the chat with a line that does not invite a create when the policy is %s", async (value) => {
    renderFlow({ workspace: policy(value) });
    await toSummary();
    expect(lastReply()).toEqual(["Got it: lookback of 20.", NOT_HERE]);
    await send("Make it $2,000");
    expect(lastReply()).toEqual(["Got it: $2,000.00 to use.", NOT_HERE]);
    for (const line of replies().flatMap((r) => [...r.querySelectorAll("p")].map((p) => p.textContent!))) {
      expect(line).not.toMatch(/Create it/);
      expect(line).not.toMatch(/V-047/);
    }
  });

  it("ends the chat inviting a create exactly as before when the policy is off", async () => {
    renderFlow({ workspace: policy(false) });
    await toSummary();
    expect(lastReply()).toEqual(["Got it: lookback of 20.", READY]);
    expect(READY).toBe("That's everything. Here is your agent. Create it, or tell me what to change.");
    await send("Make it $2,000");
    expect(lastReply()).toEqual(["Got it: $2,000.00 to use.", READY]);
  });

  it("still refuses after a change is said, since no draft can satisfy it", async () => {
    renderFlow({ workspace: policy(true) });
    await toSummary();
    await send("Make it $2,000");
    expect(row("Money it may use")).toHaveTextContent("$2,000.00");
    expect(screen.queryByRole("button", { name: "Create agent" })).toBeNull();
    expect(main().querySelector("[data-slot=blocked]")).toHaveAttribute("data-rule", "V-047");
  });

  it("takes the passkey and creates the agent exactly as before when the policy is off, even with one approver", async () => {
    renderFlow({ workspace: policy(false, 1) });
    await toSummary();
    expect(main().querySelector("[data-slot=blocked]")).toBeNull();
    createWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(deployments().map((d) => d.phase)).toEqual(["recorded"]);
    expect(probed().ws.agents.at(-1)!.label).toBe("Agent 4");
  });

  it("shows the rule only as an attribute when the runtime refuses a deployment under the policy", async () => {
    renderFlow({ workspace: policy(false) });
    await toSummary();
    fireEvent.click(button("Create agent"));
    press("Use passkey");
    act(() => {
      // Deliberate direct mutation: the policy changes between the owner's click and the runtime applying it.
      probed().ws.independent_approval_required = null;
      vi.advanceTimersByTime(PASSKEY_ANSWER_MS);
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const rejected = main().querySelector<HTMLElement>("[data-slot=after-confirm] [data-phase=rejected]")!;
    expect(rejected).toHaveTextContent("Not created. Nothing was confirmed, and version 1 does not exist.");
    expect(within(rejected).getByText(SECOND_PERSON)).toHaveAttribute("data-rule", "V-047");
    expect(rejected).not.toHaveTextContent("V-047");
    expect(probed().ws.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2", "Agent 3"]);
  });
});

describe("nothing leaves the page", () => {
  it("fetches nothing and stores nothing through the whole conversation, and reaches the runtime only after the passkey", async () => {
    vi.useFakeTimers();
    const cookie = document.cookie;
    renderFlow();
    await toSummary();
    expect(deployments()).toEqual([]);
    createWithPasskey();
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
    const lines = summaryLines(compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"], strategy.value), "sha256:abc");
    expect(lines.slice(0, 2)).toEqual(["Your agent", "PAPER: simulated funds"]);
    expect(lines.at(-1)).toBe("Version 1: sha256:abc");
    expect(lines).toContain("Money it may use: $5,000.00");
    expect(lines).toContain("Where it runs: Paper: simulated funds, no real money. (Default)");
    expect(lines).toContain("Model: Momentum (quant.momentum 1.0.0)");
    expect(lines).toContain("Could buy without asking you: $0.00");
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
