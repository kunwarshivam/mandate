import { fireEvent, screen, within } from "@testing-library/react";
import { type MockInstance, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dec, fromInt, mul } from "@/lib/decimal";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { PROTOTYPE_AGENT_ID } from "./confirmation";
import { type Read, compile, draftDigest, read, readLoss, unaskedUsd } from "./draft";
import { NewAgentPrototype } from "./new-agent-prototype";

const main = () => screen.getByRole("main");
const heading = () => screen.getByRole("heading", { level: 1 });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const field = () => screen.getByRole("textbox");

function renderPrototype() {
  setPathname("/design/new-agent");
  return renderWithRuntime(
    <main>
      <NewAgentPrototype />
    </main>,
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

function confirmAll(symbols = "msft, aapl") {
  fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: symbols } });
  for (const key of ["money", "limits", "autonomy", "universe"]) {
    fireEvent.click(within(section(key)).getByRole("button", { name: /^Confirm section/ }));
  }
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
    renderPrototype();
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
    renderPrototype();
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
    renderPrototype();
    fireEvent.click(button(/Answer three questions/));
    expect(screen.getByRole("textbox", { name: "How much money may this agent use? In dollars" })).toBeInTheDocument();
    expect(screen.getByText("In dollars").tagName).toBe("LABEL");
  });

  it("moves focus to the new step's heading on every step change", () => {
    renderPrototype();
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
    renderPrototype();
    fireEvent.click(button(/Answer three questions/));
    answer("a fair amount");
    expect(screen.getByRole("alert")).toHaveTextContent("Write the amount in figures, in dollars.");
    expect(field()).toHaveAttribute("aria-invalid", "true");
    expect(heading()).toHaveTextContent("How much money may this agent use?");
  });

  it("shows the workspace's loss ceiling as a limit and never substitutes it for the answer", () => {
    renderPrototype();
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
    renderPrototype();
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
    renderPrototype();
    fireEvent.click(button(/Describe it yourself/));
    answer("Something calm for the long run.");
    expect(screen.getByRole("alert")).toHaveTextContent("We could not find how much money it may use, in dollars, or how much it may lose");
    expect(heading()).toHaveTextContent("Describe it yourself");
  });
});

