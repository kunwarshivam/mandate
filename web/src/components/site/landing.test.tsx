import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { contrastRatio } from "@/lib/color";
import { tokenValue } from "@/lib/tokens";
import { HEADLINE, Landing, QUESTIONS, SECTIONS, SUBHEAD } from "./landing";
import { EDITED, TRACE } from "./record-trace";

/**
 * The landing page (DEC-213): one homepage set as the web looked in the late 1990s. Real headings and
 * landmarks behind the browser scenery, a guestbook that asks for a place in the private beta, and
 * copy that makes no claim of performance and no promise.
 */

function renderLanding() {
  return render(<Landing />);
}

function readable(container: HTMLElement): string {
  return container.textContent ?? "";
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the landing page's structure", () => {
  it("has one h1 named Owlhead, with the block letters hidden from assistive technology", () => {
    const { container } = renderLanding();
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveAccessibleName(HEADLINE);
    expect(h1s[0].querySelector("[aria-hidden]")?.textContent).toMatch(/_{4}/);
    expect(container).toHaveTextContent(SUBHEAD);
  });

  it("numbers every section and labels it by its heading", () => {
    renderLanding();
    const titles = [...SECTIONS.map((s) => s.title), "Questions", "Ask for a place"];
    titles.forEach((title, i) => {
      const heading = screen.getByRole("heading", { level: 2, name: `${i + 1}. ${title}` });
      expect(heading.closest("section")).toHaveAttribute("aria-labelledby", heading.id);
    });
  });

  it("lists every section in the contents, and each link lands on its heading", () => {
    const { container } = renderLanding();
    const contents = screen.getByRole("navigation", { name: "Contents" });
    const links = within(contents).getAllByRole("link");
    expect(links).toHaveLength(SECTIONS.length + 2);
    for (const a of links) expect(container.querySelector(a.getAttribute("href")!)?.tagName).toBe("H2");
  });

  it("sends the browser's guide buttons to sections that exist", () => {
    const { container } = renderLanding();
    const guides = within(screen.getByRole("navigation", { name: "Guides" })).getAllByRole("link");
    expect(guides.map((a) => a.textContent)).toEqual(["What's New?", "What's Cool?", "Handbook", "Questions"]);
    for (const a of guides) expect(container.querySelector(a.getAttribute("href")!)).not.toBeNull();
  });

  it("hides the browser's scenery: menus, toolbar and status bar are not in the accessibility tree", () => {
    const { container } = renderLanding();
    const browser = container.querySelector("[data-slot=browser]")!;
    for (const word of ["Bookmarks", "Reload", "Document: Done"]) {
      const el = within(browser as HTMLElement).getByText((_, node) => node?.tagName === "SPAN" && node.textContent === word);
      expect(el.closest("[aria-hidden=true]"), word).not.toBeNull();
    }
    expect(within(browser as HTMLElement).getByText("http://www.owlhead.ai/").closest("[aria-hidden=true]")).toBeNull();
  });

  it("answers every question as a term and its description", () => {
    renderLanding();
    for (const { q } of QUESTIONS) {
      const term = screen.getByText(q);
      expect(term.tagName).toBe("DT");
      expect(term.nextElementSibling?.tagName).toBe("DD");
    }
  });

  it("brings one main landmark and a footer", () => {
    renderLanding();
    expect(screen.getAllByRole("main")).toHaveLength(1);
    expect(screen.getByRole("main")).toHaveAttribute("id", "main");
    expect(screen.getByRole("contentinfo")).toBeInTheDocument();
  });

  it("puts every heading level in order, with no level skipped", () => {
    const { container } = renderLanding();
    const levels = [...container.querySelectorAll("h1, h2, h3, h4, h5, h6")].map((h) => Number(h.tagName[1]));
    levels.forEach((level, i) => {
      if (i > 0) expect(level - levels[i - 1]).toBeLessThanOrEqual(1);
    });
  });
});

