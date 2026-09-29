import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PAIRS } from "@/lib/contrast-pairs";
import { contrastRatio } from "@/lib/color";
import { tokenValue } from "@/lib/tokens";
import { HEADLINE, SUBHEAD } from "./hero";
import { Landing } from "./landing";

/**
 * The landing page (DEC-212): real headings and landmarks, calls to action that go to sign-in, and
 * copy that makes no claim the docs do not support. No performance, no promises, compliance text
 * only as named placeholders.
 */

function renderLanding() {
  return render(<Landing />);
}

/** Everything a visitor can read, including image descriptions. */
function readable(container: HTMLElement): string {
  const alts = [...container.querySelectorAll("img")].map((img) => img.getAttribute("alt") ?? "");
  return [container.textContent ?? "", ...alts].join("\n");
}

/** Readable text with the placeholders taken out. */
function copyOutsidePlaceholders(container: HTMLElement): string {
  const clone = container.cloneNode(true) as HTMLElement;
  for (const tag of clone.querySelectorAll("[data-placeholder]")) tag.remove();
  return readable(clone);
}

describe("the landing page's structure", () => {
  it("has one h1, the headline, and the subhead under it", () => {
    renderLanding();
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveTextContent(HEADLINE);
    expect(screen.getByText(SUBHEAD)).toBeInTheDocument();
  });

  it.each(["How it works", "Why it's safe", "Who it's for", "Questions"])("has a section headed %s", (title) => {
    renderLanding();
    const heading = screen.getByRole("heading", { level: 2, name: title });
    expect(heading.closest("section")).toHaveAttribute("aria-labelledby", heading.id);
  });

  it.each(["Set the mandate", "The agent trades inside it", "You stay in charge"])("shows the step %s, in order, as a list", (title) => {
    renderLanding();
    const steps = within(screen.getByRole("region", { name: "How it works" })).getAllByRole("listitem");
    const titles = ["Set the mandate", "The agent trades inside it", "You stay in charge"];
    expect(steps.map((li) => within(li).getByRole("heading", { level: 3 }).textContent)).toEqual(titles.map((t, i) => `${i + 1}${t}`));
    expect(screen.getByRole("heading", { level: 3, name: new RegExp(title) })).toBeInTheDocument();
  });

  it.each(["Is this real money?", "Which brokers does it work with?", "Can the agent go past my limits?", "What happens if something breaks?", "Is this investment advice?"])(
    "answers %s",
    (q) => {
      renderLanding();
      const term = screen.getByText(q);
      expect(term.tagName).toBe("DT");
      expect(term.nextElementSibling?.tagName).toBe("DD");
      expect(term.nextElementSibling?.textContent?.trim()).not.toBe("");
    },
  );

  it("brings its own main landmark and footer, and a named site navigation", () => {
    const { container } = renderLanding();
    expect(screen.getAllByRole("main")).toHaveLength(1);
    expect(screen.getByRole("main")).toHaveAttribute("id", "main");
    expect(screen.getByRole("contentinfo")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Site" })).toBeInTheDocument();
    expect(container.querySelectorAll("header")).toHaveLength(0);
  });

  it("puts every heading level in order, with no level skipped", () => {
    const { container } = renderLanding();
    const levels = [...container.querySelectorAll("h1, h2, h3, h4, h5, h6")].map((h) => Number(h.tagName[1]));
    levels.forEach((level, i) => {
      if (i > 0) expect(level - levels[i - 1]).toBeLessThanOrEqual(1);
    });
  });
});

describe("calls to action", () => {
  it("sends Get started and Sign in to /login", () => {
    renderLanding();
    expect(screen.getByRole("link", { name: "Get started" })).toHaveAttribute("href", "/login");
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
  });

  it("links How it works and Questions to sections that exist", () => {
    const { container } = renderLanding();
    const anchors = screen.getAllByRole("link").filter((a) => a.getAttribute("href")?.startsWith("#"));
    expect(anchors.map((a) => a.getAttribute("href")).sort()).toEqual(["#faq", "#how-it-works", "#how-it-works"]);
    for (const a of anchors) expect(container.querySelector(a.getAttribute("href")!)).not.toBeNull();
  });

  it("links nowhere else: no invented legal pages", () => {
    renderLanding();
    const hrefs = screen.getAllByRole("link").map((a) => a.getAttribute("href"));
    expect(hrefs.filter((h) => !h?.startsWith("#"))).toEqual(["/login", "/login"]);
  });
});

describe("the copy", () => {
  it("states no performance: no percentages, signed amounts, returns or P&L", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).not.toMatch(/\d\s?%/);
    expect(text).not.toMatch(/[+−-]\s?\$\d/);
    expect(text).not.toMatch(/\bP&L\b|\breturns?\b|\bgains?\b|\byields?\b|\bperformance\b|\boutperform/i);
  });

  it("never names the product Mandate", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).not.toMatch(/\bMandate\b/);
    expect(text).toMatch(/\bOwlhead\b/);
  });

  it("makes no promise of money: no profit, guarantee, earning, or beating the market", () => {
    const { container } = renderLanding();
    expect(readable(container)).not.toMatch(/profit|guarantee|\bearn|beat the market|risk[- ]free|revolutioni[sz]e/i);
  });

  it("has no testimonials and no quotes attributed to people", () => {
    const { container } = renderLanding();
    expect(container.querySelector("blockquote, q, cite")).toBeNull();
  });

  it("says paper, and says live trading is not available", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).toContain("Paper trading only. Live trading isn't available.");
    expect(text).not.toMatch(/\bwaitlist\b|coming soon|launching/i);
  });

  it("uses no em dash, en dash, or exclamation mark", () => {
    const { container } = renderLanding();
    expect(readable(container)).not.toMatch(/[—–!]/);
  });
});