describe("A2, the compiled review", () => {
  it("badges every field with who wrote it, and quotes the owner's words for a stated value", () => {
    renderPrototype();
    toReview();
    expect(heading()).toHaveTextContent("Check your mandate");
    const badges = Array.from(main().querySelectorAll("[data-slot=draft-field] [data-slot=provenance-badge]")).map((b) => b.getAttribute("data-provenance"));
    expect(new Set(badges)).toEqual(new Set(["user_stated", "platform_proposed", "platform_default"]));
    for (const f of main().querySelectorAll("[data-slot=draft-field]")) {
      if (f.getAttribute("data-path") !== "/universe/pinned_instruments") expect(f.querySelector("[data-slot=provenance-badge]"), f.getAttribute("data-path")!).not.toBeNull();
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
    renderPrototype();
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
    for (const key of ["money", "autonomy", "universe"]) {
      expect(section(key)).toHaveAttribute("data-confirmed", "false");
      for (const f of section(key).querySelectorAll("[data-slot=draft-field][data-provenance^=platform_]")) expect(f).toHaveAttribute("data-active", "false");
    }
    expect(within(section("limits")).getByRole("button", { name: "Undo: Limits" })).toHaveFocus();
    expect(screen.queryByRole("button", { name: /confirm all|accept all/i })).toBeNull();

    fireEvent.click(within(section("limits")).getByRole("button", { name: "Undo: Limits" }));
    expect(fieldAt("/risk/max_daily_loss")).toHaveAttribute("data-active", "false");
  });

  it("states the unasked dollars and the losses in dollars on the contract card, with the three answers quoted", () => {
    renderPrototype();
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
    renderPrototype();
    toReview({ goal: "Grow it steadily for my retirement" });
    const list = screen.getByRole("region", { name: "Not enforced" });
    expect(within(list).getByText("“Grow it steadily for my retirement”")).toBeInTheDocument();
    expect(list).toHaveTextContent("They reach the agent's models only as description text.");
    expect(main().querySelector("[data-slot=draft-field] [data-slot=not-enforced-item]")).toBeNull();
    expect(fieldAt("/goal/end_date")).toHaveAttribute("data-provenance", "platform_proposed");
    expect(fieldAt("/goal/end_date")).toHaveTextContent("Runs until you stop it.");
  });

  it("leaves the instrument list to the owner and will not confirm that section without it", () => {
    renderPrototype();
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

  it("enables deploying only once every section is confirmed", () => {
    renderPrototype();
    toReview();
    const deploy = button("Confirm and deploy to paper");
    expect(deploy).toBeDisabled();
    expect(screen.getByText(/sections confirmed/)).toHaveTextContent("0 of 4 sections confirmed. Confirm each section to continue.");
    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT" } });
    for (const key of ["money", "limits", "autonomy"]) {
      fireEvent.click(within(section(key)).getByRole("button", { name: /^Confirm section/ }));
      expect(deploy).toBeDisabled();
    }
    fireEvent.click(within(section("universe")).getByRole("button", { name: /^Confirm section/ }));
    expect(deploy).toBeEnabled();
    expect(screen.getByText(/sections confirmed/)).toHaveTextContent("4 of 4 sections confirmed. Every section is confirmed.");

    fireEvent.change(screen.getByLabelText("Symbols it may trade"), { target: { value: "MSFT, AAPL" } });
    expect(section("universe")).toHaveAttribute("data-confirmed", "false");
    expect(deploy).toBeDisabled();
  });

  it("has one primary action on the screen", () => {
    renderPrototype();
    toReview();
    expect(main().querySelectorAll("button[data-variant=primary], button[class*='kumo-button-emphasis']")).toHaveLength(1);
  });
});

describe("A5, the confirmation record", () => {
  it("shows the owl for the first time, the version and time, and says nothing was saved", () => {
    renderPrototype();
    expect(main().querySelector("[data-slot=owl]")).toBeNull();
    toReview();
    expect(main().querySelector("[data-slot=owl]")).toBeNull();
    confirmAll();
    fireEvent.click(button("Confirm and deploy to paper"));

    expect(heading()).toHaveTextContent("Mandate confirmed");
    expect(heading()).toHaveFocus();
    expect(within(heading()).getByText("PAPER")).toBeInTheDocument();
    const owl = main().querySelector("[data-slot=owl]")!;
    expect(owl).toHaveAttribute("data-mood", "awake");
    expect(main().querySelector("[data-slot=nothing-saved]")).toHaveTextContent("This prototype saved nothing.");
    expect(main()).toHaveTextContent("Sep 28, 2026, 14:05:20 ET");
    expect(main()).toHaveTextContent(/Version 1, draft [0-9a-f]{16}/);
    expect(main()).toHaveTextContent("4 of 4");
    expect(main().querySelectorAll("[data-slot=record-section]")).toHaveLength(4);
    for (const f of main().querySelectorAll("[data-slot=draft-field]")) expect(f).toHaveAttribute("data-active", "true");
    expect(main()).toHaveTextContent("AAPL, MSFT");
    expect(main().querySelector("[data-slot=contract-card] [data-slot=unasked]")).toHaveTextContent("$0.00");
  });

  it("starts over from an empty first step", () => {
    renderPrototype();
    toReview();
    confirmAll();
    fireEvent.click(button("Confirm and deploy to paper"));
    fireEvent.click(button("Start over"));
    expect(heading()).toHaveTextContent("Set up an agent");
    expect(heading()).toHaveFocus();
    fireEvent.click(button(/Answer three questions/));
    expect(field()).toHaveValue("");
  });

  it("draws the same owl for the fixed fixture agent every time", () => {
    renderPrototype();
    toReview();
    confirmAll();
    fireEvent.click(button("Confirm and deploy to paper"));
    expect(PROTOTYPE_AGENT_ID).toMatch(/^agt_[0-9A-HJKMNP-TV-Z]{26}$/);
    const first = main().querySelector("[data-slot=owl]")!.innerHTML;
    fireEvent.click(button("Start over"));
    toReview({ money: "$9,000", goal: "Grow it", loss: "5%" });
    confirmAll("NVDA");
    fireEvent.click(button("Confirm and deploy to paper"));
    expect(main().querySelector("[data-slot=owl]")!.innerHTML).toBe(first);
  });
});

describe("nothing leaves the page", () => {
  it("fetches nothing and stores nothing through the whole flow", () => {
    const cookie = document.cookie;
    renderPrototype();
    toReview();
    confirmAll();
    fireEvent.click(button("Confirm and deploy to paper"));
    fireEvent.click(button("Start over"));
    fireEvent.click(button(/Describe it yourself/));
    answer("Use $3,000. I can lose $300.");
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
    expect(getItem).not.toHaveBeenCalled();
    expect(document.cookie).toBe(cookie);
  });
});

describe("the fixture compiler", () => {
  const readQuestions = (money: string, goal: string, loss: string): Read => {
    const r = read({ kind: "questions", answers: { money, goal, loss } });
    if (!r.ok) throw new Error(r.error);
    return r.value;
  };

  it("drafts the same mandate, and the same digest, from the same words", () => {
    const a = compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]);
    const b = compile(readQuestions("$5,000", "up 10%", "$500"), ["MSFT"]);
    expect(draftDigest(a)).toBe(draftDigest(b));
    expect(draftDigest(compile(readQuestions("$5,000", "up 10%", "$400"), ["MSFT"]))).not.toBe(draftDigest(a));
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
