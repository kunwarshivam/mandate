import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import { StopControl } from "@/components/shell/stop-control";
import { PASSKEY_ANSWER_MS } from "@/components/stop/step-up-dialog";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ROUTES } from "@/test/routes";
import { colorTokens, tokenValue } from "./tokens";

/** Crimson is the kill switch and nothing else (web/DESIGN.md): the sheet's kill-switch links and the record screens' switches. */
const KILL_SWITCH = /^(Kill switch: (close|cancel) and stop|Activate the kill switch|Stop all agents on this account|Close everything on this account)/;

const CRIMSON_VALUE = /oklch\(\s*0\.415[\s_]+0\.164[\s_]+27\b/;
const CRIMSON_NAME = /-crimson\b/;

const root = resolve(process.cwd(), "src");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return /\.(tsx?|css)$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : [];
  });
}

function paintsCrimson(el: Element): boolean {
  const painted = `${el.getAttribute("class") ?? ""} ${el.getAttribute("style") ?? ""}`;
  return CRIMSON_NAME.test(painted) || CRIMSON_VALUE.test(painted);
}

/** Crimson outside a kill-switch choice; on /design, the kill-switch specimen is allowed too. */
function strays({ design = false } = {}): string[] {
  return Array.from(document.body.querySelectorAll("*"))
    .filter(paintsCrimson)
    .filter((el) => !KILL_SWITCH.test(el.closest("button, a")?.textContent ?? ""))
    .filter((el) => !(design && el.closest('[data-meaning="kill"]')))
    .map((el) => `<${el.tagName.toLowerCase()} class="${el.getAttribute("class")}"> ${(el.textContent ?? "").slice(0, 60)}`);
}

function crimsonCount(): number {
  return Array.from(document.body.querySelectorAll("*")).filter(paintsCrimson).length;
}

function openSheet() {
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  const sheet = screen.getByRole("dialog");
  for (const trigger of sheet.querySelectorAll("button[aria-expanded=false]")) fireEvent.click(trigger);
  expect(sheet.querySelector("button[aria-expanded=false]")).toBeNull();
  return sheet;
}

function choices(sheet: HTMLElement): HTMLElement[] {
  return Array.from(sheet.querySelectorAll<HTMLElement>("button[data-tone]"));
}

/** The Stop sheet's contexts: the whole account, and each agent's own page. */
function contexts(scenario: (typeof SCENARIOS)[number]["id"]): string[] {
  return ["/", ...buildWorkspace(scenario).agents.map((a) => `/agents/${a.agent_id}`)];
}

beforeEach(() => setPathname("/"));

describe("crimson in the source", () => {
  it("is a class or variable only in globals.css, the kill-switch button, and the /design specimen", () => {
    const allowed = ["app/globals.css", "components/stop/kill-switch-button.tsx", "app/design/page.tsx"];
    const naming = sources(root)
      .filter((file) => CRIMSON_NAME.test(readFileSync(file, "utf8")))
      .map((file) => relative(root, file));
    expect(naming.sort()).toEqual([...allowed].sort());
  });

  it("has its value written only in the stylesheet; palette.ts builds it from the crimson ramp", () => {
    const writing = sources(root)
      .filter((file) => CRIMSON_VALUE.test(readFileSync(file, "utf8")))
      .map((file) => relative(root, file));
    expect(writing).toEqual(["app/globals.css"]);
    expect(tokenValue("crimson")).toMatch(CRIMSON_VALUE);
  });

  it("is read only by the crimson utilities, never aliased by another variable", () => {
    const css = readFileSync(join(root, "app/globals.css"), "utf8");
    const readers = Array.from(css.matchAll(/--([a-z0-9-]+):\s*[^;]*var\(--crimson[a-z-]*\)/g), (m) => m[1]);
    expect(readers.sort()).toEqual(["color-crimson", "color-crimson-foreground"]);
  });

  it("is no other token's value", () => {
    for (const t of colorTokens.filter((c) => c.meaning !== "kill")) expect(t.value, t.name).not.toBe(tokenValue("crimson"));
  });

  it.each(["components/stop/stop-sheet.tsx", "components/stop/record-screen.tsx"])(
    "is used only by the kill-switch button's two appearances, and %s paints through that button",
    (file) => {
      const button = readFileSync(join(root, "components/stop/kill-switch-button.tsx"), "utf8");
      const lines = button.split("\n").filter((line) => CRIMSON_NAME.test(line));
      expect(lines).toHaveLength(1);
      expect(lines[0].trim()).toMatch(/^appearance === "filled" \? "bg-crimson text-crimson-foreground[^"]*" : "border-2 border-crimson [^"]*",$/);
      const source = readFileSync(join(root, file), "utf8");
      expect(source).not.toMatch(CRIMSON_NAME);
      const titles = Array.from(source.matchAll(/<KillSwitchButton\b[^>]*\btitle="([^"]+)"/g), (m) => m[1]);
      expect(titles.length).toBeGreaterThan(0);
      for (const title of titles) expect(title).toMatch(KILL_SWITCH);
    },
  );
});

