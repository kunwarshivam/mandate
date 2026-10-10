import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BETA_REQUEST_PATH, LOGIN_PATH } from "@/lib/auth-routes";
import { DISCARDED, QUESTIONS } from "../apps";
import { artwork } from "../art";
import { Landing } from "../landing";
import { BROKERS, GAP_CAVEAT, LongPage } from "./long-page";
import { PUSH_TEXT } from "./lock-screen";
import { PAGE_FORM_ID, PARTS, SIGN_UP_ID } from "./parts";
import { SHOTS } from "./shot";
import { WONT } from "./wont";

/**
 * The long page under the desktop (DEC-907): a landing page in its own type that shows the app and
 * says what the desktop's homepage does not, with Sign in and Sign up always in its bar.
 */

const WEB = join(__dirname, "..", "..", "..", "..");
const SCROLL_DIR = __dirname;
const SOURCES = readdirSync(SCROLL_DIR).filter((f) => /\.(tsx?|css)$/.test(f) && !/\.test\./.test(f) && f !== "app/globals.css");
const BLEND = new RegExp(["grad", "ient|bg-(linear|radial|conic)-"].join(""), "i");

const text = (el: Element) => (el.textContent ?? "").replace(/\s+/g, " ").trim();

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("the long page's structure", () => {
  it("gives every part a section labelled by its h2", () => {
    render(<LongPage />);
    for (const id of PARTS) {
      const section = document.getElementById(id);
      expect(section?.tagName, id).toBe("SECTION");
      const heading = within(section!).getAllByRole("heading", { level: 2 })[0]!;
      expect(section!.getAttribute("aria-labelledby")).toBe(heading.id);
    }
  });

  it("lists none of the parts in the bar: only the logo back to the desktop, Sign in and Sign up (DEC-908)", () => {
    render(<LongPage />);
    const bar = document.querySelector<HTMLElement>("[data-slot=page-bar]")!;
    expect(within(bar).queryByRole("navigation")).toBeNull();
    expect([...bar.querySelectorAll("a")].map((a) => a.getAttribute("href"))).toEqual(["#desktop", LOGIN_PATH]);
    expect(within(bar).getAllByRole("button").map((b) => b.textContent)).toEqual(["Sign up"]);
  });

  it("links within the page only to ids that exist, and back up to the desktop", () => {
    const { container } = render(
      <>
        <div id="desktop" />
        <LongPage />
      </>,
    );
    for (const a of container.querySelectorAll<HTMLAnchorElement>("a[href^='#']")) {
      expect(document.getElementById(a.getAttribute("href")!.slice(1)), a.getAttribute("href")!).not.toBeNull();
    }
  });

  it("uses h2 for every heading, under the desktop's one h1", () => {
    const { container } = render(<LongPage />);
    expect(container.querySelector("h1, h3, h4")).toBeNull();
    expect(container.querySelectorAll("h2").length).toBeGreaterThanOrEqual(PARTS.length + 2);
  });

  it("brings its own footer with the terms the page is offered on", () => {
    render(<LongPage />);
    const footer = screen.getByRole("contentinfo");
    expect(footer).toHaveTextContent("It is software, not investment advice. Trading involves risk, and you can lose money.");
    expect(footer).toHaveTextContent("© 2026 Owlhead");
  });
});

describe("the bar", () => {
  it("names Sign in and Sign up in words, and Sign in goes to the sign-in page", () => {
    render(<LongPage />);
    const bar = document.querySelector<HTMLElement>("[data-slot=page-bar]")!;
    expect(within(bar).getByRole("link", { name: "Sign in" }).getAttribute("href")).toBe(LOGIN_PATH);
    expect(within(bar).getByRole("button", { name: "Sign up" })).toBeTruthy();
  });

  it("sticks to the top of the sheet, so the two stay in view once the desktop is covered", () => {
    render(<LongPage />);
    expect(document.querySelector("[data-slot=page-bar]")!.className).toMatch(/\bsticky\b.*\btop-0\b/);
  });

  it("brings the request form into view on Sign up and puts the cursor in its email field", () => {
    const into = vi.spyOn(Element.prototype, "scrollIntoView");
    render(<LongPage />);
    fireEvent.click(within(document.querySelector<HTMLElement>("[data-slot=page-bar]")!).getByRole("button", { name: "Sign up" }));
    expect(into.mock.contexts[0]).toBe(document.getElementById(SIGN_UP_ID));
    expect(document.activeElement).toBe(document.getElementById(`${PAGE_FORM_ID}-email`));
  });
});

