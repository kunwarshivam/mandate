import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Deadline } from "./deadline";

const DEADLINE = "2026-09-28T14:20:00-04:00";
const ALARM = /animate|pulse|ping|bounce|blink|motion|transition|destructive|rose|crimson|red|persimmon|warning|alert/;

function classesUnder(root: Element) {
  return [root, ...root.querySelectorAll("*")].map((el) => el.getAttribute("class") ?? "").join(" ");
}

describe("Deadline", () => {
  it("states the absolute time with its zone and whole minutes left", () => {
    render(<Deadline deadline={DEADLINE} now="2026-09-28T14:05:20-04:00" />);
    const deadline = document.querySelector("[data-slot=deadline]")!;
    expect(deadline).toHaveTextContent("Skipped at 14:20:00 ET if you do nothing (14 min left)");
    expect(screen.getByText("14:20:00 ET").tagName).toBe("TIME");
  });

  it.each(["2026-09-28T14:05:20-04:00", "2026-09-28T14:19:30-04:00", "2026-09-28T14:21:00-04:00"])(
    "carries no animation, alarm colour, or live region at %s",
    (now) => {
      render(<Deadline deadline={DEADLINE} now={now} />);
      const deadline = document.querySelector("[data-slot=deadline]")!;
      expect(classesUnder(deadline)).not.toMatch(ALARM);
      expect(deadline.querySelector("[aria-live],[role=alert],[role=timer]")).toBeNull();
    },
  );

  it("does not change between two renders less than 15 seconds apart", () => {
    const { rerender } = render(<Deadline deadline={DEADLINE} now="2026-09-28T14:05:31-04:00" />);
    const first = document.querySelector("[data-slot=deadline]")!.textContent;
    rerender(<Deadline deadline={DEADLINE} now="2026-09-28T14:05:44-04:00" />);
    expect(document.querySelector("[data-slot=deadline]")!.textContent).toBe(first);
  });
});
