import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WORDMARK_PATH } from "@/components/brand/Logo";
import { DEMO_PATH, DEMO_SOURCE } from "@/components/demo/demo-messages";
import { contrastRatio } from "@/lib/color";
import { tokenValue } from "@/lib/tokens";
import { HEADLINE, Landing, SECTIONS, SUBHEAD, WINDOWS } from "./landing";
import { DISCARDED, QUESTIONS } from "./apps";
import { WALLPAPERS, randomWallpaper } from "./art";
import { APP_ORIGIN } from "./app-tab";
import { GREETING, TIPS } from "./assistant";
import { BODY, MONO, PIXEL } from "./letter";
import { PLAYLIST } from "./music";
import { TOUR, TOUR_VIDEO, tourVtt } from "./tour";
import { WallpaperProvider } from "./wallpaper";
import { EDITED, TRACE } from "./record-trace";

/**
 * The landing page (DEC-213): one homepage set as the web looked in the late 1990s. Real headings and
 * landmarks behind the browser scenery, a guestbook that asks for a place in the private beta, and
 * copy that makes no claim of performance and no promise.
 */

/** The landing page with its home page in front; the app's tab opens first, and has tests of its own. */
function renderLanding() {
  const result = render(<Landing />);
  fireEvent.click(screen.getByRole("tab", { name: "Owlhead Home Page" }));
  return result;
}

function readable(container: HTMLElement): string {
  return container.textContent ?? "";
}

const frame = () => new Promise((r) => requestAnimationFrame(r));

async function press(el: HTMLElement) {
  await act(async () => {
    fireEvent.click(el);
    await frame();
  });
}

const win = (name: string) => screen.getByRole("region", { name });
const zOf = (el: HTMLElement) => Number(el.style.zIndex);
const icon = (name: string) => within(screen.getByRole("list", { name: "Desktop" })).getByRole("button", { name });
const home = () => document.querySelector<HTMLElement>("[data-slot=landing-page]")!;

const RECORD = "The record - Example decision";
const HELP = "Questions - Owlhead Help";
const GUESTBOOK = "guestbook.cgi";

