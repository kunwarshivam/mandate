import { readFileSync } from "node:fs";
import path from "node:path";
import { cleanup, render } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentOwl, Owl } from "@/components/domain/owl";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { BrandOwl } from "./brand-owl";

/**
 * The brand owl never wears an agent's colour (DEC-739 item 2, critique C-15): an owl is an agent,
 * so a second owl in the same colour reads as a second agent. The rule is checked as an outcome
 * (DEC-511): each owl's feather colour is read from what it renders, and resolved through the
 * stylesheet in both themes, so no hex and no brand token is named here.
 */

const CSS = readFileSync(path.resolve(__dirname, "../../app/globals.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const THEMES = ["light", "dark"] as const;
type Theme = (typeof THEMES)[number];
const DARK = 'html:root[data-mode="dark"]';
const OWL = '[data-slot="owl"]';

/** The custom properties declared in every block whose selector is exactly this one, later blocks winning. */
function declared(selector: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const block of CSS.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    if (block[1].trim() !== selector) continue;
    for (const d of block[2].matchAll(/--([a-z0-9-]+):\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  }
  return out;
}

/**
 * Every look the stylesheet can give an owl on `<html>`: none, and each `data-owl` value it styles.
 * A per-load pick is checked at every value it could land on, so a collision that a random draw
 * only sometimes makes is always made here.
 */
const PICKS: Array<string | undefined> = [undefined, ...new Set(Array.from(CSS.matchAll(/html\[data-owl="([^"]+)"\]/g), (m) => m[1]))];

/** The custom properties in force on an owl's `<svg>`, in a theme, with `<html>` carrying this pick. */
function owlScope(theme: Theme, pick: string | undefined): Record<string, string> {
  return {
    ...declared(":root"),
    ...(theme === "dark" ? declared(DARK) : {}),
    ...declared(OWL),
    ...(pick === undefined ? {} : declared(`html[data-owl="${pick}"] ${OWL}`)),
    ...(theme === "dark" ? declared(`${DARK} ${OWL}`) : {}),
  };
}

function resolve(value: string, scope: Record<string, string>, seen: string[] = []): string {
  return value.replace(/var\(--([a-z0-9-]+)\)/g, (_, name: string) => {
    if (seen.includes(name)) throw new Error(`cycle ${[...seen, name].join(" -> ")}`);
    const raw = scope[name];
    if (raw === undefined) throw new Error(`--${name} is not defined on an owl`);
    return resolve(raw, scope, [...seen, name]);
  });
}

/**
 * The feathers, read from the drawing alone: the opaque fill that covers the most sprite pixels.
 * The outline, the eyes and the beak are each smaller, and the shade and light are translucent.
 */
function feathersOf(svg: SVGSVGElement | null): string {
  if (!svg) throw new Error("no owl drawn");
  const area = new Map<string, number>();
  for (const r of svg.querySelectorAll("rect")) {
    if (r.hasAttribute("opacity")) continue;
    const fill = r.getAttribute("fill") ?? "";
    area.set(fill, (area.get(fill) ?? 0) + Number(r.getAttribute("width")) * Number(r.getAttribute("height")));
  }
  const [top] = [...area].sort((a, b) => b[1] - a[1]);
  if (!top) throw new Error("an owl with no opaque pixels");
  return top[0];
}

const drawn = (ui: ReactElement) => feathersOf(render(ui).container.querySelector<SVGSVGElement>("svg[data-slot=owl]"));

/** Each fixture agent's feathers, by scenario, from `AgentOwl` as every screen draws it. */
function agentFeathers(): Array<{ who: string; feathers: string }> {
  return SCENARIOS.flatMap(({ id }) => buildWorkspace(id).agents.map((a) => ({ who: `${id}: ${a.label} (${a.agent_id})`, feathers: drawn(<AgentOwl agent={a} still />) })));
}

/** Enough agent IDs that every feather colour an agent's owl may take turns up, not only the fixtures'. */
const ANY_AGENT = Array.from({ length: 64 }, (_, i) => `agt_${i.toString(36)}_${(i * 2654435761) >>> 0}`);

afterEach(cleanup);

describe("the brand owl never wears an agent's colour (DEC-739 item 2, C-15)", () => {
  it("reads an owl's feathers from its drawing", () => {
    expect(drawn(<Owl seed="any" mood="awake" feathers="var(--test-feathers)" beak="var(--test-beak)" still />)).toBe("var(--test-feathers)");
    expect(drawn(<Owl seed="any" mood="stopped" feathers="var(--test-feathers)" still />)).toBe("var(--test-feathers)");
  });

  it("finds the fixture agents and more than one agent colour, so the check below compares something", () => {
    const agents = agentFeathers();
    expect(agents.length).toBeGreaterThan(1);
    for (const theme of THEMES) {
      expect(new Set(agents.map((a) => resolve(a.feathers, owlScope(theme, undefined)))).size).toBeGreaterThan(1);
    }
  });

  it.each(THEMES)("differs from every scenario's agents in %s, whatever the page load picks", (theme) => {
    const brand = drawn(<BrandOwl still />);
    const agents = agentFeathers();
    for (const pick of PICKS) {
      const scope = owlScope(theme, pick);
      const own = resolve(brand, scope);
      for (const a of agents) {
        expect(resolve(a.feathers, scope), `${theme}, data-owl=${pick ?? "none"}: the brand owl matches ${a.who}`).not.toBe(own);
      }
    }
  });

  it.each(THEMES)("differs from every colour any agent's owl may take in %s", (theme) => {
    const brand = drawn(<BrandOwl still />);
    const colours = new Set(ANY_AGENT.map((id) => drawn(<AgentOwl agent={{ agent_id: id, mode: "normal" }} still />)));
    for (const pick of PICKS) {
      const scope = owlScope(theme, pick);
      const taken = new Set([...colours].map((f) => resolve(f, scope)));
      expect(taken.has(resolve(brand, scope)), `${theme}, data-owl=${pick ?? "none"}: the brand owl wears an agent colour`).toBe(false);
    }
  });
});
