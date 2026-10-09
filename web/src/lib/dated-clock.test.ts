import { describe, expect, it } from "vitest";
import { RECORD_ZONE, datedClock } from "./format";

const ET = "America/New_York";

describe("a time on the record, placed in its day (C-23)", () => {
  const now = "2026-09-28T14:05:18-04:00";

  it("reads the record's times in Eastern time, the zone every ET label names", () => {
    expect(RECORD_ZONE).toBe(ET);
  });

  it("shows only the clock for a time on the viewer's current day", () => {
    expect(datedClock("2026-09-28T14:01:12-04:00", now, ET)).toBe("14:01:12");
    expect(datedClock("2026-09-28T00:00:00-04:00", now, ET)).toBe("00:00:00");
    expect(datedClock("2026-09-28T23:59:59-04:00", now, ET)).toBe("23:59:59");
  });

  it("dates a time from yesterday, earlier this year, and a previous year, as the timeline does", () => {
    expect(datedClock("2026-09-27T09:30:00-04:00", now, ET)).toBe("Sep 27, 2026, 09:30");
    expect(datedClock("2026-09-26T15:12:40-04:00", now, ET)).toBe("Sep 26, 2026, 15:12");
    expect(datedClock("2026-01-02T10:05:00-05:00", now, ET)).toBe("Jan 2, 2026, 10:05");
    expect(datedClock("2025-12-31T16:00:00-05:00", now, ET)).toBe("Dec 31, 2025, 16:00");
  });

  it("dates a time from a later day too, so a time never reads as the future of today", () => {
    expect(datedClock("2026-09-29T09:30:00-04:00", now, ET)).toBe("Sep 29, 2026, 09:30");
  });

  it("splits the days one minute either side of local midnight", () => {
    const justAfter = "2026-09-28T00:00:30-04:00";
    expect(datedClock("2026-09-27T23:59:30-04:00", justAfter, ET)).toBe("Sep 27, 2026, 23:59");
    expect(datedClock("2026-09-28T00:00:30-04:00", justAfter, ET)).toBe("00:00:30");
    const justBefore = "2026-09-27T23:59:30-04:00";
    expect(datedClock("2026-09-28T00:00:30-04:00", justBefore, ET)).toBe("Sep 28, 2026, 00:00");
    expect(datedClock("2026-09-27T23:59:00-04:00", justBefore, ET)).toBe("23:59:00");
  });

  it("finds the day in the zone it is given, not in the offset the timestamp happens to be written in", () => {
    const justAfter = "2026-09-28T00:00:30-04:00";
    expect(datedClock("2026-09-28T03:59:00Z", justAfter, ET)).toBe("Sep 27, 2026, 23:59");
    expect(datedClock("2026-09-28T04:01:00Z", justAfter, ET)).toBe("00:01:00");
    expect(datedClock("2026-09-28T03:59:00Z", "2026-09-28T04:00:30Z", "UTC")).toBe("03:59:00");
    expect(datedClock("2026-09-27T23:59:00Z", "2026-09-28T04:00:30Z", "UTC")).toBe("Sep 27, 2026, 23:59");
  });

  it("keeps the day whole on the day the clocks go back, and splits it at that day's own midnight", () => {
    const fallBack = "2026-11-01T12:00:00-05:00";
    expect(datedClock("2026-11-01T00:00:00-04:00", fallBack, ET)).toBe("00:00:00");
    expect(datedClock("2026-11-01T01:30:00-04:00", fallBack, ET)).toBe("01:30:00");
    expect(datedClock("2026-11-01T01:30:00-05:00", fallBack, ET)).toBe("01:30:00");
    expect(datedClock("2026-10-31T23:59:00-04:00", fallBack, ET)).toBe("Oct 31, 2026, 23:59");
    const nextMidnight = "2026-11-02T00:00:30-05:00";
    expect(datedClock("2026-11-01T23:59:30-05:00", nextMidnight, ET)).toBe("Nov 1, 2026, 23:59");
    expect(datedClock("2026-11-02T00:00:00-05:00", nextMidnight, ET)).toBe("00:00:00");
  });

  it("keeps the day whole on the day the clocks go forward", () => {
    const springForward = "2026-03-08T12:00:00-04:00";
    expect(datedClock("2026-03-08T01:59:00-05:00", springForward, ET)).toBe("01:59:00");
    expect(datedClock("2026-03-08T03:00:00-04:00", springForward, ET)).toBe("03:00:00");
    expect(datedClock("2026-03-07T23:59:00-05:00", springForward, ET)).toBe("Mar 7, 2026, 23:59");
  });

  it("takes the day from the now it is given, never from the host's clock", () => {
    expect(datedClock("2026-09-26T15:12:40-04:00", "2026-09-26T23:00:00-04:00", ET)).toBe("15:12:40");
    expect(datedClock("2020-02-29T08:00:00-05:00", "2020-02-29T20:00:00-05:00", ET)).toBe("08:00:00");
  });
});