const TITLE_OF: Record<string, string> = { record: RECORD, questions: HELP, guestbook: GUESTBOOK };

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the landing page's structure", () => {
  it("has one h1 named Owlhead, with the product's owl and wordmark beside it hidden from assistive technology", () => {
    const { container } = renderLanding();
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveAccessibleName(HEADLINE);
    const mark = h1s[0].querySelector<HTMLElement>("[data-slot=wordmark]");
    expect(mark).toHaveAttribute("aria-hidden", "true");
    expect(mark?.querySelector("[data-slot=brand-owl]"), "the brand owl leads").not.toBeNull();
    expect(mark?.querySelector("[data-slot=owlhead-wordmark] path")).toHaveAttribute("d", WORDMARK_PATH);
    expect(mark?.style.color, "in the logo's colour, as the app's header has it").toBe("var(--logo)");
    expect(container).toHaveTextContent(SUBHEAD);
  });

  it("numbers every section and labels it by its heading", () => {
    renderLanding();
    SECTIONS.forEach(({ title }, i) => {
      const heading = screen.getByRole("heading", { level: 2, name: `${i + 1}. ${title}` });
      expect(heading.closest("section")).toHaveAttribute("aria-labelledby", heading.id);
    });
    expect(screen.getAllByRole("heading", { level: 2 }).filter((h) => /^\d+\. /.test(h.textContent ?? ""))).toHaveLength(SECTIONS.length);
  });

  it("lists every section in the contents, and each link lands on its heading", () => {
    const { container } = renderLanding();
    const contents = screen.getByRole("navigation", { name: "Contents" });
    const links = within(contents).getAllByRole("link");
    expect(links).toHaveLength(SECTIONS.length);
    for (const a of links) expect(container.querySelector(a.getAttribute("href")!)?.tagName).toBe("H2");
  });

  it("opens the record, the questions and the guestbook in their own windows from the contents", async () => {
    renderLanding();
    const contents = screen.getByRole("navigation", { name: "Contents" });
    expect(within(contents).getAllByRole("button").map((b) => b.textContent)).toEqual(WINDOWS.map((w) => w.title));
    for (const { app, title } of WINDOWS) {
      await press(within(screen.getByRole("navigation", { name: "Contents" })).getByRole("button", { name: title }));
      expect(win(TITLE_OF[app])).toHaveAttribute("data-front", "true");
    }
  });

  it("sends the browser's guides to sections that exist, or opens the window that holds the rest", async () => {
    const { container } = renderLanding();
    const guides = () => screen.getByRole("navigation", { name: "Guides" });
    expect(within(guides()).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["What's New?", "What's Cool?", "Handbook", "Questions"]);
    const links = within(guides()).getAllByRole("link");
    expect(links.map((a) => a.textContent)).toEqual(["What's New?", "Handbook"]);
    for (const a of links) expect(container.querySelector(a.getAttribute("href")!)?.tagName).toBe("H2");
    await press(within(guides()).getByRole("button", { name: "What's Cool?" }));
    expect(win(RECORD)).toHaveAttribute("data-front", "true");
    await press(within(guides()).getByRole("button", { name: "Questions" }));
    expect(win(HELP)).toHaveAttribute("data-front", "true");
  });

  it("keeps the record, the questions and the guestbook out of the home page's document", () => {
    renderLanding();
    const page = home();
    for (const id of ["record", "questions", "beta"]) expect(document.getElementById(id), id).toBeNull();
    expect(page.querySelector("[data-slot=record-trace], [data-slot=beta-form], table, form")).toBeNull();
    for (const { q } of QUESTIONS) expect(page).not.toHaveTextContent(q);
    expect(page).not.toHaveTextContent("Here is one decision from start to finish.");
  });

  it("links within the page only to ids that exist", () => {
    const { container } = renderLanding();
    const hashes = [...container.querySelectorAll("a[href^='#']")].map((a) => a.getAttribute("href")!);
    expect(hashes.length).toBeGreaterThan(0);
    for (const h of hashes) expect(document.getElementById(h.slice(1)), h).not.toBeNull();
  });

  it("server-renders the guestbook form inside the page for a visitor without scripts", () => {
    const html = renderToString(<Landing />);
    const fallback = html.match(/<noscript>(.*?)<\/noscript>/s)?.[1] ?? "";
    expect(fallback).toContain('id="beta-noscript-email"');
    expect(fallback).toContain("Sign the guestbook");
    expect(html.indexOf("<noscript>")).toBeLessThan(html.indexOf('data-window="guestbook"'));
  });

  it("greys out the toolbar's buttons that can do nothing for this page, and Home and Reload work", async () => {
    renderLanding();
    const tools = screen.getByRole("toolbar", { name: "Browser" });
    const buttons = within(tools).getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Back", "Forward", "Home", "Reload", "Images", "Open", "Print", "Find", "Stop"]);
    expect(buttons.filter((b) => !(b as HTMLButtonElement).disabled).map((b) => b.textContent)).toEqual(["Home", "Reload"]);
    expect(within(tools).getByRole("button", { name: "Stop loading" })).toBeDisabled();
    expect(within(tools).queryByRole("button", { name: "Stop" })).toBeNull();
    for (const b of buttons) expect(b, b.textContent ?? "").toHaveAttribute("tabindex", "-1");
    const root = document.querySelector<HTMLElement>("[data-scroll-root]")!;
    const scrollTo = vi.fn();
    root.scrollTo = scrollTo;
    await press(within(tools).getByRole("button", { name: "Home" }));
    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: 0 }));
  });

  it("opens a real menu from each menu title that has lines, greys out the rest, and every line goes somewhere", async () => {
    const { container } = renderLanding();
    const bar = screen.getByRole("menubar", { name: "Browser menus" });
    const titles = within(bar).getAllByRole("menuitem").filter((m) => m.parentElement?.getAttribute("role") === "none" || m.parentElement === bar);
    expect(titles.map((t) => t.textContent)).toEqual(["File", "Edit", "View", "Go", "Bookmarks", "Options", "Directory", "Window", "Help"]);
    expect(titles.filter((t) => t.getAttribute("aria-disabled") === "true").map((t) => t.textContent)).toEqual(["File", "Edit", "Options"]);
    for (const t of titles.filter((t) => t.getAttribute("aria-disabled") !== "true")) {
      await press(t);
      const menu = screen.getByRole("menu", { name: t.textContent! });
      expect(t).toHaveAttribute("aria-expanded", "true");
      for (const line of within(menu).getAllByRole("menuitem")) {
        if (line.getAttribute("aria-disabled") === "true") continue;
        const href = line.getAttribute("href");
        if (href) expect(container.querySelector(href)?.tagName, `${t.textContent} › ${line.textContent}`).toBe("H2");
        else expect(line.tagName, `${t.textContent} › ${line.textContent}`).toBe("BUTTON");
      }
      await press(t);
      expect(screen.queryByRole("menu")).toBeNull();
    }
  });

  it("lists the page's sections under Bookmarks and opens a desktop window from the Window menu", async () => {
    renderLanding();
    const bar = screen.getByRole("menubar", { name: "Browser menus" });
    await press(within(bar).getByRole("menuitem", { name: "Bookmarks" }));
    expect(within(screen.getByRole("menu", { name: "Bookmarks" })).getAllByRole("menuitem").map((m) => m.textContent)).toEqual(SECTIONS.map((s) => s.title));
    await press(within(bar).getByRole("menuitem", { name: "Window" }));
    await press(within(screen.getByRole("menu", { name: "Window" })).getByRole("menuitem", { name: "The record" }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(win(RECORD)).toHaveAttribute("data-front", "true");
  });

  it("closes an open menu on Escape and gives the focus back to its title", async () => {
    renderLanding();
    const go = within(screen.getByRole("menubar", { name: "Browser menus" })).getByRole("menuitem", { name: "Go" });
    await press(go);
    const menu = screen.getByRole("menu", { name: "Go" });
    expect(document.activeElement).toHaveTextContent("Home");
    expect(within(menu).getByRole("menuitem", { name: "Back" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(go);
  });

  it("shows the address in a read-only field, and keeps only the status bar as scenery", () => {
    const { container } = renderLanding();
    const address = screen.getByRole("textbox", { name: "Address" });
    expect(address).toHaveValue("http://www.owlhead.ai/");
    expect(address).toHaveAttribute("readonly");
    const browser = container.querySelector<HTMLElement>("[data-slot=browser]")!;
    const status = within(browser).getByText((_, node) => node?.tagName === "SPAN" && node.textContent === "Document: Done");
    expect(status.closest("[aria-hidden=true]")).not.toBeNull();
  });

  it("switches dark mode from the taskbar's tray, pressed in while on, and saves the app's own theme choice", async () => {
    const root = document.documentElement;
    root.dataset.mode = "light";
    renderLanding();
    const toggle = screen.getByRole("button", { name: "Dark" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    await act(async () => fireEvent.click(toggle));
    expect(root.dataset.mode).toBe("dark");
    expect(document.cookie).toContain("owlhead-theme=dark");
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    await act(async () => fireEvent.click(toggle));
    expect(root.dataset.mode).toBe("light");
    expect(document.cookie).toContain("owlhead-theme=light");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("answers every question as a term and its description, in the Questions window", async () => {
    renderLanding();
    await press(icon("Questions"));
    const help = win(HELP);
    expect(help).toHaveAttribute("data-front", "true");
    for (const { q } of QUESTIONS) {
      const term = within(help).getByText(q);
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
  it("signs in at /login", () => {
    renderLanding();
    for (const link of screen.getAllByRole("link", { name: "Sign in" })) expect(link).toHaveAttribute("href", "/login");
  });

  it("offers the guestbook as a button in the hero and again after who it's for, each opening its window", async () => {
    renderLanding();
    const buttons = screen.getAllByRole("button", { name: "Sign the guestbook" });
    expect(buttons).toHaveLength(2);
    expect(buttons[0].closest("[data-slot=hero-actions]")).not.toBeNull();
    expect(buttons[1].closest("section")).toHaveAttribute("aria-labelledby", "who");
    for (const b of buttons) {
      expect(screen.queryByRole("region", { name: GUESTBOOK })).toBeNull();
      await press(b);
      expect(win(GUESTBOOK)).toHaveAttribute("data-front", "true");
      expect(within(win(GUESTBOOK)).getByLabelText("Email address:")).toBeInTheDocument();
      await press(screen.getByRole("button", { name: `Close ${GUESTBOOK}` }));
    }
  });

  it("puts See why it traded beside Sign the guestbook in the hero, and it opens the record's window", async () => {
    renderLanding();
    const hero = document.querySelector<HTMLElement>("[data-slot=hero-actions]")!;
    expect(within(hero).getAllByRole("button").map((b) => b.textContent)).toEqual(["Sign the guestbook", "See why it traded", "Look inside the app"]);
    const see = within(hero).getByRole("button", { name: "See why it traded" });
    expect(see.className).toBe(within(hero).getByRole("button", { name: "Sign the guestbook" }).className);
    expect(screen.queryByRole("region", { name: RECORD })).toBeNull();
    await press(see);
    const record = win(RECORD);
    expect(record).toHaveAttribute("data-front", "true");
    expect(zOf(record)).toBeGreaterThan(zOf(win("Owlhead Home Page")));
    expect(within(record).getByRole("table")).toBeInTheDocument();
  });

  it("echoes the address of the link under the pointer in the status bar, as a browser of the time did", () => {
    const { container } = renderLanding();
    const status = container.querySelector("[data-slot=status-text]")!;
    const link = within(screen.getByRole("navigation", { name: "Contents" })).getByRole("link", { name: "How it works" });
    fireEvent.pointerOver(link);
    expect(status).toHaveTextContent("http://www.owlhead.ai/#how");
    fireEvent.pointerOut(link);
    expect(status).toHaveTextContent("Document: Done");
    const browser = container.querySelector<HTMLElement>("[data-slot=browser]")!;
    fireEvent.focusIn(within(browser).getAllByRole("link", { name: "Sign in" })[0]);
    expect(status).toHaveTextContent("http://www.owlhead.ai/login");
  });

  it("links nowhere off the page but sign-in and the Met's page for each picture: no invented legal pages", () => {
    const { container } = renderLanding();
    const hrefs = [...container.querySelectorAll("a[href]")].map((a) => a.getAttribute("href")!);
    const away = hrefs.filter((h) => !h.startsWith("#") && h !== "/login");
    expect(away.length).toBeGreaterThan(0);
    for (const h of away) expect(h).toMatch(/^https:\/\/www\.metmuseum\.org\/art\/collection\/search\/\d+$/);
  });
});

describe("the browser's tabs", () => {
  const tabs = () => screen.getByRole("tablist", { name: "Pages" });
  const address = () => screen.getByRole("textbox", { name: "Address" });
  const appFrame = () => screen.getByTitle("Owlhead app, example workspace") as HTMLIFrameElement;
  const tool = (name: string) => within(screen.getByRole("toolbar", { name: "Browser" })).getByRole("button", { name });
  const status = () => document.querySelector("[data-slot=status-text]")!;

  async function report(path: string, back: boolean, forward: boolean, from: MessageEventSource | null = appFrame().contentWindow, origin = window.location.origin) {
    await act(async () => window.dispatchEvent(new MessageEvent("message", { data: { source: DEMO_SOURCE, type: "at", path, back, forward }, origin, source: from })));
  }

  it("opens on the app itself, live in a frame on the example workspace, with the home page in the second tab", () => {
    render(<Landing />);
    const [app, site] = within(tabs()).getAllByRole("tab");
    expect(app).toHaveAccessibleName("Inside the app");
    expect(app).toHaveAttribute("aria-selected", "true");
    expect(site).toHaveAccessibleName("Owlhead Home Page");
    const appPanel = document.getElementById(app.getAttribute("aria-controls")!)!;
    expect(appPanel).toBeVisible();
    expect(appPanel).toHaveAccessibleName("Inside the app");
    expect(within(appPanel).getByTitle("Owlhead app, example workspace")).toHaveAttribute("src", DEMO_PATH);
    expect(document.getElementById(site.getAttribute("aria-controls")!)).not.toBeVisible();
    expect(home(), "the home page stays in the document while hidden").toBeInTheDocument();
    expect(address()).toHaveValue(`${APP_ORIGIN}/`);
    expect(address(), "the app's address takes a typed one").not.toHaveAttribute("readonly");
    expect(status()).toHaveTextContent("Contacting host: app.owlhead.ai…");
    expect(appFrame()).toHaveAttribute("data-ready", "false");
  });

  it("follows where the app goes, and hears only its own frame on this origin", async () => {
    render(<Landing />);
    await report("/agents", true, false);
    expect(address()).toHaveValue(`${APP_ORIGIN}/agents`);
    expect(appFrame()).toHaveAttribute("data-ready", "true");
    expect(status()).toHaveTextContent("Document: Done");
    expect(tool("Back")).toBeEnabled();
    expect(tool("Forward")).toBeDisabled();
    await report("/positions", false, true, window);
    await report("/positions", false, true, appFrame().contentWindow, "https://elsewhere.example");
    expect(address()).toHaveValue(`${APP_ORIGIN}/agents`);
  });

  it("sends the app back, forward, home, to a typed address, or to start over, and nowhere it has no path for", async () => {
    render(<Landing />);
    const sent = vi.fn();
    appFrame().contentWindow!.postMessage = sent;
    await report("/agents", true, true);
    const asked = () => sent.mock.calls.map(([data, origin]) => (expect(origin).toBe(window.location.origin), data));
    sent.mockClear();
    await press(tool("Back"));
    await press(tool("Forward"));
    await press(tool("Home"));
    await press(tool("Reload"));
    for (const typed of ["app.owlhead.ai/approvals", "https://app.owlhead.ai/alerts", "/positions", "https://elsewhere.example/x", "//elsewhere.example", "app.owlhead.ai.elsewhere.example/x"]) {
      fireEvent.change(address(), { target: { value: typed } });
      fireEvent.submit(address().closest("form")!);
    }
    expect(asked()).toEqual([
      { source: DEMO_SOURCE, type: "go", to: "back" },
      { source: DEMO_SOURCE, type: "go", to: "forward" },
      { source: DEMO_SOURCE, type: "open", path: "/" },
      { source: DEMO_SOURCE, type: "go", to: "restart" },
      { source: DEMO_SOURCE, type: "open", path: "/approvals" },
      { source: DEMO_SOURCE, type: "open", path: "/alerts" },
      { source: DEMO_SOURCE, type: "open", path: "/positions" },
    ]);
    fireEvent.change(address(), { target: { value: "half typed" } });
    fireEvent.keyDown(address(), { key: "Escape" });
    expect(address()).toHaveValue(`${APP_ORIGIN}/agents`);
  });

  it("dresses the app in the page's theme, and keeps it in step", async () => {
    render(<Landing />);
    const root = document.documentElement;
    root.dataset.mode = "light";
    const doc = appFrame().contentDocument!;
    const inside = doc.documentElement ?? doc.appendChild(doc.createElement("html"));
    await report("/", false, false);
    expect(inside.dataset.mode).toBe("light");
    await act(async () => {
      root.dataset.mode = "dark";
      root.classList.add("dark");
      await frame();
    });
    expect(inside.dataset.mode).toBe("dark");
    expect(inside).toHaveClass("dark");
    root.classList.remove("dark");
    root.dataset.mode = "light";
  });

  it("moves between the tabs with the arrow keys, and comes back to the home page from the hero, a guide, or the skip link", async () => {
    render(
      <>
        <a href="#main">Skip to content</a>
        <Landing />
      </>,
    );
    const site = within(tabs()).getByRole("tab", { name: "Owlhead Home Page" });
    const app = within(tabs()).getByRole("tab", { name: "Inside the app" });
    await press(screen.getByRole("link", { name: "Skip to content" }));
    expect(site).toHaveAttribute("aria-selected", "true");
    expect(home()).toBeVisible();
    expect(address()).toHaveValue("http://www.owlhead.ai/");
    expect(address(), "the home page's address is read-only").toHaveAttribute("readonly");
    await press(within(document.querySelector<HTMLElement>("[data-slot=hero-actions]")!).getByRole("button", { name: "Look inside the app" }));
    expect(app).toHaveAttribute("aria-selected", "true");
    expect(app, "the keyboard follows the tab").toHaveFocus();
    expect(app).toHaveAttribute("tabindex", "0");
    expect(site).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(app, { key: "ArrowRight" });
    expect(site).toHaveAttribute("aria-selected", "true");
    expect(site).toHaveFocus();
    fireEvent.keyDown(site, { key: "ArrowLeft" });
    expect(app).toHaveAttribute("aria-selected", "true");
    await press(within(screen.getByRole("navigation", { name: "Guides" })).getByRole("link", { name: "Handbook" }));
    expect(site).toHaveAttribute("aria-selected", "true");
    expect(home()).toBeVisible();
  });
});

describe("the desktop", () => {
  afterEach(() => localStorage.clear());

  it("opens on the home page's window, in front, with the page's one main inside it", () => {
    renderLanding();
    const home = win("Owlhead Home Page");
    expect(home).toBeVisible();
    expect(home).toHaveAttribute("data-front", "true");
    expect(within(home).getByRole("main")).toHaveAttribute("id", "main");
    const tasks = within(screen.getByRole("list", { name: "Open windows" })).getAllByRole("button");
    expect(tasks.map((b) => b.textContent)).toEqual(["Owlhead"]);
    expect(tasks[0]).toHaveAttribute("aria-pressed", "true");
  });

  it("minimizes a window to the taskbar, and brings it back from there", async () => {
    renderLanding();
    await press(screen.getByRole("button", { name: "Minimize Owlhead Home Page" }));
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
    const task = within(screen.getByRole("list", { name: "Open windows" })).getByRole("button", { name: "Owlhead" });
    expect(task).toHaveAttribute("aria-pressed", "false");
    await press(task);
    expect(win("Owlhead Home Page")).toBeVisible();
    expect(task).toHaveAttribute("aria-pressed", "true");
  });

  it("cascades each window it opens from the one in front, so a new window shows the last one rather than covering it; the home page is never cascaded from", async () => {
    renderLanding();
    const at = (name: string) => (win(name).style.translate || "0px 0px").split(" ").map(parseFloat);
    const home = at("Owlhead Home Page");
    await press(icon("The record"));
    const record = at(RECORD);
    await press(icon("Questions"));
    const help = at(HELP);
    await press(icon("Guestbook"));
    const guestbook = at(GUESTBOOK);
    for (const [front, next] of [
      [record, help],
      [help, guestbook],
    ]) {
      expect(next[0], "each new window sits right of the one in front").toBeGreaterThan(front[0]);
      expect(next[1], "and below it").toBeGreaterThan(front[1]);
    }
    const step = [help[0] - record[0], help[1] - record[1]];
    expect(record, "the home page is never cascaded from").not.toEqual([home[0] + step[0], home[1] + step[1]]);
    await press(icon("The record"));
    expect(at(RECORD), "a window brought forward stays where it was").toEqual(record);
  });

  it("maximizes a window to fill the desktop, and restores it", async () => {
    renderLanding();
    await press(screen.getByRole("button", { name: "Maximize Owlhead Home Page" }));
    expect(win("Owlhead Home Page")).toHaveAttribute("data-maximized", "true");
    await press(screen.getByRole("button", { name: "Restore Owlhead Home Page" }));
    expect(win("Owlhead Home Page")).not.toHaveAttribute("data-maximized");
  });

  it("closes a window off the taskbar, and the Owlhead icon opens it again", async () => {
    renderLanding();
    await press(screen.getByRole("button", { name: "Close Owlhead Home Page" }));
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
    expect(within(screen.getByRole("list", { name: "Open windows" })).queryAllByRole("button")).toHaveLength(0);
    await press(within(screen.getByRole("list", { name: "Desktop" })).getByRole("button", { name: "Owlhead" }));
    expect(win("Owlhead Home Page")).toBeVisible();
  });

  it("opens each icon's window in front, and a touched window comes forward", async () => {
    renderLanding();
    const icons = screen.getByRole("list", { name: "Desktop" });
    await press(within(icons).getByRole("button", { name: "readme.txt" }));
    const readme = win("readme.txt - Notepad");
    expect(readme).toHaveTextContent("Drag a window by its title bar.");
    expect(readme).toHaveAttribute("data-front", "true");
    expect(zOf(readme)).toBeGreaterThan(zOf(win("Owlhead Home Page")));

    await press(within(icons).getByRole("button", { name: "owl.jpg" }));
    const owl = win("owl.jpg - Picture Viewer");
    expect(within(owl).getByRole("img")).toHaveAccessibleName(/owl in ink, perched on a pine branch/);
    expect(owl).toHaveTextContent("Soga Nichokuan, Owl on a Pine Branch, early 17th century.");
    expect(within(owl).getByRole("link", { name: "See it at the Met" })).toHaveAttribute("href", "https://www.metmuseum.org/art/collection/search/77198");

    fireEvent.pointerDown(win("Owlhead Home Page"));
    expect(win("Owlhead Home Page")).toHaveAttribute("data-front", "true");
    expect(zOf(win("Owlhead Home Page"))).toBeGreaterThan(zOf(owl));
  });

  it("opens The record in its own window, with the tamper demo, and lists it on the taskbar", async () => {
    renderLanding();
    await press(icon("The record"));
    const record = win(RECORD);
    expect(record).toHaveAttribute("data-front", "true");
    expect(within(record).getByRole("heading", { name: "Why did it buy that?" })).toBeInTheDocument();
    expect(record).toHaveTextContent("Here is one decision from start to finish.");
    expect(record.querySelector("[data-slot=record-trace]")).not.toBeNull();
    expect(within(record).getByRole("button", { name: `Edit line ${EDITED.index + 1}` })).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Open windows" })).getAllByRole("button").map((b) => b.textContent)).toEqual(["Owlhead", "The record"]);
    expect(document.activeElement).toBe(record);
  });

  it("opens Questions in its own window, with every question", async () => {
    renderLanding();
    await press(icon("Questions"));
    const help = win(HELP);
    expect(help).toHaveAttribute("data-front", "true");
    for (const { q, a } of QUESTIONS) {
      expect(help).toHaveTextContent(q);
      expect(help).toHaveTextContent(a);
    }
  });

  it("opens the Guestbook in its own window, with the form, even with the home page minimized", async () => {
    renderLanding();
    await press(screen.getByRole("button", { name: "Minimize Owlhead Home Page" }));
    await press(icon("Guestbook"));
    const guestbook = win(GUESTBOOK);
    expect(guestbook).toHaveAttribute("data-front", "true");
    expect(guestbook.querySelector("[data-slot=beta-form]")).not.toBeNull();
    expect(within(guestbook).getByLabelText("Email address:")).toHaveAttribute("type", "email");
    expect(within(guestbook).getByRole("button", { name: "Sign the guestbook" })).toHaveAttribute("type", "submit");
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
  });

  it("opens on the painting the server picked for the visit, offers no grey pattern, and picks among every painting", async () => {
    const { container } = render(
      <WallpaperProvider initial="kiso-snow">
        <Landing />
      </WallpaperProvider>,
    );
    expect(container.querySelector("[data-slot=wallpaper]")).toHaveAttribute("data-wallpaper", "kiso-snow");
    expect(container.querySelector("[data-slot=desktop-pattern]")).toBeNull();
    await press(within(screen.getByRole("list", { name: "Desktop" })).getByRole("button", { name: "Display" }));
    const display = win("Display Properties");
    expect(within(display).getByRole("radio", { name: "The Kiso Mountains in Snow" })).toBeChecked();
    expect(within(display).queryAllByRole("radio", { name: /pattern/i })).toHaveLength(0);
    const rolls = Array.from({ length: 600 }, (_, i) => randomWallpaper(i / 600));
    expect(new Set(rolls), "every painting can open a visit, and nothing else").toEqual(new Set(WALLPAPERS.map((w) => w.id)));
    expect(randomWallpaper(0.999999)).toBe(WALLPAPERS.at(-1)?.id);
  });

  it("changes the wallpaper in Display, credits the painting, and keeps the choice out of browser storage", async () => {
    const { container } = renderLanding();
    const wallpaper = container.querySelector("[data-slot=wallpaper]")!;
    expect(wallpaper).toHaveAttribute("data-wallpaper", "auto");
    await press(within(screen.getByRole("list", { name: "Desktop" })).getByRole("button", { name: "Display" }));
    const display = win("Display Properties");
    await act(async () => fireEvent.click(within(display).getByRole("radio", { name: "Wheat Field with Cypresses" })));
    expect(wallpaper).toHaveAttribute("data-wallpaper", "wheat-field-cypresses");
    expect(localStorage.length).toBe(0);
    expect(display).toHaveTextContent("Vincent van Gogh, Wheat Field with Cypresses, 1889.");
    await act(async () => fireEvent.click(within(display).getByRole("radio", { name: "Day and night" })));
    expect(wallpaper).toHaveAttribute("data-wallpaper", "auto");
    await press(within(display).getByRole("button", { name: "OK" }));
    expect(screen.queryByRole("region", { name: "Display Properties" })).toBeNull();
  });

  it("lists every window in Start, closes it on Escape, and shuts down to the safe-to-turn-off screen", async () => {
    renderLanding();
    const startButton = screen.getByRole("button", { name: "Start" });
    await press(startButton);
    expect(startButton).toHaveAttribute("aria-expanded", "true");
    const start = screen.getByRole("menu", { name: "Start" });
    expect(within(start).getAllByRole("menuitem").map((i) => i.textContent)).toEqual([
      "Owlhead Home Page",
      "Guestbook",
      "The record",
      "Questions",
      "readme.txt",
      "owl.jpg",
      "Display",
      "Winamp",
      "Tour.mp4",
      "Recycle Bin",
      "Sign in",
      "Shut down…",
    ]);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Start" })).toBeNull();

    await press(startButton);
    await press(within(screen.getByRole("menu", { name: "Start" })).getByRole("menuitem", { name: "Shut down…" }));
    const off = screen.getByRole("button", { name: /safe to turn off/ });
    await press(off);
    expect(screen.queryByRole("button", { name: /safe to turn off/ })).toBeNull();
    expect(win("Owlhead Home Page")).toBeVisible();
  });
});

describe("the desktop's other things", () => {
  const right = () => screen.getByRole("list", { name: "Desktop, right" });

  it("plays Tour.mp4 in Media Player, loading nothing until play, with captions and a transcript", async () => {
    renderLanding();
    await press(within(right()).getByRole("button", { name: "Tour.mp4" }));
    const player = win("Tour.mp4 - Media Player");
    const video = player.querySelector("video")!;
    expect(video).toHaveAttribute("preload", "none");
    expect(video).toHaveAttribute("poster", TOUR_VIDEO.poster);
    expect(video.querySelector("source")).toHaveAttribute("src", TOUR_VIDEO.src);
    expect(video.querySelector("track[kind=captions]")).toHaveAttribute("src", TOUR_VIDEO.captions);
    await press(within(player).getByRole("button", { name: "Transcript" }));
    for (const s of TOUR) expect(within(player).getByRole("list", { name: "Transcript" })).toHaveTextContent(s.caption);
  });

  it("ships the tour's video, poster and the captions its scenes make", () => {
    const pub = join(__dirname, "../../../public");
    expect(readFileSync(join(pub, TOUR_VIDEO.src)).byteLength).toBeLessThan(4_000_000);
    expect(readFileSync(join(pub, TOUR_VIDEO.poster)).byteLength).toBeGreaterThan(1000);
    expect(readFileSync(join(pub, TOUR_VIDEO.captions), "utf8")).toBe(tourVtt());
  });

  it("keeps what Owlhead won't do in the Recycle Bin: Restore says why, and Empty empties it", async () => {
    renderLanding();
    await press(within(right()).getByRole("button", { name: "Recycle Bin" }));
    const bin = win("Recycle Bin");
    const [first] = DISCARDED;
    await press(within(bin).getByRole("button", { name: first.name }));
    expect(within(bin).getByRole("status")).toHaveTextContent(first.why);
    await press(within(bin).getByRole("button", { name: "Restore" }));
    expect(within(bin).getByRole("status")).toHaveTextContent(`${first.name} can't be restored.`);
    await press(within(bin).getByRole("button", { name: "Empty Recycle Bin" }));
    expect(bin).toHaveTextContent("This folder is empty.");
    expect(within(bin).getByRole("button", { name: "Empty Recycle Bin" })).toBeDisabled();
  });

  it("brings the owl assistant back from the desktop's menu, with tips and the tour", async () => {
    const { container } = renderLanding();
    fireEvent.contextMenu(container.querySelector("[data-slot=wallpaper]")!);
    await press(screen.getByRole("menuitem", { name: "Ask the owl…" }));
    const owl = screen.getByRole("complementary", { name: "Owl assistant" });
    expect(owl).toHaveTextContent(GREETING);
    await press(within(owl).getByRole("button", { name: "Give me a tip" }));
    expect(owl).toHaveTextContent(TIPS[0]);
    await press(within(owl).getByRole("button", { name: "Next tip" }));
    expect(owl).toHaveTextContent(TIPS[1]);
    await press(within(owl).getByRole("button", { name: "Watch the tour" }));
    expect(screen.queryByRole("complementary", { name: "Owl assistant" })).toBeNull();
    expect(win("Tour.mp4 - Media Player")).toBeVisible();
  });
});

describe("Winamp's playlist", () => {
  it("streams public domain MP3s from Wikimedia Commons instead of shipping them with the app", () => {
    expect(PLAYLIST.length).toBeGreaterThanOrEqual(20);
    for (const song of PLAYLIST) {
      expect(song.url).toMatch(/^https:\/\/upload\.wikimedia\.org\/wikipedia\/commons\/.+\.mp3$/);
      expect(song.metaData.title && song.metaData.artist).toBeTruthy();
      expect(song.duration).toBeGreaterThan(60);
    }
    expect(new Set(PLAYLIST.map((s) => s.url)).size).toBe(PLAYLIST.length);
  });
});

describe("the guestbook", () => {
  async function openGuestbook() {
    renderLanding();
    await press(icon("Guestbook"));
    return win(GUESTBOOK);
  }

  function fill(email: string) {
    fireEvent.change(within(win(GUESTBOOK)).getByLabelText("Email address:"), { target: { value: email } });
  }

  const sign = () => within(win(GUESTBOOK)).getByRole("button", { name: "Sign the guestbook" });

  it("posts the email and the chosen use to /api/beta, then says you're on the list", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const guestbook = await openGuestbook();
    fill("ada@example.com");
    fireEvent.click(within(guestbook).getByLabelText("Managing money for others"));
    await act(async () => {
      fireEvent.click(sign());
    });
    expect(fetchMock).toHaveBeenCalledWith("/api/beta", expect.objectContaining({ method: "POST" }));
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({ email: "ada@example.com", role: "clients", website: "" });
    expect(document.querySelector("[data-slot=beta-done]")).toHaveTextContent("You're on the list.");
    expect(document.querySelector("[data-slot=beta-done]")).toHaveTextContent("ada@example.com");
  });

  it("asks you to check the email when the server says it's wrong", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "email" }), { status: 400 })));
    const guestbook = await openGuestbook();
    fill("ada@example");
    await act(async () => {
      fireEvent.click(sign());
    });
    expect(within(guestbook).getByLabelText("Email address:")).toHaveAttribute("aria-invalid", "true");
    expect(document.getElementById("beta-problem")).toHaveTextContent("That email doesn't look right.");
    expect(document.getElementById("beta-problem")).toHaveAttribute("data-slot", "beta-problem");
  });

  it("says to try again when the request can't be saved or sent", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));
    await openGuestbook();
    fill("ada@example.com");
    await act(async () => {
      fireEvent.click(sign());
    });
    expect(document.getElementById("beta-problem")).toHaveTextContent("We couldn't save that just now.");
    expect(sign()).toBeEnabled();
  });

  it("carries a field people never see, for bots to fill", async () => {
    await openGuestbook();
    const trap = document.getElementById("beta-website")!;
    expect(trap).toHaveAttribute("tabindex", "-1");
    expect(trap.closest("[aria-hidden=true]")).not.toBeNull();
  });
});

