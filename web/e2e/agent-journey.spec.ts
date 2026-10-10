import { type Page, expect, test } from "@playwright/test";

/**
 * The whole journey on the fixture workspace (DEC-472, DEC-477), in one page load so the runtime's
 * state carries from screen to screen: set up an agent in a conversation, create it from its summary
 * with a passkey, find the new agent on Agents and Home, approve its first buy, see it fill with its
 * protective stop in place, read what it decided, and stop it with its kill switch. Every step after
 * the first `goto` is a click or a message, as an owner would take it.
 */

test.use({ viewport: { width: 1440, height: 900 } });

const dock = (page: Page) => page.getByRole("navigation", { name: "Primary" });

const log = (page: Page) => page.getByRole("log", { name: "Conversation" });
const composer = (page: Page) => page.getByRole("textbox", { name: "Your answer" });
const lastReply = (page: Page) => log(page).locator("[data-slot=said]").last();

/** Sends one message and waits until the model has read it. */
async function say(page: Page, text: string) {
  await composer(page).fill(text);
  await composer(page).press("Enter");
  await expect(log(page).locator("[data-slot=owner-message]").last()).toHaveText(`You: ${text}`);
  await expect(page.locator("[data-slot=thinking]")).toHaveText("");
}

/** Opens one of the agent's tabs and waits until it is the current page, so what follows reads that tab. */
async function openTab(page: Page, name: string) {
  const tab = page.getByRole("navigation", { name: "Agent sections" }).getByRole("link", { name });
  await tab.click();
  await expect(tab).toHaveAttribute("aria-current", "page");
}

