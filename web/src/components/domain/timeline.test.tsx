import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { TimelineEvent } from "@/fixtures/types";
import { Timeline } from "./timeline";

/** The viewer's now in these tests: 28 September 2026, 14:05:20 ET, the fixture's. */
const NOW = "2026-09-28T14:05:20-04:00";

function event(id: string, at: string): TimelineEvent {
  return { event_id: id, at, kind: "order", text: `Order ${id}.` };
}

function headings(level: number): (string | null)[] {
  return screen.queryAllByRole("heading", { level }).map((h) => h.textContent);
}

/** The entries under a day's heading, found by the list that heading names. */
function underHeading(day: string): HTMLElement[] {
  return [...screen.getByRole("list", { name: day }).querySelectorAll<HTMLElement>("li")];
}

/** What an entry's time shows on screen: its text without the full date kept for assistive tech. */
function shownTime(entry: HTMLElement): string {
  const time = entry.querySelector("time")!.cloneNode(true) as HTMLElement;
  time.querySelectorAll("[data-slot=full-date]").forEach((n) => n.remove());
  return time.textContent ?? "";
}

describe("the record's timeline marks where each day ends (C-19)", () => {
  const twoDays = [event("today", "2026-09-28T08:02:44-04:00"), event("older", "2026-09-25T08:15:00-04:00")];

  it("puts entries from two days under two headings, Today and the older date", () => {
    render(<Timeline events={twoDays} now={NOW} />);
    expect(headings(3)).toEqual(["Today", "25 September"]);
    expect(underHeading("Today")).toHaveLength(1);
    expect(underHeading("25 September")).toHaveLength(1);
  });

  it("shows times alone under each heading, and keeps each entry's full date for assistive tech", () => {
    render(<Timeline events={twoDays} now={NOW} />);
    const [today] = underHeading("Today");
    const [older] = underHeading("25 September");
    expect(shownTime(today)).toBe("08:02:44");
    expect(shownTime(older)).toBe("08:15:00");
    expect(older.querySelector("time")).toHaveAttribute("dateTime", "2026-09-25T08:15:00-04:00");
    expect(older.querySelector("time")).toHaveTextContent(/^Sep 25, 2026, 08:15:00$/);
    expect(today.querySelector("time")).toHaveTextContent(/^Sep 28, 2026, 08:02:44$/);
  });

  it("counts days in Eastern time, so midnight ET written in UTC starts today", () => {
    render(<Timeline events={[event("midnight", "2026-09-28T04:00:00Z"), event("before", "2026-09-28T03:59:59Z")]} now={NOW} />);
    expect(headings(3)).toEqual(["Today", "27 September"]);
    expect(underHeading("Today").map(shownTime)).toEqual(["00:00:00"]);
    expect(underHeading("27 September").map(shownTime)).toEqual(["23:59:59"]);
  });

  it("takes its heading level from where it sits", () => {
    render(<Timeline events={twoDays} now={NOW} headingLevel={2} />);
    expect(headings(2)).toEqual(["Today", "25 September"]);
  });

  it("shows no headings when nothing is recorded", () => {
    render(<Timeline events={[]} now={NOW} />);
    expect(screen.queryAllByRole("heading")).toEqual([]);
    expect(screen.getByText("Nothing recorded yet.")).toBeInTheDocument();
  });
});