describe("links", () => {
  it("asks for a place from the top of the page, and signs in at /login", () => {
    renderLanding();
    expect(screen.getAllByRole("link", { name: "Ask for a place" })[0]).toHaveAttribute("href", "#beta");
    for (const link of screen.getAllByRole("link", { name: "Sign in" })) expect(link).toHaveAttribute("href", "/login");
  });

  it("offers the guestbook as a button in the hero and again after who it's for", () => {
    renderLanding();
    const buttons = screen.getAllByRole("link", { name: "Sign the guestbook" });
    expect(buttons).toHaveLength(2);
    for (const a of buttons) expect(a).toHaveAttribute("href", "#beta");
    expect(buttons[0].closest("header")).not.toBeNull();
    expect(buttons[1].closest("section")).toHaveAttribute("aria-labelledby", "who");
  });

  it("echoes the address of the link under the pointer in the status bar, as a browser of the time did", () => {
    const { container } = renderLanding();
    const status = container.querySelector("[data-slot=status-text]")!;
    const link = within(screen.getByRole("navigation", { name: "Contents" })).getByRole("link", { name: "How it works" });
    fireEvent.pointerOver(link);
    expect(status).toHaveTextContent("http://www.owlhead.ai/#how");
    fireEvent.pointerOut(link);
    expect(status).toHaveTextContent("Document: Done");
    fireEvent.focusIn(screen.getAllByRole("link", { name: "Sign in" })[0]);
    expect(status).toHaveTextContent("http://www.owlhead.ai/login");
  });

  it("links nowhere off the page but sign-in: no invented legal pages", () => {
    renderLanding();
    const hrefs = screen.getAllByRole("link").map((a) => a.getAttribute("href"));
    expect(new Set(hrefs.filter((h) => !h?.startsWith("#")))).toEqual(new Set(["/login"]));
  });
});

describe("the guestbook", () => {
  function fill(email: string) {
    fireEvent.change(screen.getByLabelText("Email address:"), { target: { value: email } });
  }

  it("posts the email and the chosen use to /api/beta, then says you're on the list", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    renderLanding();
    fill("ada@example.com");
    fireEvent.click(screen.getByLabelText("Managing money for others"));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign the guestbook" }));
    });
    expect(fetchMock).toHaveBeenCalledWith("/api/beta", expect.objectContaining({ method: "POST" }));
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({ email: "ada@example.com", role: "clients", website: "" });
    expect(document.querySelector("[data-slot=beta-done]")).toHaveTextContent("You're on the list.");
    expect(document.querySelector("[data-slot=beta-done]")).toHaveTextContent("ada@example.com");
  });

  it("asks you to check the email when the server says it's wrong", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "email" }), { status: 400 })));
    renderLanding();
    fill("ada@example");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign the guestbook" }));
    });
    expect(screen.getByLabelText("Email address:")).toHaveAttribute("aria-invalid", "true");
    expect(document.getElementById("beta-problem")).toHaveTextContent("That email doesn't look right.");
    expect(document.getElementById("beta-problem")).toHaveAttribute("data-slot", "beta-problem");
  });

  it("says to try again when the request can't be saved or sent", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));
    renderLanding();
    fill("ada@example.com");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign the guestbook" }));
    });
    expect(document.getElementById("beta-problem")).toHaveTextContent("We couldn't save that just now.");
    expect(screen.getByRole("button", { name: "Sign the guestbook" })).toBeEnabled();
  });

  it("carries a field people never see, for bots to fill", () => {
    renderLanding();
    const trap = document.getElementById("beta-website")!;
    expect(trap).toHaveAttribute("tabindex", "-1");
    expect(trap.closest("[aria-hidden=true]")).not.toBeNull();
  });
});

