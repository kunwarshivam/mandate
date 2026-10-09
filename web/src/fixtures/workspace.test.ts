// @vitest-environment node
import { describe, expect, it } from "vitest";
import { SCENARIOS, buildWorkspace } from "./workspace";

/**
 * The timelines render `ws.decisions` and `ws.timeline[agent]` as given, and `byRecordDay` groups
 * consecutive entries, so each list must be newest first. The check reads each entry's instant
 * itself and compares neighbours, rather than sorting with the fixture's own helper.
 */
function outOfOrder(entries: ReadonlyArray<{ event_id: string; at: string }>): string[] {
  const found: string[] = [];
  for (let i = 1; i < entries.length; i += 1) {
    const newer = entries[i - 1];
    const older = entries[i];
    if (Date.parse(older.at) > Date.parse(newer.at)) found.push(`${older.event_id} (${older.at}) after ${newer.event_id} (${newer.at})`);
  }
  return found;
}

describe("every fixture scenario lists its records newest first", () => {
  for (const { id } of SCENARIOS) {
    it(`${id}: the decisions and every agent's timeline never go back in time`, () => {
      const ws = buildWorkspace(id);
      expect(ws.decisions.every((d) => Number.isFinite(Date.parse(d.at))), "every decision has a readable time").toBe(true);
      expect(outOfOrder(ws.decisions), "decisions").toEqual([]);
      for (const [agentId, events] of Object.entries(ws.timeline)) {
        expect(events.every((e) => Number.isFinite(Date.parse(e.at))), `${agentId} has readable times`).toBe(true);
        expect(outOfOrder(events), `timeline of ${agentId}`).toEqual([]);
      }
    });
  }
});