describe("what it says", () => {
  /** Every sentence the page says in its own words: its headings, leads and the won't list. */
  function said(container: HTMLElement): string[] {
    const own = [...container.querySelectorAll("h2, [data-slot=part] p, [data-slot=intro] p, [data-slot=wont] li > span:last-child")]
      .map(text)
      .filter((t) => t && t !== GAP_CAVEAT);
    return own.flatMap((t) => t.split(/(?<=[.?])\s+/)).filter((s) => s.split(" ").length > 3);
  }

  it("repeats nothing the desktop's homepage, its questions or its Recycle Bin already say", () => {
    const { container } = render(<LongPage />);
    const sentences = said(container);
    expect(sentences.length).toBeGreaterThan(10);
    cleanup();
    render(<Landing />);
    fireEvent.click(screen.getByRole("tab", { name: "Owlhead Home Page" }));
    const desktop = [text(document.querySelector("[data-slot=landing-page]")!), ...QUESTIONS.map((q) => q.a), ...DISCARDED.map((d) => d.why)].join(" ");
    for (const s of sentences) expect(desktop, s).not.toContain(s);
  });

  it("sells what the visitor gets and leaves the machinery to the app and the desktop (DEC-908)", () => {
    const { container } = render(<LongPage />);
    const pitch = said(container).join(" ");
    for (const term of ["limit order", "safe point", "headroom", "passkey", "regular session", "buying power", "margin", "checks", "thread", "mode", "Nothing more"]) {
      expect(pitch, term).not.toMatch(new RegExp(`\\b${term}\\b`, "i"));
    }
  });

  it("says losses can pass a limit, beside the limits", () => {
    render(<LongPage />);
    expect(document.getElementById("limits")).toHaveTextContent(GAP_CAVEAT);
  });

  it("says under the first buttons that it is free in the beta and starts on paper money", () => {
    render(<LongPage />);
    const intro = document.querySelector<HTMLElement>("[data-slot=intro]")!;
    expect(within(intro).getByText("Free during the beta. It starts on paper money.")).toHaveAttribute("data-slot", "reassure");
  });

  it("marks the brokers that are not connected yet as coming, and Alpaca as paper", () => {
    render(<LongPage />);
    const brokers = document.querySelector<HTMLElement>("[data-slot=brokers]")!;
    expect(BROKERS.map((b) => `${b.name}: ${b.status}`)).toEqual(["Alpaca: Paper trading", "Robinhood: Coming", "Kraken Derivatives US: Coming"]);
    for (const b of BROKERS) expect(brokers).toHaveTextContent(`${b.name}${b.status}`);
  });

  it("puts on the lock screen only the sentence a request's push carries", () => {
    const worker = readFileSync(join(WEB, "public", "push-sw.js"), "utf8");
    expect(worker).toContain(`approval_needed: "${PUSH_TEXT}"`);
    render(<LongPage />);
    const lock = document.querySelector<HTMLElement>("[data-slot=lock-screen]")!;
    expect(lock).toHaveTextContent(PUSH_TEXT);
    expect(text(lock)).not.toMatch(/\$|BTC|XYZ|Buy|Sell/);
  });

  it("strikes through each thing it won't do, under It won't", () => {
    render(<LongPage />);
    const wont = document.getElementById("wont")!;
    expect(within(wont).getByRole("heading", { level: 2 })).toHaveTextContent("It won't");
    expect([...wont.querySelectorAll("s")].map(text)).toEqual(WONT.map((w) => w.verb));
  });

  it("states no performance and makes no promise of money", () => {
    const { container } = render(<LongPage />);
    const words = text(container);
    expect(words).not.toMatch(/\d\s?%/);
    expect(words).not.toMatch(/[+−-]\s?\$\d/);
    expect(words).not.toMatch(/\bP&L\b|\breturns?\b|\bgains?\b|\bperformance\b|profit|guarantee|make money|beat the market|risk[- ]free/i);
    expect(container.querySelector("blockquote, q, cite")).toBeNull();
  });

  it("uses no em dash, en dash, or exclamation mark, and never names the product Mandate", () => {
    const { container } = render(<LongPage />);
    expect(text(container)).not.toMatch(/[—–!]/);
    expect(text(container)).not.toMatch(/\bMandate\b/);
  });

  it("names no button exactly Stop (DEC-211)", () => {
    render(<LongPage />);
    expect(screen.queryByRole("button", { name: /^stop$/i })).toBeNull();
  });
});

