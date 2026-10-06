import { act, fireEvent, screen, within } from "@testing-library/react";
import { type MockInstance, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
import { confirmationLines } from "./confirmation";
import { type Read, compile, mandateFrom, read, readLoss, readStrategy, unaskedUsd } from "./draft";
import { NewAgentFlow } from "./new-agent-flow";
import { KEY } from "@/components/kumo/key";

const main = () => screen.getByRole("main");
const heading = () => screen.getByRole("heading", { level: 1 });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const field = () => screen.getByRole("textbox");

const deployments = (): Deployment[] => probed().deployments;

function renderFlow({ role = "owner", passkey }: { role?: Role; passkey?: Passkey } = {}) {
  setPathname("/agents/new");
  return renderWithRuntime(
    <main>
      <NewAgentFlow />
      <RuntimeProbe />
    </main>,
    "normal",
    { role, ...(passkey ? { passkey } : {}) },
  );
}

function answer(text: string) {
  fireEvent.change(field(), { target: { value: text } });
  fireEvent.click(button("Continue"));
}

/** Through the three questions to the review. */
function toReview({ money = "$5,000", goal = "Stop when it is up 10%", loss = "$500" } = {}) {
  fireEvent.click(button(/Answer three questions/));
  answer(money);
  answer(goal);
  answer(loss);
}

const section = (key: string) => main().querySelector<HTMLElement>(`[data-slot=review-section][data-section=${key}]`)!;
const fieldAt = (path: string) => main().querySelector<HTMLElement>(`[data-slot=draft-field][data-path="${path}"]`)!;

const confirmSection = (key: string) => fireEvent.click(within(section(key)).getByRole("button", { name: /^Confirm section/ }));

function chooseMomentum(lookback = "20") {
  fireEvent.click(screen.getByRole("radio", { name: /Momentum/ }));
  fireEvent.change(screen.getByLabelText("Lookback, in bars"), { target: { value: lookback } });
}

function confirmAll(symbols = "msft, aapl") {
  fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: symbols } });
  chooseMomentum();
  for (const key of ["money", "limits", "strategy", "autonomy", "universe"]) confirmSection(key);
}

/** The fixture account has $3,478.36 that no agent uses, so a deployment asks for less. */
const FITS = { money: "$3,000", loss: "$300" };