describe("the record", () => {
  async function openRecord() {
    renderLanding();
    await press(icon("The record"));
    return win(RECORD);
  }

  it("shows one decision step by step, with no seal column until a line is edited, and a chain that matches", async () => {
    const record = await openRecord();
    const table = within(record).getByRole("table");
    expect(within(table).getAllByRole("row")).toHaveLength(TRACE.length + 1);
    for (const e of TRACE) expect(within(table).getByText(e.text)).toBeInTheDocument();
    expect(within(table).queryByRole("columnheader", { name: "Seal" })).toBeNull();
    expect(within(record).getByText(/all 8 lines match/)).toBeInTheDocument();
  });

  it("breaks the chain from an edited line down, and mends it on undo", async () => {
    const record = await openRecord();
    fireEvent.click(within(record).getByRole("button", { name: `Edit line ${EDITED.index + 1}` }));
    expect(within(record).getByText(EDITED.text)).toBeInTheDocument();
    expect(within(record).getByText(/fails at line 3\. Lines 3 to 8 no longer match/)).toBeInTheDocument();
    expect(within(record).getByRole("columnheader", { name: "Seal" })).toBeInTheDocument();
    expect(within(record).getAllByText("matches")).toHaveLength(EDITED.index);
    expect(within(record).getAllByText("edited")).toHaveLength(1);
    expect(within(record).getAllByText("no match")).toHaveLength(TRACE.length - EDITED.index - 1);
    fireEvent.click(within(record).getByRole("button", { name: "Undo the edit" }));
    expect(within(record).queryByText(EDITED.text)).toBeNull();
    expect(within(record).queryByRole("columnheader", { name: "Seal" })).toBeNull();
    expect(within(record).getByText(/all 8 lines match/)).toBeInTheDocument();
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

  it("says what Stop does to what the agents hold, and that an unanswered request is skipped", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).toContain("each one cancels its orders, sells what it holds and ends");
    expect(text).toContain("If you don't answer in time, it's skipped.");
    expect(QUESTIONS.find((q) => q.q === "How do I stop it?")?.a).toContain("sells what it holds");
    expect(QUESTIONS.find((q) => q.q === "What if I miss a request?")?.a).toMatch(/^Nothing is sent\./);
  });

  it("marks what is not built yet as coming, wherever the page names it", () => {
    const { container } = renderLanding();
    const text = readable(container);
    expect(text).toContain("in your own cloud, so strategy and keys stay with you. Both are coming during the beta.");
    expect(text).toContain("MCP server, coming during the beta.");
    expect(text).toContain("Agents that bring their own trade ideas.");
    expect(text).not.toMatch(/three tries|a stop, a target and a time limit/);
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

  it("uses no font-sans: the product's Public Sans never joins the landing page's three faces", () => {
    for (const file of SOURCES) expect(readFileSync(join(SITE_DIR, file), "utf8"), file).not.toMatch(/\bfont-sans\b/);
    const { container } = renderLanding();
    for (const el of container.querySelectorAll("*")) expect(el.getAttribute("class") ?? "").not.toMatch(/\bfont-sans\b/);
  });

  it("sets every word of the desktop and its windows in one of three faces: DotGothic16, VT323 or Pixelify Sans", () => {
    const { container } = renderLanding();
    const faces = [BODY, MONO, PIXEL].map((c) => `.${c}`).join(", ");
    const unset = [...container.querySelectorAll<HTMLElement>("*")].filter((el) => [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim()) && !el.closest(faces));
    expect(unset.map((el) => el.outerHTML.slice(0, 80))).toEqual([]);
    for (const field of container.querySelectorAll("input[type=email]")) expect(field.closest(faces)).not.toBeNull();
  });

  it("sets typed text at 20px or more, so a phone never zooms into the field", () => {
    renderLanding();
    expect(screen.getByLabelText("Email address:").className).toMatch(new RegExp(`\\b${MONO}\\b.*text-\\[1\\.25rem\\]`));
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
  { fg: "highlight-foreground", bg: "highlight", use: "The New tag, a hovered link and the sun badge" },
  { fg: "card", bg: "foreground", use: "Title bars, icon labels, the record's column heads and the ink badges" },
  { fg: "card", bg: "muted-foreground", use: "The title bars of windows behind the front one" },
  { fg: "foreground", bg: "warning-soft", use: "The edited line of the record" },
] as const;

describe("contrast", () => {
  it.each((["light", "dark"] as const).flatMap((theme) => LANDING_PAIRS.map((p) => [theme, p.fg, p.bg] as const)))("%s: %s on %s reaches WCAG AA (4.5:1)", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(4.5);
  });

  it("sets the shut-down screen's sun type on ink by day and on the night's background after dark", () => {
    expect(contrastRatio(tokenValue("highlight", "light"), tokenValue("foreground", "light"))).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(tokenValue("highlight", "dark"), tokenValue("background", "dark"))).toBeGreaterThanOrEqual(4.5);
  });
});
