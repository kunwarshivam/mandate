import { describe, expect, it } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { isRecordRoute } from "@/lib/frozen";
import { agentHref, positionHref } from "@/lib/screens";
import { pageFor, pathsFor } from "@/test/app-routes";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

/**
 * Every P&L figure carries `[[DISCLOSURE-PERFORMANCE]]` beside it (brief §5, rule 9; DEC-79; DEC-210):
 * the disclosure symbol and the text it opens sit in the same region, the nearest section, article,
 * header, region or list item.
 * A P&L figure is a signed gain or loss (`SignedMoney`), or a dollar figure whose label names
 * realised or unrealised P&L, a gain or a loss. A limit, cap or floor on losses is a mandate term,
 * not a result.
 */
const PNL_LABEL = /\b(un)?reali[sz]ed\b|\bP&L\b|\bP\/L\b|\bgains?\b|\bloss(es)?\b/i;
const LIMIT_LABEL = /\b(limit|cap|floor)s?\b/i;
const MONEY = /[+−-]?\$\d[\d,]*(\.\d+)?/;
const REGION = "section, article, header, [role=region], li";
const TEXT = "[[DISCLOSURE-PERFORMANCE]]";
const PLACEHOLDER = '[data-placeholder="performance"]';
const TRIGGER = 'button[aria-label="Performance disclosure"]';

function ownText(el: Element): string {
  return Array.from(el.childNodes)
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent)
    .join("");
}

/** The words that name a figure: its column header, its term, or the text before it up to a colon. */
function labelOf(el: Element): string {
  const cell = el.closest("td");
  if (cell) {
    const header = cell.closest("table")?.querySelector("thead tr")?.children[(cell as HTMLTableCellElement).cellIndex];
    return header?.textContent ?? "";
  }
  const dd = el.closest("dd");
  if (dd) return dd.parentElement?.querySelector("dt")?.textContent ?? "";
  const text = el.closest("li, p, div")?.textContent ?? "";
  return text.slice(0, Math.max(0, text.search(MONEY))).split(":")[0];
}

function pnlFigures(): Element[] {
  return Array.from(document.body.querySelectorAll("*")).filter((el) => {
    if (el.hasAttribute("data-direction")) return true;
    if (el.closest("[data-direction]") || !MONEY.test(ownText(el))) return false;
    const label = labelOf(el);
    return PNL_LABEL.test(label) && !LIMIT_LABEL.test(label);
  });
}

/**
 * DEC-210: beside each P&L is the disclosure symbol, a button described by the disclosure text in the
 * same region. On a frozen record screen, where collapsed content counts as not shown (brief §4.1),
 * the text stands inline and there is no symbol.
 */
function disclosed(region: Element | null, inline: boolean): boolean {
  if (!region) return false;
  const text = region.querySelector(PLACEHOLDER);
  if (text?.textContent !== TEXT) return false;
  if (inline) return !region.querySelector(TRIGGER);
  return Array.from(region.querySelectorAll(TRIGGER)).some((t) => {
    const described = document.getElementById(t.getAttribute("aria-describedby") ?? "");
    return described !== null && region.contains(described) && described.matches(PLACEHOLDER) && described.textContent === TEXT;
  });
}

function undisclosed(inline = false): string[] {
  return pnlFigures()
    .filter((el) => !disclosed(el.closest(REGION), inline))
    .map((el) => {
      const region = el.closest(REGION);
      const name = region?.getAttribute("aria-labelledby") ?? region?.getAttribute("aria-label") ?? region?.tagName.toLowerCase() ?? "no region";
      return `${el.hasAttribute("data-direction") ? "signed" : `"${labelOf(el).trim()}"`} ${el.textContent?.trim()} in ${name}`;
    });
}

async function renderPath(path: string, scenario: (typeof SCENARIOS)[number]["id"]) {
  setPathname(path);
  const page = await pageFor(path);
  return renderWithRuntime(<AppShell>{page}</AppShell>, scenario);
}

const WS = buildWorkspace("normal");
const SWING = WS.agents.find((a) => a.agent_id === AGENT_IDS.swing)!;

describe("[[DISCLOSURE-PERFORMANCE]] beside every P&L figure", () => {
  it.each([
    ["the dashboard's positions", "/"],
    ["the positions list", "/positions"],
    ["an agent's overview", agentHref(AGENT_IDS.swing, "overview")],
    ["an agent's positions tab", agentHref(AGENT_IDS.swing, "positions")],
    ["a position record", positionHref(AGENT_IDS.swing, SWING.positions[0].instrument.asset_id)],
  ])("finds P&L on %s, so the route scan below has something to check", async (_, path) => {
    await renderPath(path, "normal");
    expect(pnlFigures().length).toBeGreaterThan(0);
  });

  it.each(SCENARIOS.flatMap((s) => pathsFor(s.id).map((path) => [path, s.id] as const)))("%s in %s shows no P&L without its disclosure in its region", async (path, scenario) => {
    await renderPath(path, scenario);
    expect(undisclosed(isRecordRoute(path))).toEqual([]);
    for (const trigger of document.querySelectorAll(TRIGGER)) {
      expect(trigger).toHaveAccessibleDescription(TEXT);
      expect(trigger).toHaveAttribute("aria-expanded", "false");
    }
  });

  it.each([
    ["the symbol is missing", (t: Element) => t.remove()],
    ["the symbol is not described by the text", (t: Element) => t.removeAttribute("aria-describedby")],
    ["the text is gone", (t: Element) => document.getElementById(t.getAttribute("aria-describedby")!)!.remove()],
  ])("fails every P&L on Home when %s", async (_, seed) => {
    await renderPath("/", "normal");
    expect(undisclosed()).toEqual([]);
    document.querySelectorAll(TRIGGER).forEach(seed);
    expect(undisclosed()).toHaveLength(pnlFigures().length);
  });

  it("fails a P&L on a record screen shown behind a symbol", async () => {
    await renderPath("/", "normal");
    expect(undisclosed(true)).toHaveLength(pnlFigures().length);
  });

  it("puts a symbol beside every P&L on Home, one per disclosure", async () => {
    await renderPath("/", "normal");
    const triggers = document.querySelectorAll(TRIGGER);
    const texts = document.querySelectorAll(PLACEHOLDER);
    expect(triggers.length).toBeGreaterThanOrEqual(5);
    expect(texts).toHaveLength(triggers.length);
  });
});