describe("the pictures", () => {
  /** A PNG's width and height, from its header. */
  function pngSize(file: string): [number, number] {
    const bytes = readFileSync(file);
    return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)];
  }

  it("ships each picture of the app in light and dark, at twice the size it is drawn", () => {
    for (const [name, { width, height }] of Object.entries(SHOTS)) {
      for (const mode of ["light", "dark"]) {
        expect(pngSize(join(WEB, "public", "landing", `${name}-${mode}.png`)), `${name}-${mode}`).toEqual([width * 2, height * 2]);
      }
    }
  });

  it("draws no picture wider than it was taken, so the app's words read at their own size", () => {
    const { container } = render(<LongPage />);
    for (const shot of container.querySelectorAll<HTMLElement>("[data-slot=shot]")) {
      expect(shot.style.maxWidth, shot.dataset.shot).toBe(`${SHOTS[shot.dataset.shot as keyof typeof SHOTS].width}px`);
    }
  });

  it("describes every picture of the app, and draws each one", () => {
    const { container } = render(<LongPage />);
    const shots = [...container.querySelectorAll<HTMLElement>("[data-slot=shot]")];
    expect(new Set(shots.map((s) => s.dataset.shot))).toEqual(new Set(Object.keys(SHOTS)));
    for (const img of container.querySelectorAll("[data-slot=shot] img")) expect(img.getAttribute("alt")?.length ?? 0).toBeGreaterThan(20);
  });

  it("credits the painting behind the request form to the Met, linked to its page", () => {
    render(<LongPage />);
    const art = artwork("kanasawa-full-moon");
    const plate = document.getElementById(SIGN_UP_ID)!;
    const credit = within(plate).getByRole("link", { name: `${art.artist}, ${art.title}` });
    expect(credit.getAttribute("href")).toBe(art.url);
    expect(credit.parentElement).toHaveTextContent(/, the Met$/);
  });
});

describe("the request form", () => {
  it("asks for an email in the page's own type and posts it to the beta's request", async () => {
    const fetch = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetch);
    render(<LongPage />);
    const plate = document.getElementById(SIGN_UP_ID)!;
    fireEvent.change(within(plate).getByLabelText("Email address"), { target: { value: "owner@example.com" } });
    fireEvent.click(within(plate).getByRole("radio", { name: "Trading my own account" }));
    await act(async () => {
      fireEvent.click(within(plate).getByRole("button", { name: "Ask for a place" }));
    });
    expect(fetch).toHaveBeenCalledWith(BETA_REQUEST_PATH, expect.objectContaining({ method: "POST" }));
    const [, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toMatchObject({ email: "owner@example.com", role: "own" });
    expect(within(plate).getByRole("status")).toHaveTextContent("You're on the list.");
  });
});