describe("the record", () => {
  it("shows one decision, every line with its hash, and a chain that matches", () => {
    renderLanding();
    const table = screen.getByRole("table");
    expect(within(table).getAllByRole("row")).toHaveLength(TRACE.length + 1);
    for (const e of TRACE) expect(within(table).getByText(e.hash)).toBeInTheDocument();
    expect(screen.getByText(/all 8 lines match/)).toBeInTheDocument();
  });

  it("breaks the chain from an edited line down, and mends it on undo", () => {
    renderLanding();
    fireEvent.click(screen.getByRole("button", { name: `Edit line ${EDITED.index + 1}` }));
    expect(screen.getByText(EDITED.text)).toBeInTheDocument();
    expect(screen.getByText(/fails at line 3\. Lines 3 to 8 no longer match/)).toBeInTheDocument();
    expect(screen.getAllByText("no match")).toHaveLength(TRACE.length - EDITED.index - 1);
    fireEvent.click(screen.getByRole("button", { name: "Undo the edit" }));
    expect(screen.queryByText(EDITED.text)).toBeNull();
    expect(screen.getByText(/all 8 lines match/)).toBeInTheDocument();
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
    expect(readable(container)).not.toMatch(/profit|guarantee|\bearn(s|ings?)? (money|returns|income)|make money|beat the market|risk[- ]free|revolutioni[sz]e/i);
  });

  it("has no testimonials and no quotes attributed to people", () => {
    const { container } = renderLanding();
    expect(container.querySelector("blockquote, q, cite")).toBeNull();
  });

  it("calls it a private beta on paper, with live trading waiting on legal sign-off", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).toContain("Private beta");
    expect(text).toContain("Paper trading on Alpaca, with simulated money");
    expect(text).toContain("Live trading, once it has legal sign-off");
    expect(text).not.toMatch(/\bwaitlist\b|coming soon|launching/i);
  });

  it("says losses can pass a limit", () => {
    const { container } = renderLanding();
    expect(readable(container)).toContain("a loss can end up larger than the limit");
  });

  it("uses no em dash, en dash, or exclamation mark", () => {
    const { container } = renderLanding();
    expect(readable(container)).not.toMatch(/[—–!]/);
  });
});

describe("the terms the page is offered on", () => {
  it("are plain words, with no placeholder left anywhere", () => {
    const { container } = renderLanding();
    expect(readable(container)).not.toContain("[[");
    expect(container.querySelector("[data-placeholder]")).toBeNull();
  });

  it("answer the investment-advice question with a no", () => {
    renderLanding();
    expect(screen.getByText("Is this investment advice?").nextElementSibling?.textContent).toMatch(/^No\. Owlhead is software/);
  });

  it("sit in the footer with the copyright", () => {
    renderLanding();
    const footer = screen.getByRole("contentinfo");
    expect(footer).toHaveTextContent("It is software, not investment advice. Trading involves risk, and you can lose money.");
    expect(footer).toHaveTextContent("© 2026 Owlhead");
  });
});

const SITE_DIR = __dirname;
const SOURCES = readdirSync(SITE_DIR).filter((f) => /\.(tsx?|css)$/.test(f) && !/\.test\./.test(f));
const BLEND = new RegExp(["grad", "ient|bg-(linear|radial|conic)-"].join(""), "i");

describe("the design system", () => {
  it("draws no colour blends, in source or in the rendered classes and styles", () => {
    for (const file of SOURCES) expect(readFileSync(join(SITE_DIR, file), "utf8"), file).not.toMatch(BLEND);
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

  it("uses no raw colours: every colour is a token, so dark mode is the same page", () => {
    for (const file of SOURCES) expect(readFileSync(join(SITE_DIR, file), "utf8"), file).not.toMatch(/oklch\(|rgba?\(|hsla?\(|#[0-9a-f]{3,8}\b/i);
  });

  it("keeps crimson for the kill switch", () => {
    const { container } = renderLanding();
    expect(container.innerHTML).not.toMatch(/crimson/);
  });
});

/** The colour pairs the landing page sets text in. */
const LANDING_PAIRS = [
  { fg: "foreground", bg: "card", use: "The page's text and headings" },
  { fg: "muted-foreground", bg: "card", use: "The date line and the footer" },
  { fg: "mandate-strong", bg: "card", use: "Links" },
  { fg: "foreground", bg: "muted", use: "The contents frame, the chrome and the guestbook" },
  { fg: "mandate-strong", bg: "muted", use: "Links in the contents frame" },
  { fg: "highlight-foreground", bg: "highlight", use: "The guestbook buttons, the New tag, a hovered link and the sun badge" },
  { fg: "card", bg: "foreground", use: "Title bars, the record's column heads and the ink badges" },
  { fg: "foreground", bg: "warning-soft", use: "The edited line of the record" },
] as const;

describe("contrast", () => {
  it.each((["light", "dark"] as const).flatMap((theme) => LANDING_PAIRS.map((p) => [theme, p.fg, p.bg] as const)))("%s: %s on %s reaches WCAG AA (4.5:1)", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(4.5);
  });
});
