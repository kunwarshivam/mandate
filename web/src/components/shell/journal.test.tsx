import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RECORD_AFTER_MS, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";

function pauseAll() {
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  const sheet = screen.getByRole("dialog");
  fireEvent.click(within(sheet).getByRole("button", { name: /Pause all agents/ }));
  return sheet;
}

beforeEach(() => {
  vi.useFakeTimers();
  setPathname("/");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("journal toasts", () => {
  it("raises a toast only after the runtime journals the action, naming the action and nothing else", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    pauseAll();
    expect(screen.queryByText("Recorded in the journal")).toBeNull();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(screen.getByText("Recorded in the journal")).toBeInTheDocument();
    expect(screen.getByText("Pause, as you asked.")).toBeInTheDocument();
  });

  it("raises no toast when the request was not delivered", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "unreachable");
    pauseAll();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(screen.queryByText("Recorded in the journal")).toBeNull();
  });
});

describe("result unknown", () => {
  it("says the result is unknown in a banner and the sheet, with no toast and no success", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "result-unknown");
    expect(document.querySelector("[data-slot=result-unknown]")).toBeNull();
    const sheet = pauseAll();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    const banner = document.querySelector("[data-slot=result-unknown]");
    expect(banner).toHaveAttribute("role", "alert");
    expect(banner).toHaveTextContent("The result is unknown; we are checking.");
    expect(within(sheet).getByRole("status")).toHaveTextContent("The result is unknown; we are checking.");
    expect(within(sheet).getByRole("status")).not.toHaveTextContent("Recorded");
    expect(screen.queryByText("Recorded in the journal")).toBeNull();
  });
});

describe("Stop sheet", () => {
  it("names the environment in its title", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const title = within(screen.getByRole("dialog")).getByRole("heading", { level: 2, name: /^Stop/ });
    expect(title.querySelector("[data-slot=environment-badge]")).toHaveTextContent("PAPER");
  });
});

describe("command palette", () => {
  it("opens on ⌘K with Stop… as the first command, and Stop… opens the sheet", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    act(() => {
      fireEvent.keyDown(window, { key: "k", metaKey: true });
    });
    const options = screen.getAllByRole("option");
    expect(options[0]).toHaveTextContent("Stop…");
    fireEvent.click(options[0]);
    act(() => vi.advanceTimersByTime(0));
    expect(screen.getByRole("dialog", { name: /Stop/ })).toBeInTheDocument();
  });

  it.each([
    ["the command bar", /^Jump to an agent or screen…/],
    ["the compact trigger", "Go to…"],
  ])("opens from %s, with the same commands as ⌘K", (_, name) => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const header = screen.getAllByRole("banner")[0];
    expect(screen.queryByRole("option")).toBeNull();
    fireEvent.click(within(header).getByRole("button", { name }));
    const options = screen.getAllByRole("option");
    expect(options[0]).toHaveTextContent("Stop…");
    expect(screen.getByRole("combobox", { name: "Command" })).toBeInTheDocument();
  });

  it("offers no Stop… to a viewer", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role: "viewer" });
    act(() => {
      fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    });
    expect(screen.queryByRole("option", { name: /Stop…/ })).toBeNull();
    expect(screen.getAllByRole("option").length).toBeGreaterThan(0);
  });
});