test("set up, deploy, approve, fill, read, and stop a new agent", async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto("/agents");
  await expect(page.getByRole("heading", { level: 1, name: "Agents" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /^Agent 4/ })).toHaveCount(0);

  await test.step("the conversation, in the owner's words, with V-006 refusing a symbol another agent trades", async () => {
    await page.getByRole("link", { name: "Describe an agent" }).click();
    await expect(page.getByRole("heading", { level: 1, name: "Set up an agent" })).toBeVisible();
    await expect(lastReply(page)).toContainText("how much money it can use, what it's for, and how much you could stand to lose");
    await say(page, "about three grand");
    await expect(lastReply(page)).toContainText("I read “about three grand” as $3,000.00.");
    await expect(lastReply(page)).toContainText("What's it for, in your own words?");
    await say(page, "Grow it steadily, and avoid oil companies");
    await expect(lastReply(page)).toContainText("How much could you stand to lose, in total?");
    await say(page, "$300");
    await expect(lastReply(page)).toContainText("Which stocks or ETFs can it trade?");

    await say(page, "XYZ");
    await expect(lastReply(page)).toHaveText("Owlhead: XYZ is already traded by Agent 2. One agent trades an instrument on an account; choose another.");
    await expect(lastReply(page)).toBeInViewport();
    await expect(composer(page)).toBeFocused();
    await say(page, "MSFT");
    const send = page.getByRole("button", { name: "Send" });
    await expect(send).toBeInViewport();
    await expect
      .poll(async () => {
        const [button, bar] = await Promise.all([send.boundingBox(), dock(page).boundingBox()]);
        return button && bar ? button.y + button.height <= bar.y : false;
      }, { message: "the composer stays clear of the dock as the conversation grows" })
      .toBe(true);

    const models = page.getByRole("group", { name: "Models" });
    await expect(models.getByRole("button")).toHaveText([/^Mean reversion/, /^Momentum/]);
    await models.getByRole("button", { name: /^Momentum/ }).click();
    await expect(lastReply(page)).toContainText("How many bars should the model read back over?");
    await say(page, "20");
  });

  let agentHref = "";
  await test.step("the whole agent, shown once in the conversation, created with a passkey", async () => {
    const summary = page.locator("[data-slot=summary]");
    await expect(summary).toBeVisible();
    await expect(summary.locator("[data-slot=mandate-version]")).toHaveText(/^sha256:[0-9a-f]{64}$/);
    await expect(summary).toContainText("It trades only MSFT.");
    await expect(summary).toContainText("It decides with momentum (quant.momentum), with the settings you chose.");
    await expect(summary.locator("[data-slot=not-enforced-item]")).toContainText(["avoid oil companies"]);
    await expect(page.getByRole("button", { name: /confirm section/i })).toHaveCount(0);

    await page.getByRole("button", { name: "Create agent" }).click();
    const dialog = page.getByRole("dialog", { name: "Confirm it is you" });
    await expect(dialog).toContainText("Create this agent on paper with $3,000.00 of simulated money, trading MSFT.");
    await dialog.getByRole("button", { name: "Use passkey" }).click();

    const progress = page.locator("[data-slot=after-confirm]");
    await expect(progress.locator("[data-phase=sent]")).toBeVisible();
    await expect(progress.getByRole("heading", { name: "After you created it" })).toBeFocused();
    await expect(progress).toBeInViewport();
    await expect(progress.locator("[data-phase=recorded]")).toContainText("Agent 4 is running on paper");
    await expect(progress.locator("[data-slot=owl]")).toBeVisible();
    await expect(page.getByText("Your confirmed mandate.")).toBeVisible();
    await expect(progress.locator("[data-slot=first-ask]")).toContainText(/Its first check asks you: buy 6 MSFT at \$44\.62\./);
    const review = progress.getByRole("link", { name: "Review the request" });
    await expect(review).toBeInViewport();
    await expect
      .poll(async () => {
        const [link, bar] = await Promise.all([review.boundingBox(), dock(page).boundingBox()]);
        return link && bar ? link.y + link.height <= bar.y : false;
      }, { message: "the request's link clears the dock" })
      .toBe(true);
    agentHref = (await progress.getByRole("link", { name: "Open Agent 4" }).getAttribute("href")) ?? "";
    expect(agentHref).toMatch(/^\/agents\/agt_[0-9A-HJKMNP-TV-Z]{26}$/);
  });

  await test.step("the new agent is on Agents and on Home", async () => {
    await dock(page).getByRole("link", { name: "Agents" }).click();
    await expect(page.getByRole("heading", { level: 1, name: "Agents" })).toBeVisible();
    await expect(page.getByRole("heading", { name: /^Agent 4/ })).toBeVisible();
    await dock(page).getByRole("link", { name: "Home" }).click();
    await expect(page).toHaveURL("/");
    await expect(page.getByRole("main")).toContainText("Agent 4");
  });

  await test.step("D6: the first buy asks, and the owner approves it", async () => {
    await dock(page).getByRole("link", { name: "Approvals" }).click();
    await expect(page).toHaveURL("/approvals");
    await page.getByRole("main").getByRole("link", { name: /MSFT/ }).first().click();
    await expect(page.getByRole("main")).toContainText("Buy 6 MSFT at a limit of $44.62");
    await expect(page.getByRole("main")).toContainText("Your mandate asks before every buy.");
    await page.getByRole("button", { name: "Approve", exact: true }).click();
    await expect(page.getByRole("heading", { name: "You chose Approve" })).toBeVisible();
    await expect(page.getByRole("main")).toContainText("submitted and filled 6 at $44.62", { timeout: 15_000 });
  });

  await test.step("the fill and its protective stop, on the agent's positions, orders, and activity", async () => {
    await dock(page).getByRole("link", { name: "Agents" }).click();
    await page.locator(`a[href="${agentHref}"]`).first().click();
    await expect(page).toHaveURL(agentHref);

    await openTab(page, "Positions");
    await expect(page.getByRole("main")).toContainText("MSFT");
    await expect(page.getByRole("main")).toContainText("$41.05");

    await openTab(page, "Orders");
    await expect(page.getByRole("main")).toContainText("Buy 6 MSFT limit $44.62Filled");
    await expect(page.getByRole("main")).toContainText("Sell 6 MSFT stop $41.05RestingProtection, good until canceled");

    await openTab(page, "Activity");
    const main = page.getByRole("main");
    await expect(main).toContainText("Bracket placed for 6 MSFT: stop $41.05.");
    await expect(main).toContainText("Filled buy 6 MSFT at $44.62.");
    await expect(main).toContainText("You approved buy 6 MSFT at $44.62.");
    await expect(main).toContainText("Mandate version 1 confirmed by you and deployed to paper, with $3,000.00 of simulated money.");

    await openTab(page, "Decisions");
    await expect(main).toContainText("Approved by you; submitted and filled.");
  });

  await test.step("Stop: the agent's kill switch sells its position and ends it", async () => {
    await dock(page).getByRole("button", { name: "Stop", exact: true }).click();
    const sheet = page.locator("[data-slot=stop-sheet]");
    await sheet.getByRole("link", { name: /Kill switch: close and stop/ }).click();
    await expect(page).toHaveURL(`${agentHref}/kill-switch`);
    await expect(page.locator("[data-slot=record]")).toContainText("MSFT");
    await page.getByRole("button", { name: /Activate the kill switch/ }).click();
    await page.getByRole("dialog", { name: "Confirm it is you" }).getByRole("button", { name: "Use passkey" }).click();
    const after = page.locator("[data-slot=after-confirm]");
    await expect(after.locator("[data-phase=recorded]")).toBeVisible();
    await expect(after).toContainText("Stopped");
  });

  await test.step("after the kill switch, the agent holds nothing and is stopped", async () => {
    await dock(page).getByRole("link", { name: "Agents" }).click();
    await page.locator(`a[href="${agentHref}"]`).first().click();
    await expect(page).toHaveURL(agentHref);
    const main = page.getByRole("main");
    await expect(main.locator("[data-slot=mode-badge][data-mode=stopped]").filter({ visible: true })).toHaveText("Stopped");
    await openTab(page, "Activity");
    await expect(main).toContainText(/Kill switch at [0-9:]+: 1 order canceled; 1 position sold \(fixture fills\)\. Agent stopped\./);
  });
});