/** From the review to A5, every section confirmed. */
function toConfirm(symbols?: string) {
  confirmAll(symbols);
  fireEvent.click(button("Continue to confirm"));
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

describe("A0, the goal questions", () => {
  it("offers the three questions and describing it yourself as equal choices, with neither preselected or focused", () => {
    renderFlow();
    expect(heading()).toHaveTextContent("Set up an agent");
    const choices = within(screen.getByRole("group", { name: "How to start" })).getAllByRole("button");
    expect(choices.map((c) => c.textContent)).toEqual([expect.stringMatching(/^Answer three questions/), expect.stringMatching(/^Describe it yourself/)]);
    expect(choices[0].className).toBe(choices[1].className);
    for (const c of choices) {
      expect(c).not.toHaveAttribute("aria-pressed");
      expect(c).not.toHaveFocus();
    }
  });

  it("asks one question per step, every answer empty, with no suggested amounts or example returns", () => {
    renderFlow();
    fireEvent.click(button(/Answer three questions/));
    expect(heading()).toHaveTextContent("How much money may this agent use?");
    expect(field()).toHaveValue("");
    expect(field()).not.toHaveAttribute("placeholder");
    expect(main().textContent).not.toMatch(/\$\s?\d|\d\s?%|e\.g\.|for example/i);
    expect(main().querySelectorAll("input, textarea")).toHaveLength(1);

    answer("$5,000");
    expect(heading()).toHaveTextContent("What is the goal?");
    expect(field()).toHaveValue("");
    expect(main().textContent).not.toMatch(/\$\s?\d|\d\s?%|e\.g\.|for example/i);

    answer("Grow it steadily");
    expect(heading()).toHaveTextContent("How much could you stand to lose?");
    expect(field()).toHaveValue("");
    expect(field()).not.toHaveAttribute("placeholder");
  });

  it("names each answer field with its question and a real label", () => {
    renderFlow();
    fireEvent.click(button(/Answer three questions/));
    expect(screen.getByRole("textbox", { name: "How much money may this agent use? In dollars" })).toBeInTheDocument();
    expect(screen.getByText("In dollars").tagName).toBe("LABEL");
  });

  it("moves focus to the new step's heading on every step change", () => {
    renderFlow();
    expect(heading()).not.toHaveFocus();
    fireEvent.click(button(/Answer three questions/));
    expect(heading()).toHaveFocus();
    answer("$5,000");
    expect(heading()).toHaveFocus();
    fireEvent.click(button("Back"));
    expect(heading()).toHaveTextContent("How much money may this agent use?");
    expect(heading()).toHaveFocus();
  });

  it("says what is wrong with an answer and stays on the step", () => {
    renderFlow();
    fireEvent.click(button(/Answer three questions/));
    answer("a fair amount");
    expect(screen.getByRole("alert")).toHaveTextContent("Write the amount in figures, in dollars.");
    expect(field()).toHaveAttribute("aria-invalid", "true");
    expect(heading()).toHaveTextContent("How much money may this agent use?");
  });

  it("shows the workspace's loss ceiling as a limit and never substitutes it for the answer", () => {
    renderFlow();
    fireEvent.click(button(/Answer three questions/));
    answer("$5,000");
    answer("Grow it steadily");
    expect(main()).toHaveTextContent("This workspace's limit: at most 20% of the money, $1,000.00.");
    expect(field()).toHaveValue("");
    answer("$2,000");
    expect(screen.getByRole("alert")).toHaveTextContent("That is more than this workspace allows: at most 20% of the money, $1,000.00.");
    expect(field()).toHaveValue("$2,000");
    expect(heading()).toHaveTextContent("How much could you stand to lose?");
  });
});

describe("A1, describe it yourself", () => {
  it("starts empty, and drafts from the owner's words, quoting them back", () => {
    renderFlow();
    fireEvent.click(button(/Describe it yourself/));
    expect(heading()).toHaveTextContent("Describe it yourself");
    expect(field()).toHaveValue("");
    expect(field()).not.toHaveAttribute("placeholder");
    answer("Use $8,000 on big tech. Stop when it is up 12%. I can stand to lose 10%. Avoid oil companies.");
    expect(heading()).toHaveTextContent("Check your mandate");
    expect(fieldAt("/capital/allocation_usd")).toHaveTextContent("$8,000.00");
    expect(within(fieldAt("/capital/allocation_usd")).getByText("You said “Use $8,000 on big tech.”")).toBeInTheDocument();
    expect(within(fieldAt("/capital/max_loss_from_allocation")).getByText("You said “I can stand to lose 10%.”")).toBeInTheDocument();
    expect(fieldAt("/goal/profit_level")).toHaveTextContent("Stops when its equity reaches $8,960.00");
    expect(within(screen.getByRole("region", { name: "Not enforced" })).getByText("“Avoid oil companies.”")).toBeInTheDocument();
  });

  it("says what it could not find instead of guessing", () => {
    renderFlow();
    fireEvent.click(button(/Describe it yourself/));
    answer("Something calm for the long run.");
    expect(screen.getByRole("alert")).toHaveTextContent("We could not find how much money it may use, in dollars, or how much it may lose");
    expect(heading()).toHaveTextContent("Describe it yourself");
  });
});

/** The fields only the owner fills in, empty until they do (V-038, brief A3). */
const OWNER_ONLY = ["/universe/pinned_instruments", "/behavior/signal_models/0/id", "/behavior/signal_models/0/params"];

describe("A2, the compiled review", () => {
  it("badges every field with who wrote it, and quotes the owner's words for a stated value", () => {
    renderFlow();
    toReview();
    expect(heading()).toHaveTextContent("Check your mandate");
    const badges = Array.from(main().querySelectorAll("[data-slot=draft-field] [data-slot=provenance-badge]")).map((b) => b.getAttribute("data-provenance"));
    expect(new Set(badges)).toEqual(new Set(["user_stated", "platform_proposed", "platform_default"]));
    for (const f of main().querySelectorAll("[data-slot=draft-field]")) {
      if (!OWNER_ONLY.includes(f.getAttribute("data-path")!)) expect(f.querySelector("[data-slot=provenance-badge]"), f.getAttribute("data-path")!).not.toBeNull();
    }
    expect(within(fieldAt("/capital/allocation_usd")).getByText("You said")).toBeInTheDocument();
    expect(within(fieldAt("/capital/allocation_usd")).getByText("You said “$5,000”")).toBeInTheDocument();
    expect(within(fieldAt("/capital/max_loss_from_allocation")).getByText("You said “$500”")).toBeInTheDocument();
    expect(within(fieldAt("/goal/profit_level")).getByText("You said “10%”")).toBeInTheDocument();
    expect(within(fieldAt("/risk/max_daily_loss")).getByText("Proposed by the platform")).toBeInTheDocument();
    expect(within(fieldAt("/environment")).getByText("Platform default")).toHaveAttribute("data-provenance", "platform_default");
    expect(fieldAt("/environment")).toHaveTextContent("Paper: simulated funds, no real money.");
  });

  it("keeps proposed values inactive until their own section is confirmed, one section at a time", () => {
    renderFlow();
    toReview();
    const proposed = Array.from(main().querySelectorAll<HTMLElement>("[data-slot=draft-field][data-provenance^=platform_]"));
    expect(proposed.length).toBeGreaterThan(5);
    for (const f of proposed) {
      expect(f).toHaveAttribute("data-active", "false");
      expect(f).toHaveTextContent("not active until you confirm this section");
    }
    for (const f of main().querySelectorAll("[data-slot=draft-field][data-provenance=user_stated]")) expect(f).toHaveAttribute("data-active", "true");

    fireEvent.click(within(section("limits")).getByRole("button", { name: "Confirm section: Limits" }));
    expect(section("limits")).toHaveAttribute("data-confirmed", "true");
    for (const f of section("limits").querySelectorAll("[data-slot=draft-field]")) expect(f).toHaveAttribute("data-active", "true");
    for (const key of ["money", "strategy", "autonomy", "universe"]) {
      expect(section(key)).toHaveAttribute("data-confirmed", "false");
      for (const f of section(key).querySelectorAll("[data-slot=draft-field][data-provenance^=platform_]")) expect(f).toHaveAttribute("data-active", "false");
    }
    expect(within(section("limits")).getByRole("button", { name: "Undo: Limits" })).toHaveFocus();
    expect(screen.queryByRole("button", { name: /confirm all|accept all/i })).toBeNull();

    fireEvent.click(within(section("limits")).getByRole("button", { name: "Undo: Limits" }));
    expect(fieldAt("/risk/max_daily_loss")).toHaveAttribute("data-active", "false");
  });

  it("states the unasked dollars and the losses in dollars on the contract card, with the three answers quoted", () => {
    renderFlow();
    toReview();
    const card = main().querySelector<HTMLElement>("[data-slot=contract-card]")!;
    expect(within(card).getByRole("heading", { name: "Your mandate in plain words" })).toBeInTheDocument();
    const unasked = card.querySelector<HTMLElement>("[data-slot=unasked]")!;
    expect(unasked).toHaveTextContent("Unasked dollars: what could trade without asking you once you confirm");
    expect(unasked).toHaveTextContent("$0.00");
    expect(unasked).toHaveTextContent("No buy runs without your answer");
    expect(unasked).toHaveTextContent("Selling to cut risk never waits for you.");
    const figures = card.querySelector<HTMLElement>("[data-slot=loss-figures]")!;
    expect(figures).toHaveTextContent("One full position stopped out$100.00");
    expect(figures).toHaveTextContent("Most it may lose in one day$125.00");
    expect(figures).toHaveTextContent("Fall at which it closes everything$250.00");
    expect(figures).toHaveTextContent("Most it may lose, in total$500.00");
    expect(card).toHaveTextContent("Price gaps and exit prices can make any of these losses larger.");
    expect(card).toHaveTextContent("“$5,000”");
    expect(card).toHaveTextContent("“Stop when it is up 10%”");
    expect(card).toHaveTextContent("“$500”");
    expect(card).toHaveTextContent("of simulated money, on paper.");
  });

  it("lists what no limit can check apart from the fields, under Not enforced", () => {
    renderFlow();
    toReview({ goal: "Grow it steadily for my retirement" });
    const list = screen.getByRole("region", { name: "Not enforced" });
    expect(within(list).getByText("“Grow it steadily for my retirement”")).toBeInTheDocument();
    expect(list).toHaveTextContent("They reach the agent's models only as description text.");
    expect(main().querySelector("[data-slot=draft-field] [data-slot=not-enforced-item]")).toBeNull();
    expect(fieldAt("/goal/end_date")).toHaveAttribute("data-provenance", "platform_proposed");
    expect(fieldAt("/goal/end_date")).toHaveTextContent("Runs until you stop it.");
  });

  it("leaves the instrument list to the owner and will not confirm that section without it", () => {
    renderFlow();
    toReview();
    expect(screen.getByLabelText("Symbols it may trade")).toHaveValue("");
    expect(fieldAt("/universe/pinned_instruments")).toHaveTextContent("None yet.");
    fireEvent.click(within(section("universe")).getByRole("button", { name: /^Confirm section/ }));
    expect(screen.getByRole("alert")).toHaveTextContent("Add at least one symbol.");
    expect(section("universe")).toHaveAttribute("data-confirmed", "false");
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "msft aapl msft" } });
    expect(within(fieldAt("/universe/pinned_instruments")).getByText("You entered")).toBeInTheDocument();
    expect(fieldAt("/universe/pinned_instruments")).toHaveTextContent("AAPL, MSFT");
  });

  it("continues to the confirmation only once every section is confirmed, and a change unconfirms its section", () => {
    renderFlow();
    toReview(FITS);
    const next = button("Continue to confirm");
    expect(next).toBeDisabled();
    expect(screen.getByText(/sections confirmed/)).toHaveTextContent("0 of 5 sections confirmed. Confirm each section to continue.");
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT" } });
    chooseMomentum();
    for (const key of ["money", "limits", "strategy", "autonomy"]) {
      confirmSection(key);
      expect(next).toBeDisabled();
    }
    confirmSection("universe");
    expect(next).toBeEnabled();
    expect(screen.getByText(/sections confirmed/)).toHaveTextContent("5 of 5 sections confirmed. Every section is confirmed.");

    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT, AAPL" } });
    expect(section("universe")).toHaveAttribute("data-confirmed", "false");
    expect(next).toBeDisabled();
    confirmSection("universe");
    fireEvent.change(screen.getByLabelText("Lookback, in bars"), { target: { value: "30" } });
    expect(section("strategy")).toHaveAttribute("data-confirmed", "false");
    expect(next).toBeDisabled();
  });

  it("refuses to confirm more money than the account has free (V-002), and says how much is free", () => {
    renderFlow();
    toReview();
    confirmSection("money");
    expect(section("money")).toHaveAttribute("data-confirmed", "false");
    expect(within(section("money")).getByRole("alert")).toHaveTextContent(
      "Your paper account has $3,478.36 that no agent uses, less than the $5,000.00 this agent asks for. Change your answers to use less.",
    );
  });

  it("refuses an instrument another agent already trades on the account (V-006)", () => {
    renderFlow();
    toReview(FITS);
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT, XYZ" } });
    confirmSection("universe");
    expect(section("universe")).toHaveAttribute("data-confirmed", "false");
    expect(within(section("universe")).getByRole("alert")).toHaveTextContent("XYZ is already traded by Agent 2. One agent trades an instrument on an account; choose another.");
    expect(screen.getByLabelText("Symbols it may trade")).toHaveFocus();
    expect(screen.getByLabelText("Symbols it may trade")).toHaveAttribute("aria-invalid", "true");
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT" } });
    expect(within(section("universe")).queryByRole("alert")).toBeNull();
    confirmSection("universe");
    expect(section("universe")).toHaveAttribute("data-confirmed", "true");
  });

  it("lists the models by methodology only, with none chosen and every setting empty (brief A3)", () => {
    renderFlow();
    toReview();
    const radios = within(section("strategy")).getAllByRole("radio");
    expect(radios.map((r) => r.closest("label")!.querySelector("span span")!.textContent)).toEqual(["Mean reversion", "Momentum"]);
    for (const r of radios) expect(r).not.toBeChecked();
    expect(section("strategy").textContent).not.toMatch(/recommended|popular|best|return|sharpe|win rate/i);
    expect(fieldAt("/behavior/signal_models/0/id")).toHaveTextContent("None chosen yet.");
    confirmSection("strategy");
    expect(within(section("strategy")).getByRole("alert")).toHaveTextContent("Choose the model this agent uses. The platform does not choose it for you.");

    fireEvent.click(screen.getByRole("radio", { name: /Mean reversion/ }));
    expect(screen.getByLabelText("Lookback, in bars")).toHaveValue("");
    expect(screen.getByLabelText("Entry z-score")).toHaveValue("");
    confirmSection("strategy");
    expect(within(section("strategy")).getByRole("alert")).toHaveTextContent("Lookback: a whole number from 2 to 500.");
    fireEvent.change(screen.getByLabelText("Lookback, in bars"), { target: { value: "20" } });
    fireEvent.change(screen.getByLabelText("Entry z-score"), { target: { value: "9" } });
    confirmSection("strategy");
    expect(within(section("strategy")).getByRole("alert")).toHaveTextContent("Entry z-score: a number from 0.5 to 4, with at most two decimals.");
    fireEvent.change(screen.getByLabelText("Entry z-score"), { target: { value: "1.5" } });
    confirmSection("strategy");
    expect(section("strategy")).toHaveAttribute("data-confirmed", "true");
    expect(fieldAt("/behavior/signal_models/0/id")).toHaveTextContent("Mean reversion (quant.mean_reversion 1.0.0)");
    expect(within(fieldAt("/behavior/signal_models/0/params")).getByText("You entered")).toBeInTheDocument();
    expect(fieldAt("/behavior/signal_models/0/params")).toHaveTextContent("Lookback, in bars: 20; Entry z-score: 1.5");
  });

  it("draws every action as the app's one key, so no button shouts over another (DEC-469)", () => {
    renderFlow();
    toReview();
    expect(main().querySelectorAll("button[data-variant=primary], button[class*='kumo-button-emphasis']")).toHaveLength(0);
    const actions = [...main().querySelectorAll<HTMLButtonElement>("button[data-kumo-component=Button], button[data-variant]")];
    expect(actions.length).toBeGreaterThan(1);
    for (const b of actions) for (const c of KEY.split(" ")) expect(b, b.textContent ?? "").toHaveClass(c);
  });
});

