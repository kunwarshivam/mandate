import { describe, expect, it } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { agentHref, positionHref } from "@/lib/screens";
import { pageFor, pathsFor } from "@/test/app-routes";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

/**
 * Every P&L figure carries `[[DISCLOSURE-PERFORMANCE]]` beside it (brief §5, rule 9; DEC-79): the
 * placeholder sits in the same region, the nearest section, article, header, region or list item.
 * A P&L figure is a signed gain or loss (`SignedMoney`), or a dollar figure whose label names
 * realised or unrealised P&L, a gain or a loss. A limit, cap or floor on losses is a mandate term,
 * not a result.
 */
const PNL_LABEL = /\b(un)?reali[sz]ed\b|\bP&L\b|\bP\/L\b|\bgains?\b|\bloss(es)?\b/i;
const LIMIT_LABEL = /\b(limit|cap|floor)s?\b/i;
const MONEY = /[+−-]?\$\d[\d,]*(\.\d+)?/;
const REGION = "section, article, header, [role=region], li";

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

function undisclosed(): string[] {
  return pnlFigures()
    .filter((el) => !el.closest(REGION)?.querySelector('[data-placeholder="performance"]'))
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

  it.each(SCENARIOS.flatMap((s) => pathsFor(s.id).map((path) => [path, s.id] as const)))("%s in %s shows no P&L without the placeholder in its region", async (path, scenario) => {
    await renderPath(path, scenario);
    expect(undisclosed()).toEqual([]);
  });
});
