import { describe, expect, it } from "vitest";
import { RECORD_ZONE, andList, byRecordDay } from "./format";

interface Entry {
  id: string;
  at: string;
}

const entries = (...ats: string[]): Entry[] => ats.map((at, n) => ({ id: `e${n}`, at }));

/** Each day's heading with its entries' ids and times, in the order the timeline reads them. */
function read(items: Entry[], now: string) {
  return byRecordDay(items, (e) => e.at, now, RECORD_ZONE).map((day) => ({
    heading: day.heading,
    entries: day.entries.map((e) => [e.item.id, e.time]),
  }));
}

describe("byRecordDay, given entries newest first (C-19)", () => {
  it("heads each day once, in order, across days that are not adjacent and a previous year", () => {
    const now = "2026-09-28T14:05:18-04:00";
    const items = entries(
      "2026-09-28T14:04:58-04:00",
      "2026-09-28T09:27:10-04:00",
      "2026-09-25T16:00:00-04:00",
      "2026-09-25T08:15:04-04:00",
      "2026-09-20T12:00:00-04:00",
      "2025-12-31T16:00:00-05:00",
    );
    expect(read(items, now)).toEqual([
      { heading: "Today", entries: [["e0", "14:04:58"], ["e1", "09:27:10"]] },
      { heading: "25 September", entries: [["e2", "16:00:00"], ["e3", "08:15:04"]] },
      { heading: "20 September", entries: [["e4", "12:00:00"]] },
      { heading: "31 December 2025", entries: [["e5", "16:00:00"]] },
    ]);
  });

  it("keeps every entry exactly once and in the order given", () => {
    const items = entries("2026-09-28T10:00:00-04:00", "2026-09-26T10:00:00-04:00", "2026-09-24T10:00:00-04:00", "2026-09-24T09:00:00-04:00");
    const flat = byRecordDay(items, (e) => e.at, "2026-09-28T14:05:18-04:00", RECORD_ZONE).flatMap((d) => d.entries.map((e) => e.item));
    expect(flat).toEqual(items);
  });

  it("keeps 1 November whole when the clocks go back and 01:00 to 01:59 happens twice, with each time as the clock read", () => {
    const now = "2026-11-02T10:00:00-05:00";
    const items = entries(
      "2026-11-02T05:00:00Z",
      "2026-11-02T04:59:59Z",
      "2026-11-01T01:30:00-05:00",
      "2026-11-01T06:00:00Z",
      "2026-11-01T05:59:59Z",
      "2026-11-01T01:30:00-04:00",
      "2026-11-01T04:00:00Z",
      "2026-11-01T03:59:59Z",
    );
    expect(read(items, now)).toEqual([
      { heading: "Today", entries: [["e0", "00:00:00"]] },
      {
        heading: "1 November",
        entries: [
          ["e1", "23:59:59"],
          ["e2", "01:30:00"],
          ["e3", "01:00:00"],
          ["e4", "01:59:59"],
          ["e5", "01:30:00"],
          ["e6", "00:00:00"],
        ],
      },
      { heading: "31 October", entries: [["e7", "23:59:59"]] },
    ]);
  });

  it("keeps 8 March whole when the clocks go forward and 02:00 to 02:59 never happens", () => {
    const now = "2026-03-09T10:00:00-04:00";
    const items = entries(
      "2026-03-09T04:00:00Z",
      "2026-03-09T03:59:59Z",
      "2026-03-08T07:00:00Z",
      "2026-03-08T06:59:59Z",
      "2026-03-08T05:00:00Z",
      "2026-03-08T04:59:59Z",
    );
    expect(read(items, now)).toEqual([
      { heading: "Today", entries: [["e0", "00:00:00"]] },
      {
        heading: "8 March",
        entries: [
          ["e1", "23:59:59"],
          ["e2", "03:00:00"],
          ["e3", "01:59:59"],
          ["e4", "00:00:00"],
        ],
      },
      { heading: "7 March", entries: [["e5", "23:59:59"]] },
    ]);
  });

  it("dates each entry for assistive tech by its Eastern day, not its UTC one, across both changes", () => {
    const dates = (items: Entry[], now: string) => byRecordDay(items, (e) => e.at, now, RECORD_ZONE).flatMap((d) => d.entries.map((e) => e.date));
    expect(dates(entries("2026-11-02T04:59:59Z", "2026-11-01T03:59:59Z"), "2026-11-02T10:00:00-05:00")).toEqual(["Nov 1, 2026", "Oct 31, 2026"]);
    expect(dates(entries("2026-03-09T03:59:59Z", "2026-03-08T04:59:59Z"), "2026-03-09T10:00:00-04:00")).toEqual(["Mar 8, 2026", "Mar 7, 2026"]);
  });
});

describe("andList (#1189)", () => {
  it("joins names as a sentence, without the serial comma", () => {
    expect(andList(["Agent 1"])).toBe("Agent 1");
    expect(andList(["Agent 1", "Agent 2"])).toBe("Agent 1 and Agent 2");
    expect(andList(["Agent 1", "Agent 2", "Agent 3"])).toBe("Agent 1, Agent 2 and Agent 3");
    expect(andList(["Agent 1", "Agent 2", "Agent 3", "Agent 4"])).toBe("Agent 1, Agent 2, Agent 3 and Agent 4");
  });

  it("gives an empty string for no names, as it does today; its one caller never passes none", () => {
    expect(andList([])).toBe("");
  });
});