describe("crimson on screen", () => {
  afterEach(() => vi.useRealTimers());

  it.each(ROUTES.flatMap(([path, Page]) => SCENARIOS.map((s) => [path, s.id, Page] as const)))("%s in %s shows crimson only on the kill switch", (path, scenario, Page) => {
    setPathname(path);
    renderWithRuntime(
      <AppShell>
        <Page />
      </AppShell>,
      scenario,
    );
    expect(strays({ design: path === "/design" })).toEqual([]);
  });

  it.each(SCENARIOS.flatMap((s) => contexts(s.id).map((path) => [path, s.id] as const)))("the Stop sheet at %s in %s shows crimson only on the kill switch", (path, scenario) => {
    setPathname(path);
    renderWithRuntime(<StopControl />, scenario);
    openSheet();
    expect(crimsonCount()).toBeGreaterThan(0);
    expect(strays()).toEqual([]);
  });

  it.each(contexts("normal"))("at %s, the passkey check and the recorded result of every choice carry no crimson", (path) => {
    vi.useFakeTimers();
    setPathname(path);
    const { unmount } = renderWithRuntime(<StopControl />);
    const count = choices(openSheet()).length;
    unmount();
    expect(count).toBeGreaterThan(0);

    for (let i = 0; i < count; i += 1) {
      const view = renderWithRuntime(<StopControl />);
      const sheet = openSheet();
      const choice = choices(sheet)[i];
      const title = choice.querySelector("span")?.textContent ?? "";
      fireEvent.click(choice);
      const stepUp = screen.queryByRole("dialog", { name: "Confirm it is you" });
      if (stepUp) {
        expect(Array.from(stepUp.querySelectorAll("*")).filter(paintsCrimson), title).toEqual([]);
        fireEvent.click(within(stepUp).getByRole("button", { name: "Use passkey" }));
        act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
      }
      act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
      expect(within(sheet).getByRole("status").querySelector("[data-phase=recorded]"), title).not.toBeNull();
      expect(strays(), title).toEqual([]);
      view.unmount();
    }
  });

  it.each(ROUTES.filter(([path]) => /\/(kill-switch|release|stop-all|close-all)$/.test(path)))("on the record screen %s, the passkey check and the recorded result carry no crimson", (path, Page) => {
    vi.useFakeTimers();
    setPathname(path);
    renderWithRuntime(<Page />);
    const record = document.querySelector<HTMLElement>("[data-slot=record-screen]")!;
    const action = record.querySelector<HTMLElement>("[data-slot=kill-switch], button[data-tone]")!;
    fireEvent.click(action);
    const stepUp = screen.getByRole("dialog", { name: "Confirm it is you" });
    expect(Array.from(stepUp.querySelectorAll("*")).filter(paintsCrimson)).toEqual([]);
    fireEvent.click(within(stepUp).getByRole("button", { name: "Use passkey" }));
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    expect(within(record).getByRole("status").querySelector("[data-phase=recorded]")).not.toBeNull();
    expect(strays()).toEqual([]);
  });
});
