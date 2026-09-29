import { fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { CVD_COOKIE } from "@/lib/colour-pref";
import { renderWithRuntime } from "@/test/harness";
import { ScenarioSwitcher } from "./scenario-switcher";

const cvdCookie = () => document.cookie.split("; ").find((c) => c.startsWith(`${CVD_COOKIE}=`));

afterEach(() => {
  document.cookie = `${CVD_COOKIE}=; path=/; max-age=0`;
});

describe("ScenarioSwitcher", () => {
  it("Alt+Shift+C turns colour-blind friendly on, and off again", () => {
    const { rerender } = renderWithRuntime(<ScenarioSwitcher scenario="normal" colourBlind={false} />);
    fireEvent.keyDown(window, { code: "KeyC", altKey: true, shiftKey: true });
    expect(cvdCookie()).toBe(`${CVD_COOKIE}=on`);
    rerender(<ScenarioSwitcher scenario="normal" colourBlind />);
    fireEvent.keyDown(window, { code: "KeyC", altKey: true, shiftKey: true });
    expect(cvdCookie()).toBe(`${CVD_COOKIE}=off`);
  });

  it("ignores C without both modifiers, and has no palette shortcut", () => {
    renderWithRuntime(<ScenarioSwitcher scenario="normal" colourBlind={false} />);
    fireEvent.keyDown(window, { code: "KeyC", altKey: true });
    fireEvent.keyDown(window, { code: "KeyP", altKey: true, shiftKey: true });
    expect(cvdCookie()).toBeUndefined();
  });

  it("the checkbox sets the cookie and links to the palette reference", () => {
    renderWithRuntime(<ScenarioSwitcher scenario="normal" colourBlind={false} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Colour-blind friendly" }));
    expect(cvdCookie()).toBe(`${CVD_COOKIE}=on`);
    expect(screen.getByRole("link", { name: "Palette" })).toHaveAttribute("href", "/palette");
    expect(screen.queryByRole("combobox", { name: /palette/i })).toBeNull();
  });
});
