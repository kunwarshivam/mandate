import { type Locator, type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "../src/fixtures/workspace";
import { AGENT_SECTIONS, SCREENS, SECTION_INDEX, agentHref } from "../src/lib/screens";

/**
 * DEC-200: flat fills only. Kumo paints linear gradients and masks in several components, and
 * `src/app/placard-kumo.css` flattens them. jsdom computes no styles, so these checks run in
 * Chromium against the production build.
 */

const CONNECTION = buildWorkspace("normal").connection.connection_id;

const ROUTES = [
  ...new Set([
    ...SCREENS.map((s) => s.href),
    SECTION_INDEX.audit.href,
    SECTION_INDEX.workspace.href,
    ...AGENT_SECTIONS.map((s) => agentHref(AGENT_IDS.swing, s.key)),
    agentHref(AGENT_IDS.btc, "overview"),
    `/approvals/${APPROVAL_IDS.swingXyz}`,
    recordHref("kill", AGENT_IDS.btc),
    recordHref("release", AGENT_IDS.btc),
    recordHref("stop_all", CONNECTION),
    recordHref("close_all", CONNECTION),
    "/design",
    "/no-such-page",
  ]),
];

const PAINT = ["background-image", "mask-image", "-webkit-mask-image", "border-image-source", "list-style-image"];

/** Every element and rendered pseudo-element whose computed paint contains a gradient. */
async function gradientsIn(page: Page, root = "body"): Promise<string[]> {
  return page.evaluate(
    ({ props, root }) => {
      const hits: string[] = [];
      const scope = document.querySelector(root);
      if (!scope) return [`no ${root}`];
      const label = (el: Element) => `<${el.tagName.toLowerCase()} class="${(el.getAttribute("class") ?? "").slice(0, 80)}">`;
      for (const el of [scope, ...scope.querySelectorAll("*")]) {
        for (const pseudo of [null, "::before", "::after"]) {
          const style = getComputedStyle(el, pseudo);
          if (pseudo && (style.content === "none" || style.content === "normal")) continue;
          for (const prop of props) {
            const value = style.getPropertyValue(prop);
            if (/gradient/i.test(value)) hits.push(`${label(el)}${pseudo ?? ""} ${prop}: ${value.slice(0, 100)}`);
          }
        }
      }
      return hits;
    },
    { props: PAINT, root },
  );
}

async function computed(locator: Locator, props: string[], pseudo: "::before" | "::after" | null = null): Promise<Record<string, string>> {
  return locator.evaluate(
    (el, { props, pseudo }) => {
      const style = getComputedStyle(el, pseudo);
      return Object.fromEntries(props.map((p) => [p, style.getPropertyValue(p)]));
    },
    { props, pseudo },
  );
}

/** A colour token as the browser resolves it, for comparing against a computed fill. */
async function tokenColor(page: Page, token: string): Promise<string> {
  return page.evaluate((token) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = `var(${token})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return value;
  }, token);
}

/** Next hydrates after load; a press before then does nothing, so retry until it opens. */
async function openBy(page: Page, trigger: Locator, opened: Locator) {
  await expect(async () => {
    await trigger.click();
    await expect(opened).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
}

test.describe("no gradient paints on any route (DEC-200)", () => {
  for (const path of ROUTES) {
    test(`desktop ${path}`, async ({ page }) => {
      await page.goto(path);
      await page.waitForLoadState("networkidle");
      expect(await gradientsIn(page)).toEqual([]);
    });
  }

  test.describe("phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });
    for (const path of ROUTES) {
      test(`phone ${path}`, async ({ page }) => {
        await page.goto(path);
        await page.waitForLoadState("networkidle");
        expect(await gradientsIn(page)).toEqual([]);
      });
    }
  });
});

test.describe("no gradient paints in any overlay (DEC-200)", () => {
  for (const path of ["/", agentHref(AGENT_IDS.btc, "overview")]) {
    test(`the Stop sheet at ${path}`, async ({ page }) => {
      await page.goto(path);
      await openBy(page, page.getByRole("button", { name: "Stop", exact: true }), page.getByRole("dialog"));
      expect(await gradientsIn(page)).toEqual([]);
    });
  }

  test("the passkey check on the kill-switch record screen", async ({ page }) => {
    await page.goto(recordHref("kill", AGENT_IDS.btc));
    await openBy(page, page.getByRole("button", { name: /Activate the kill switch/ }), page.getByRole("dialog", { name: "Confirm it is you" }));
    expect(await gradientsIn(page)).toEqual([]);
  });

  test("the command palette", async ({ page }) => {
    await page.goto("/");
    await openBy(page, page.getByRole("button", { name: /^Go to/ }), page.getByRole("dialog"));
    expect(await gradientsIn(page)).toEqual([]);
  });

  test.describe("phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });
    test("the sidebar sheet", async ({ page }) => {
      await page.goto(agentHref(AGENT_IDS.btc, "overview"));
      await page.waitForLoadState("networkidle");
      const sidebar = page.getByRole("navigation", { name: "Main" }).filter({ hasText: "Account" });
      await expect(sidebar).toBeHidden();
      await page.getByRole("button", { name: /^(Expand|Collapse) sidebar$/ }).first().click();
      await expect(sidebar).toBeVisible();
      expect(await gradientsIn(page)).toEqual([]);
    });
  });
});

test.describe("Kumo surfaces are flat (DEC-200)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/design");
    await page.waitForLoadState("networkidle");
  });

  test("primary Button and LinkButton: the emphasis overlay is a solid brand fill, with no shadow", async ({ page }) => {
    const brand = await tokenColor(page, "--color-kumo-brand");
    const buttons = page.locator("[data-specimen=button] [data-kumo-component]");
    await expect(buttons).toHaveCount(2);
    for (const button of await buttons.all()) {
      expect(await computed(button, ["background-image"])).toEqual({ "background-image": "none" });
      const overlay = button.locator(':scope > span[aria-hidden="true"].absolute');
      await expect(overlay).toHaveCount(1);
      expect(await overlay.getAttribute("class")).toContain("bg-linear-to-b");
      expect(await computed(overlay, ["background-image", "background-color", "box-shadow"])).toEqual({
        "background-image": "none",
        "background-color": brand,
        "box-shadow": "none",
      });
    }
  });

  test("destructive-styled Button: Kumo's destructive classes and danger emphasis still paint a flat overlay", async ({ page }) => {
    const classes = process.env.KUMO_DESTRUCTIVE_BUTTON_CLASSES ?? "";
    expect(classes).toContain("bg-(--kumo-button-emphasis-bg)");
    await page.evaluate((classes) => {
      const source = document.querySelector('[data-specimen=button] button[data-kumo-component="Button"]')!;
      const clone = source.cloneNode(true) as HTMLElement;
      clone.className = classes;
      clone.dataset.e2e = "destructive";
      const token = "var(--color-kumo-danger)";
      clone.style.setProperty("--kumo-button-emphasis-ring", `color-mix(in oklch, ${token}, black 10%)`);
      clone.style.setProperty("--kumo-button-emphasis-bg", `color-mix(in oklch, ${token}, white 30%)`);
      clone.style.setProperty("--kumo-button-emphasis-gradient-start", `color-mix(in oklch, ${token}, white 15%)`);
      clone.style.setProperty("--kumo-button-emphasis-gradient-end", token);
      source.parentElement!.append(clone);
    }, classes);
    const button = page.locator("[data-e2e=destructive]");
    expect(await computed(button, ["background-image"])).toEqual({ "background-image": "none" });
    const overlay = button.locator(':scope > span[aria-hidden="true"].absolute');
    expect(await computed(overlay, ["background-image", "box-shadow"])).toEqual({ "background-image": "none", "box-shadow": "none" });
    expect(await gradientsIn(page, "[data-specimen=button]")).toEqual([]);
  });

  test("Table with a sticky header and sticky columns: no fade on any cell", async ({ page }) => {
    const table = page.locator("[data-specimen=table]");
    await table.evaluate((el) => el.scrollTo({ left: 120, top: 60 }));
    const sticky = await table.locator("td, th").evaluateAll((cells) =>
      cells
        .filter((c) => getComputedStyle(c).position === "sticky")
        .map((c) => [getComputedStyle(c).backgroundImage, getComputedStyle(c, "::before").backgroundImage, getComputedStyle(c, "::after").backgroundImage]),
    );
    const headers = await table.locator("thead th").evaluateAll((cells) => cells.map((c) => getComputedStyle(c).position));
    expect(headers.every((p) => p === "sticky")).toBe(true);
    expect(sticky.length).toBeGreaterThan(headers.length);
    for (const paints of sticky) expect(paints).toEqual(["none", "none", "none"]);
    expect(await gradientsIn(page, "[data-specimen=table]")).toEqual([]);
  });

  test("Tabs that overflow: no mask on the list and no fade on its scroll buttons", async ({ page }) => {
    const list = page.locator("[data-specimen=tabs] [data-overflowing]");
    await expect(list).toHaveCount(1);
    expect(await computed(list, ["mask-image", "-webkit-mask-image"])).toEqual({ "mask-image": "none", "-webkit-mask-image": "none" });
    expect(await gradientsIn(page, "[data-specimen=tabs]")).toEqual([]);
  });

  test("Sidebar: no scroll mask on its content", async ({ page }) => {
    await page.goto("/");
    const masked = page.locator('[data-sidebar] [class*="mask-image"], [class*="mask-image"]');
    expect(await masked.count()).toBeGreaterThan(0);
    for (const el of await masked.all()) expect(await computed(el, ["mask-image", "-webkit-mask-image"])).toEqual({ "mask-image": "none", "-webkit-mask-image": "none" });
  });

  test("skeletons: a flat fill, with no shimmer and no animation", async ({ page }) => {
    const lines = page.locator("[data-specimen=skeleton] .skeleton-line");
    await expect(lines).toHaveCount(2);
    for (const line of await lines.all()) {
      expect(await computed(line, ["background-image", "animation-name"])).toEqual({ "background-image": "none", "animation-name": "none" });
      expect(await computed(line, ["content", "background-image", "animation-name"], "::after")).toEqual({ content: "none", "background-image": "none", "animation-name": "none" });
      expect(await line.evaluate((el) => el.getAnimations({ subtree: true }).length)).toBe(0);
    }
    const muted = await tokenColor(page, "--muted");
    expect((await computed(lines.first(), ["background-color"]))["background-color"]).toBe(muted);
  });

  test("LayerDialog: no mask on its scrolling body and a flat primary action", async ({ page }) => {
    const dialog = page.getByRole("dialog");
    await openBy(page, page.getByRole("button", { name: "Open a layer dialog" }), dialog);
    const masked = page.locator('[role=dialog] [class*="mask-image"], [role=dialog][class*="mask-image"]');
    expect(await masked.count()).toBeGreaterThan(0);
    for (const el of await masked.all()) expect(await computed(el, ["mask-image", "-webkit-mask-image"])).toEqual({ "mask-image": "none", "-webkit-mask-image": "none" });
    const primary = dialog.locator("[data-kumo-component=Button]").filter({ hasText: "Save" });
    const overlay = primary.locator(':scope > span[aria-hidden="true"].absolute');
    expect(await computed(overlay, ["background-image", "background-color"])).toEqual({
      "background-image": "none",
      "background-color": await tokenColor(page, "--color-kumo-brand"),
    });
    expect(await gradientsIn(page)).toEqual([]);
  });
});