describe("the design system", () => {
  it("draws no colour blends and no raw colours, in source or in the rendered classes", () => {
    for (const file of SOURCES) {
      const source = readFileSync(join(SCROLL_DIR, file), "utf8");
      expect(source, file).not.toMatch(BLEND);
      expect(source, file).not.toMatch(/oklch\(\s*[\d.]|rgba?\(\s*\d|hsla?\(\s*\d|#[0-9a-f]{3,8}\b/i);
    }
    const { container } = render(<LongPage />);
    for (const el of container.querySelectorAll("*")) expect(el.getAttribute("class") ?? "").not.toMatch(BLEND);
  });

  it("uses no font-sans, never goes above 600, writes no dark: class and keeps crimson for the kill switch", () => {
    const { container } = render(<LongPage />);
    expect(container.innerHTML).not.toMatch(/\bfont-sans\b|\bfont-(bold|extrabold|black)\b|\bdark:|crimson/);
  });

  it("moves only inside a reduced-motion check, ties the parallax to a check for scroll-driven animations, and hides a piece only once the page is moving", () => {
    const css = readFileSync(join(SCROLL_DIR, "scroll.module.css"), "utf8");
    const [before, after] = css.split("@media (prefers-reduced-motion: no-preference)");
    expect(before).not.toMatch(/\b(animation|transition)\s*:/);
    expect(before).not.toMatch(/data-reveal/);
    expect(after).toMatch(/@supports \(animation-timeline: scroll\(\)\)/);
    const hidden = after!.match(/^[^{}\n]*:not\(\[data-shown\]\)[^{}\n]*\{/gm) ?? [];
    expect(hidden.length).toBeGreaterThanOrEqual(3);
    for (const rule of hidden) expect(rule.trim(), "a hidden state needs the page marked as moving").toMatch(/^\.page\[data-motion="on"\] /);
  });

  it("fills one section with tide, the part about asking you, and uses tide nowhere outside the landing page", () => {
    const { container } = render(<LongPage />);
    const filled = [...container.querySelectorAll("[class]")].filter((el) => el.classList.contains("bg-tide"));
    expect(filled.map((el) => el.id)).toEqual(["asking"]);
    const src = join(WEB, "src");
    const users = (readdirSync(src, { recursive: true }) as string[])
      .filter((f) => /\.(tsx?|css)$/.test(f) && !/\.test\./.test(f) && f !== "app/globals.css")
      .filter((f) => /\b(bg|text|ring|border|fill|stroke)-tide\b|var\(--tide|"--tide"/.test(readFileSync(join(src, f), "utf8")))
      .sort();
    expect(users).toEqual([
      "components/site/scroll/lock-screen.tsx",
      "components/site/scroll/long-page.tsx",
      "components/site/scroll/parts.ts",
      "components/site/scroll/pixel-night.tsx",
      "components/site/scroll/pixel-sea.tsx",
      "components/site/scroll/scroll.module.css",
    ]);
  });
});

describe("the motion", () => {
  /** Stands in for every observer the page makes, and tells the one watching an element when it comes into view or leaves. */
  function observe() {
    type Report = (entries: { target: Element; isIntersecting: boolean }[]) => void;
    const seen: Element[] = [];
    const watcher = new Map<Element, Report>();
    class Observer {
      constructor(private readonly report: Report) {}
      observe(el: Element) {
        seen.push(el);
        watcher.set(el, this.report);
      }
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("IntersectionObserver", Observer);
    const tell = (el: Element, isIntersecting: boolean) => act(() => watcher.get(el)?.([{ target: el, isIntersecting }]));
    return { seen, show: (el: Element) => tell(el, true), hide: (el: Element) => tell(el, false) };
  }

  it("with motion reduced, leaves every piece where the server drew it", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce"), media: query, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
    const { seen } = observe();
    render(<LongPage />);
    expect(document.querySelector("[data-slot=long-page]")).not.toHaveAttribute("data-motion");
    expect(seen).toHaveLength(0);
  });

  it("with motion allowed, watches every piece and lets each settle once it comes into view", () => {
    const { seen, show } = observe();
    const { container } = render(<LongPage />);
    expect(container.querySelector("[data-slot=long-page]")).toHaveAttribute("data-motion", "on");
    expect(seen.filter((el) => el.matches("[data-reveal]")).length).toBe(container.querySelectorAll("[data-reveal]").length);
    const notification = container.querySelector("[data-slot=lock-screen] [data-reveal=drop]")!;
    expect(notification).not.toHaveAttribute("data-shown");
    show(notification);
    expect(notification).toHaveAttribute("data-shown");
  });

  it("with motion allowed, holds the sea's and the night's loops still while each is out of view, and lets them go as it comes back", () => {
    const { seen, show, hide } = observe();
    const { container } = render(<LongPage />);
    const pictures = [...container.querySelectorAll<HTMLElement>("[data-ambient]")];
    expect(pictures.map((p) => p.dataset.slot).sort()).toEqual(["pixel-night", "pixel-sea"]);
    expect(seen.filter((el) => el.matches("[data-ambient]"))).toEqual(pictures);
    for (const picture of pictures) {
      hide(picture);
      expect(picture.dataset.ambient, picture.dataset.slot).toBe("paused");
      show(picture);
      expect(picture.dataset.ambient, picture.dataset.slot).toBe("playing");
    }
  });

  it("pauses only the looping animations of a picture out of view, never the moon's rise, which follows the scroll", () => {
    const css = readFileSync(join(__dirname, "scroll.module.css"), "utf8");
    const motion = css.slice(css.indexOf("@media (prefers-reduced-motion: no-preference)"));
    const rule = motion.match(/\[data-ambient="paused"\] :is\(([^)]*)\) \{\s*animation-play-state: paused;/);
    expect(rule, "a paused picture's loops hold still, with motion allowed").not.toBeNull();
    const held = rule![1]!.split(",").map((s) => s.trim());
    expect(held).toEqual(expect.arrayContaining([".wave", ".foam", ".glintA", ".glintB", ".wingsUp", ".wingsDown", ".twinkle"]));
    expect(held).not.toContain(".moon");
  });

  it("holds the desktop's loops and its app's still once the page has risen 200 px past covering it, and lets them go before it shows", () => {
    const { show } = observe();
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(900);
    const scrollTo = (y: number) => {
      vi.spyOn(window, "scrollY", "get").mockReturnValue(y);
      fireEvent.scroll(window);
    };
    const { unmount } = render(
      <>
        <section data-slot="desktop-stage">
          <iframe title="app" />
        </section>
        <LongPage />
      </>,
    );
    const desktop = document.querySelector<HTMLElement>("[data-slot=desktop-stage]")!;
    const frame = desktop.querySelector("iframe")!;
    const app = () => frame.contentDocument!.documentElement;
    const states = () => [desktop.dataset.ambient, app().dataset.ambient];
    expect(states()).toEqual(["playing", "playing"]);
    scrollTo(1099);
    expect(states(), "covered, but within 200 px").toEqual(["playing", "playing"]);
    scrollTo(1100);
    expect(states()).toEqual(["paused", "paused"]);
    show(document.querySelector("[data-slot=pixel-sea]")!);
    expect(states(), "a picture coming into view does not wake the desktop").toEqual(["paused", "paused"]);
    const root = app();
    frame.contentDocument!.removeChild(root);
    const errors: unknown[] = [];
    const caught = (e: ErrorEvent) => {
      errors.push(e.error);
      e.preventDefault();
    };
    window.addEventListener("error", caught);
    scrollTo(1200);
    window.removeEventListener("error", caught);
    expect(errors, "an app part way through loading has no root yet").toEqual([]);
    delete root.dataset.ambient;
    frame.contentDocument!.appendChild(root);
    fireEvent.load(frame);
    expect(app().dataset.ambient, "an app that loads again while covered is held at once").toBe("paused");
    scrollTo(1099);
    expect(states()).toEqual(["playing", "playing"]);
    unmount();
    expect(desktop).not.toHaveAttribute("data-ambient");
    expect(app()).not.toHaveAttribute("data-ambient");

    const held = /animation-play-state: paused !important;/;
    const page = readFileSync(join(__dirname, "scroll.module.css"), "utf8");
    const stageRule = page.slice(0, page.indexOf("@media")).match(/\.stage\[data-ambient="paused"\] :is\(\*, ::before, ::after\) \{([^}]*)\}/);
    expect(stageRule?.[1], "whatever motion the visitor asked for, and over each loop's shorthand").toMatch(held);
    const globals = readFileSync(join(WEB, "src", "app", "globals.css"), "utf8");
    expect(globals.match(/:root\[data-ambient="paused"\] :is\(\*, ::before, ::after\) \{([^}]*)\}/)?.[1], "the app's own document").toMatch(held);
  });

  it("strikes each thing it won't do in turn, and settles every picture of the app with a tilt", () => {
    const { container } = render(<LongPage />);
    const lines = [...container.querySelectorAll<HTMLElement>("#wont li")];
    expect(lines.map((li) => [li.dataset.reveal, li.style.getPropertyValue("--i")])).toEqual(WONT.map((_, i) => ["strike", String(i)]));
    for (const shot of container.querySelectorAll("[data-slot=shot]")) expect(shot.closest("[data-reveal=tilt]"), shot.getAttribute("data-shot")!).not.toBeNull();
  });
});