describe("compliance text", () => {
  it("appears only as named placeholders", () => {
    const { container } = renderLanding();
    const tags = [...container.querySelectorAll("[data-placeholder]")];
    expect(tags.map((t) => t.getAttribute("data-placeholder")).sort()).toEqual(["notAdvice", "privacy", "siteDisclaimer", "terms"]);
    expect(copyOutsidePlaceholders(container)).not.toContain("[[");
  });

  it("answers the investment-advice question with the placeholder and nothing else", () => {
    renderLanding();
    const answer = screen.getByText("Is this investment advice?").nextElementSibling as HTMLElement;
    expect(answer.textContent?.trim()).toBe("[[NOT-INVESTMENT-ADVICE]]");
  });

  it("writes no disclaimer of its own", () => {
    const { container } = renderLanding();
    expect(copyOutsidePlaceholders(container)).not.toMatch(/not (financial|investment) advice|past results|consult (a|your)/i);
  });

  it("puts the disclaimer and the copyright in the footer", () => {
    renderLanding();
    const footer = screen.getByRole("contentinfo");
    expect(footer.querySelector("[data-placeholder=siteDisclaimer]")).toHaveTextContent("[[SITE-DISCLAIMER]]");
    expect(footer).toHaveTextContent("© 2026 Owlhead");
  });
});

describe("the screenshots", () => {
  it("each has a description, a size, and a dark version for a dark system", () => {
    const { container } = renderLanding();
    const pictures = [...container.querySelectorAll("picture[data-slot=screenshot]")];
    expect(pictures.length).toBeGreaterThanOrEqual(7);
    for (const picture of pictures) {
      const img = picture.querySelector("img")!;
      expect(img.getAttribute("alt")?.length).toBeGreaterThan(20);
      expect(Number(img.getAttribute("width"))).toBeGreaterThan(0);
      expect(Number(img.getAttribute("height"))).toBeGreaterThan(0);
      const source = picture.querySelector("source")!;
      expect(source).toHaveAttribute("media", "(prefers-color-scheme: dark)");
      expect(source.getAttribute("srcset")).toContain("-dark.png");
      expect(img.getAttribute("srcset")).toContain("-light.png");
    }
  });

  it("loads the hero's screens first and every other screen lazily", () => {
    const { container } = renderLanding();
    for (const img of container.querySelectorAll("picture[data-slot=screenshot] img")) {
      const hero = /hero-/.test(img.getAttribute("srcset") ?? "");
      expect(img).toHaveAttribute("loading", hero ? "eager" : "lazy");
      if (hero) expect(img).toHaveAttribute("fetchpriority", "high");
    }
  });
});

const SITE_DIR = __dirname;
const BLEND = new RegExp(["grad", "ient|bg-(linear|radial|conic)-"].join(""), "i");

describe("the design system", () => {
  it("draws no colour blends, in source or in the rendered classes and styles", () => {
    const sources = readdirSync(SITE_DIR).filter((f) => /\.(tsx?|css)$/.test(f) && !/\.test\./.test(f));
    for (const file of sources) expect(readFileSync(join(SITE_DIR, file), "utf8"), file).not.toMatch(BLEND);
    const { container } = renderLanding();
    for (const el of container.querySelectorAll("*")) {
      expect(el.getAttribute("class") ?? "").not.toMatch(BLEND);
      expect(el.getAttribute("style") ?? "").not.toMatch(BLEND);
    }
  });

  it("uses only the app's weights: never bold", () => {
    const { container } = renderLanding();
    expect(container.innerHTML).not.toMatch(/\bfont-(bold|extrabold|black)\b/);
  });

  it("uses no card fills: sections are hairlines and whitespace", () => {
    const { container } = renderLanding();
    expect(container.innerHTML).not.toMatch(/\bshadow-(sm|md|lg|xl|2xl)\b|(?<![\w:-])bg-(muted|background|lapis-soft|mandate)(?![\w-])/);
  });

  it("uses no raw colours", () => {
    const sources = readdirSync(SITE_DIR).filter((f) => /\.(tsx?|css)$/.test(f) && !/\.test\./.test(f));
    for (const file of sources) expect(readFileSync(join(SITE_DIR, file), "utf8"), file).not.toMatch(/oklch\(|rgba?\(|hsla?\(|#[0-9a-f]{3,8}\b/i);
  });
});

/** The colour pairs the landing page puts text on; each is one of the app's tested pairs. */
const LANDING_PAIRS = [
  { fg: "foreground", bg: "card", use: "Headings, the ledger, links" },
  { fg: "muted-foreground", bg: "card", use: "The subhead, step and answer text, captions, placeholders" },
  { fg: "mandate-strong", bg: "card", use: "Step numbers, and the focus ring" },
  { fg: "primary-foreground", bg: "primary", use: "Get started" },
  { fg: "primary-foreground", bg: "lapis-strong", use: "Get started, hovered" },
  { fg: "foreground", bg: "background", use: "How it works, hovered" },
] as const;

describe("contrast", () => {
  it.each(LANDING_PAIRS.map((p) => [p.fg, p.bg] as const))("%s on %s is one of the app's tested text pairs", (fg, bg) => {
    expect(PAIRS.some((p) => p.fg === fg && p.bg === bg && p.kind !== "mark")).toBe(true);
  });

  it.each((["light", "dark"] as const).flatMap((theme) => LANDING_PAIRS.map((p) => [theme, p.fg, p.bg] as const)))("%s: %s on %s reaches WCAG AA (4.5:1)", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(4.5);
  });
});
