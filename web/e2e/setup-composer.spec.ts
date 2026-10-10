import { type Page, expect, test } from "@playwright/test";

/**
 * Set up an agent asks, it does not look like it is loading (critique C-4): the composer carries a
 * visible label above the field that also names it, and the send button is flat, with no shadow and
 * no heavier foot that a round button draws as a spinner's arc. Still no placeholder and no example
 * (DEC-476, DEC-477). Checked at a phone's width and a desktop's, in the light and dark projects,
 * by what the browser draws (DEC-511).
 */

const WIDTHS = [390, 1440] as const;

const field = (page: Page) => page.getByRole("textbox", { name: "Your answer" });
/** The page's one text field, found without its name, so the button's check stands on its own. */
const anyField = (page: Page) => page.getByRole("main").getByRole("textbox");
const send = (page: Page) => page.getByRole("button", { name: "Send", exact: true });

/**
 * Whether a computed `box-shadow` draws anything: Tailwind composes a ring and a shadow into layers
 * that are all zero when off, so "none" and a list of zero-length layers both draw nothing.
 */
function drawsShadow(boxShadow: string): boolean {
  if (boxShadow === "none") return false;
  const layers = boxShadow.split(/,(?![^(]*\))/);
  return layers.some((layer) => (layer.replace(/\([^)]*\)/g, "").match(/-?\d*\.?\d+px/g) ?? []).some((length) => parseFloat(length) !== 0));
}

async function open(page: Page, width: number) {
  await page.setViewportSize({ width, height: width < 1024 ? 844 : 900 });
  await page.goto("/agents/new");
  await expect(page.getByRole("heading", { level: 1, name: "Set up an agent" })).toBeVisible();
}

for (const width of WIDTHS) {
  test.describe(`at ${width}px`, () => {
    test("the composer is named by a label the owner can read, above the field", async ({ page }) => {
      await open(page, width);
      await expect(field(page)).toBeVisible();
      await expect(field(page)).not.toHaveAttribute("placeholder");
      const drawn = await field(page).evaluate((el) => {
        const label = (el as HTMLTextAreaElement).labels?.[0];
        if (!label) return null;
        const box = label.getBoundingClientRect();
        const style = getComputedStyle(label);
        return {
          text: label.innerText.trim(),
          width: box.width,
          height: box.height,
          bottom: box.bottom,
          fieldTop: el.getBoundingClientRect().top,
          clipped: style.clipPath !== "none" || (style.clip !== "auto" && style.clip !== ""),
          hidden: style.visibility !== "visible" || Number(style.opacity) === 0,
        };
      });
      expect(drawn).not.toBeNull();
      expect(drawn!.text).toBe("Your answer");
      expect(drawn!.clipped).toBe(false);
      expect(drawn!.hidden).toBe(false);
      expect(drawn!.width).toBeGreaterThan(40);
      expect(drawn!.height).toBeGreaterThanOrEqual(12);
      expect(drawn!.bottom).toBeLessThanOrEqual(drawn!.fieldTop);
    });

    test("the send button is flat: no shadow and no heavier foot, and Enter still sends", async ({ page }) => {
      await open(page, width);
      await anyField(page).fill("$3,000");
      await expect(send(page)).toBeEnabled();
      const edge = await send(page).evaluate((el) => {
        const s = getComputedStyle(el);
        return { shadow: s.boxShadow, top: s.borderTopWidth, bottom: s.borderBottomWidth, topColor: s.borderTopColor, bottomColor: s.borderBottomColor };
      });
      expect(drawsShadow(edge.shadow), edge.shadow).toBe(false);
      expect(edge.bottom).toBe(edge.top);
      expect(edge.bottomColor).toBe(edge.topColor);
      await anyField(page).press("Enter");
      await expect(page.getByRole("log", { name: "Conversation" }).locator("[data-slot=owner-message]").last()).toHaveText("You: $3,000");
      await expect(anyField(page)).toBeFocused();
    });
  });
}