describe("A5, the confirmation record", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const record = () => main().querySelector<HTMLElement>("[data-slot=record]")!;
  const progress = () => main().querySelector<HTMLElement>("[data-slot=after-confirm]");

  it("shows the whole mandate expanded, with the version its confirmation binds, and sends nothing yet", () => {
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
    expect(heading()).toHaveTextContent("Confirm your mandate");
    expect(heading()).toHaveFocus();
    expect(within(heading()).getByText("PAPER")).toBeInTheDocument();
    expect(record().querySelector("[data-slot=mandate-version]")!.textContent).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(record()).toHaveTextContent("5 of 5, each by you, one at a time.");
    expect(record().querySelectorAll("[data-slot=record-section]")).toHaveLength(5);
    for (const f of record().querySelectorAll("[data-slot=draft-field]")) expect(f).toHaveAttribute("data-active", "true");
    expect(record()).toHaveTextContent("It decides with momentum (quant.momentum), with the settings you chose.");
    expect(record()).toHaveTextContent("It trades only MSFT.");
    expect(record().querySelector("details, [aria-expanded=false]")).toBeNull();
    expect(main().querySelector("[data-slot=owl]")).toBeNull();
    expect(deployments()).toEqual([]);
  });

  it("keeps every line it shows in the record it sends, bound to the same version", () => {
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
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

  it("asks for the passkey; a cancel or a failure sends nothing", () => {
    renderFlow({ passkey: failingPasskey });
    toReview(FITS);
    toConfirm("MSFT");
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

  it("is sent, then recorded as a running agent with its owl, then shows the first request", () => {
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
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

  it("says nothing was confirmed when the deployment rejects it at application (V-002 again), and allows another try", () => {
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
    const strategy = readStrategy({ model: "quant.momentum", params: { lookback_bars: "20" } });
    const other = read({ kind: "questions", answers: { money: "$1,000", goal: "Grow it", loss: "$100" } });
    if (!strategy.ok || !other.ok) throw new Error("fixture answers");
    act(() => {
      probed().deploy(mandateFrom(compile(other.value, ["AAPL"], strategy.value), probed().ws.connection.connection_id)!, { screen: "A5", environment: "paper", shown: [] });
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    deployWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const rejected = progress()!.querySelector("[data-phase=rejected]")!;
    expect(rejected).toHaveTextContent("Not deployed. Nothing was confirmed, and version 1 does not exist.");
    expect(rejected).toHaveTextContent("Your paper account has $2,478.36 that no agent uses, less than the $3,000.00 this agent asks for.");
    expect(probed().ws.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2", "Agent 3", "Agent 4"]);
    expect(button("Confirm and deploy to paper")).toBeEnabled();
    fireEvent.click(within(rejected as HTMLElement).getByRole("button", { name: "Change your answers" }));
    expect(heading()).toHaveTextContent("Check your mandate");
  });

  it("will not let the next agent claim an instrument the new agent trades", () => {
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
    deployWithPasskey();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    fireEvent.click(button("Set up another agent"));
    expect(heading()).toHaveTextContent("Set up an agent");
    toReview({ money: "$400", goal: "Grow it", loss: "$40" });
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT" } });
    confirmSection("universe");
    expect(within(section("universe")).getByRole("alert")).toHaveTextContent("MSFT is already traded by Agent 4.");
  });

  it("is the owner's alone to confirm", () => {
    renderFlow({ role: "operator" });
    toReview(FITS);
    toConfirm("MSFT");
    expect(screen.queryByRole("button", { name: "Confirm and deploy to paper" })).toBeNull();
    expect(main()).toHaveTextContent("Only the workspace owner confirms a new mandate.");
  });
});

describe("nothing leaves the page", () => {
  it("fetches nothing and stores nothing through the whole flow, and reaches the runtime only after the passkey", () => {
    vi.useFakeTimers();
    const cookie = document.cookie;
    renderFlow();
    toReview(FITS);
    toConfirm("MSFT");
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

describe("the fixture compiler", () => {
  const readQuestions = (money: string, goal: string, loss: string): Read => {
    const r = read({ kind: "questions", answers: { money, goal, loss } });
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
