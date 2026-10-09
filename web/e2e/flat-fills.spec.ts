import { type Locator, type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "../src/fixtures/workspace";
import { AGENT_SECTIONS, SCREENS, SECTION_INDEX, agentHref, decisionHref, orderHref, positionHref } from "../src/lib/screens";

/**
 * DEC-200: flat fills only. Kumo paints linear gradients and masks in several components, and
 * `src/app/kumo-theme.css` flattens them. jsdom computes no styles, so these checks run in
 * Chromium against the production build.
 */

const WS = buildWorkspace("normal");
const CONNECTION = WS.connection.connection_id;
const SWING = WS.agents.find((a) => a.agent_id === AGENT_IDS.swing)!;
const BTC = WS.agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
const XYZ = SWING.positions.find((p) => p.instrument.symbol === "XYZ")!;

const ROUTES = [
  ...new Set([
    ...SCREENS.map((s) => s.href),
    SECTION_INDEX.audit.href,
    SECTION_INDEX.workspace.href,
    ...AGENT_SECTIONS.map((s) => agentHref(AGENT_IDS.swing, s.key)),
    agentHref(AGENT_IDS.btc, "overview"),
    `/approvals/${APPROVAL_IDS.swingXyz}`,
    positionHref(AGENT_IDS.swing, XYZ.instrument.asset_id),
    `${positionHref(AGENT_IDS.swing, XYZ.instrument.asset_id)}/close`,
    positionHref(AGENT_IDS.btc, BTC.positions[0].instrument.asset_id),
    orderHref(AGENT_IDS.swing, SWING.orders[0].client_order_id),
    orderHref(AGENT_IDS.swing, SWING.past_orders[0].client_order_id),
    decisionHref(AGENT_IDS.swing, WS.decisions.find((d) => d.agent_id === AGENT_IDS.swing)!.event_id),
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

/** The markup Kumo renders for each emphasis button, from `e2e/global-setup.mjs`. */
function emphasisMarkup(): { primaryButton: string; primaryLink: string; destructiveButton: string } {
  return JSON.parse(process.env.KUMO_EMPHASIS_MARKUP ?? "{}");
}

/**
 * Mounts a Kumo button's markup in the specimen, under the page's theme, and reads its paint: the
 * button's fill, each overlay span's class and paint, and every gradient on the button, its children
 * and their pseudo-elements. React may re-render the specimen and drop a foreign node, so the button
 * is appended, read and removed in one synchronous call.
 */
async function mountKumo(page: Page, markup: string) {
  return page.evaluate(
    ({ markup, paint }) => {
      const host = document.createElement("div");
      host.innerHTML = markup;
      document.querySelector("[data-specimen=button]")!.append(host);
      const button = host.firstElementChild!;
      const read = (el: Element, props: string[]) => {
        const style = getComputedStyle(el);
        return Object.fromEntries(props.map((p) => [p, style.getPropertyValue(p)]));
      };
      const gradients = [button, ...button.querySelectorAll("*")].flatMap((el) =>
        [null, "::before", "::after"].flatMap((pseudo) => {
          const style = getComputedStyle(el, pseudo);
          return paint.map((p) => style.getPropertyValue(p)).filter((v) => /gradient/i.test(v));
        }),
      );
      const result = {
        button: read(button, ["background-image"]),
        overlays: [...button.querySelectorAll(':scope > span[aria-hidden="true"].absolute')].map((el) => ({
          className: el.getAttribute("class") ?? "",
          style: read(el, ["background-image", "background-color", "box-shadow"]),
        })),
        gradients,
      };
      host.remove();
      return result;
    },
    { markup, paint: PAINT },
  );
}

/** Saturated volt: the mandate's rules, markers and labels and the account's line. Never a block, and in light mode never text. */
const SATURATED_VOLT = ["--mandate-strong", "--mandate-marker", "--mandate-edge", "--lapis-line"];
const MARK_VOLT = ["--mandate-marker", "--mandate-edge", "--lapis-line"];
/** A rail is 8 px tall and a post, tick or bar a few px wide: anything thicker on both sides is a block. */
const VOLT_MAX_THICKNESS = 8;

/** Elements painted in saturated volt thicker than a line, and text set in volt's mark colours. */
async function voltMisuse(page: Page): Promise<string[]> {
  return page.evaluate(
    ({ fills, marks, max }) => {
      const resolve = (token: string) => {
        const probe = document.createElement("div");
        probe.style.color = `var(${token})`;
        document.body.append(probe);
        const value = getComputedStyle(probe).color;
        probe.remove();
        return value;
      };
      const fill = new Set(fills.map(resolve));
      const mark = new Set(marks.map(resolve));
      const label = (el: Element) => `<${el.tagName.toLowerCase()} class="${(el.getAttribute("class") ?? "").slice(0, 80)}">`;
      const hits: string[] = [];
      for (const el of document.body.querySelectorAll("*")) {
        for (const pseudo of [null, "::before", "::after"]) {
          const style = getComputedStyle(el, pseudo);
          if (pseudo && (style.content === "none" || style.content === "normal")) continue;
          if (!fill.has(style.backgroundColor)) continue;
          const box = pseudo ? { width: parseFloat(style.width), height: parseFloat(style.height) } : el.getBoundingClientRect();
          if (Math.min(box.width, box.height) > max) hits.push(`${label(el)}${pseudo ?? ""} is a ${Math.round(box.width)}x${Math.round(box.height)} volt block`);
        }
        const text = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim());
        if (text && mark.has(getComputedStyle(el).color)) hits.push(`${label(el)} sets text in a volt mark colour: ${el.textContent?.slice(0, 40)}`);
      }
      return hits;
    },
    { fills: SATURATED_VOLT, marks: MARK_VOLT, max: VOLT_MAX_THICKNESS },
  );
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

test.describe("volt is a line, never a block, and never text lighter than deep volt (DEC-205)", () => {
  for (const path of ROUTES) {
    test(`desktop ${path}`, async ({ page }) => {
      await page.goto(path);
      await page.waitForLoadState("networkidle");
      expect(await voltMisuse(page)).toEqual([]);
    });
  }

  test.describe("phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });
    for (const path of ROUTES) {
      test(`phone ${path}`, async ({ page }) => {
        await page.goto(path);
        await page.waitForLoadState("networkidle");
        expect(await voltMisuse(page)).toEqual([]);
      });
    }
  });
});

test("renders in the project's theme, with the body on the theme's card colour", async ({ page }, testInfo) => {
  const mode = testInfo.project.use.colorScheme === "dark" ? "dark" : "light";
  await page.goto("/");
  await expect(page.locator("html")).toHaveAttribute("data-mode", mode);
  expect(await page.locator("html").evaluate((el) => el.classList.contains("dark"))).toBe(mode === "dark");
  expect((await computed(page.locator("body"), ["background-color"]))["background-color"]).toBe(await tokenColor(page, "--card"));
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
    await openBy(page, page.locator("[data-slot=command-bar]"), page.getByRole("dialog"));
    expect(await gradientsIn(page)).toEqual([]);
  });

  test.describe("phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });
    test("the More sheet", async ({ page }) => {
      await page.goto(agentHref(AGENT_IDS.btc, "overview"));
      await page.waitForLoadState("networkidle");
      await openBy(page, page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" }), page.getByRole("dialog", { name: "More" }));
      expect(await gradientsIn(page)).toEqual([]);
    });
  });
});

test.describe("Kumo surfaces are flat (DEC-200)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/design");
    await page.waitForLoadState("networkidle");
  });

  test("the specimen's Button and LinkButton are the key: a flat fill with no gradient (DEC-469)", async ({ page }) => {
    const buttons = page.locator("[data-specimen=button] [data-kumo-component]");
    await expect(buttons).toHaveCount(2);
    for (const button of await buttons.all()) {
      expect(await computed(button, ["background-image"])).toEqual({ "background-image": "none" });
    }
    expect(await gradientsIn(page, "[data-specimen=button]")).toEqual([]);
  });

  test("primary Button and LinkButton: the emphasis overlay is a solid brand fill, with no shadow", async ({ page }) => {
    const brand = await tokenColor(page, "--color-kumo-brand");
    const { primaryButton, primaryLink } = emphasisMarkup();
    for (const markup of [primaryButton, primaryLink]) {
      const found = await mountKumo(page, markup);
      expect(found.button).toEqual({ "background-image": "none" });
      expect(found.overlays, "Kumo still paints one overlay span on an emphasis button").toHaveLength(1);
      expect(found.overlays[0].className, "and still draws it as a gradient, which the theme must flatten").toContain("bg-linear-to-b");
      expect(found.overlays[0].style).toEqual({ "background-image": "none", "background-color": brand, "box-shadow": "none" });
      expect(found.gradients).toEqual([]);
    }
  });

  test("destructive-styled Button: Kumo's destructive classes and danger emphasis still paint a flat overlay", async ({ page }) => {
    const classes = process.env.KUMO_DESTRUCTIVE_BUTTON_CLASSES ?? "";
    expect(classes).toContain("bg-(--kumo-button-emphasis-bg)");
    const { destructiveButton } = emphasisMarkup();
    expect(destructiveButton, "the markup carries Kumo's destructive classes").toContain(classes);
    expect(destructiveButton, "and its danger emphasis").toContain("--kumo-button-emphasis-gradient-end:var(--color-kumo-danger)");
    const found = await mountKumo(page, destructiveButton);
    expect(found.button).toEqual({ "background-image": "none" });
    expect(found.overlays).toHaveLength(1);
    expect(found.overlays[0].style).toMatchObject({ "background-image": "none", "box-shadow": "none" });
    expect(found.gradients).toEqual([]);
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

  test("phone frame and More sheet: no scroll mask anywhere", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    await expect(page.locator("[data-sidebar]"), "no sidebar on a phone (DEC-207)").toHaveCount(0);
    await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" }).click();
    await expect(page.getByRole("dialog", { name: "More" })).toBeVisible();
    for (const el of await page.locator("[data-slot=more-sheet], [data-slot=more-sheet] *, nav[aria-label=Main]").all()) {
      expect(await computed(el, ["mask-image", "-webkit-mask-image"])).toEqual({ "mask-image": "none", "-webkit-mask-image": "none" });
    }
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
