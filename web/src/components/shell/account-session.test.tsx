import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as SessionModule from "@/lib/session";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";

const signOut = vi.hoisted(() => vi.fn());

vi.mock("@/lib/session", async (original) => ({ ...(await original<typeof SessionModule>()), signOut }));

beforeEach(() => {
  setPathname("/");
  signOut.mockReset();
});

async function openMenu(name: "Your account" | "Alerts and account") {
  fireEvent.click(screen.getByRole("button", { name }));
  return screen.findByRole("menu");
}

describe("the account menus with sign-in off", () => {
  it.each(["Your account", "Alerts and account"] as const)("%s reads as before: no address and no Sign out", async (name) => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const menu = await openMenu(name);
    expect(within(menu).getByText("You, owner")).toBeInTheDocument();
    expect(within(menu).queryByRole("menuitem", { name: "Sign out" })).toBeNull();
    expect(menu.textContent).not.toContain("@");
  });

  it("leaves the phone's More sheet without Sign out", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    fireEvent.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" }));
    const sheet = screen.getByRole("dialog", { name: "More" });
    expect(within(sheet).queryByRole("button", { name: "Sign out" })).toBeNull();
    expect(sheet.querySelector("[data-slot=more-identity]")).toBeNull();
  });
});

describe("the account menus while signed in (DEC-211)", () => {
  const session = { email: "ada@example.com" };

  it.each(["Your account", "Alerts and account"] as const)("%s shows the address beside the role, and Sign out", async (name) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { session });
    const menu = await openMenu(name);
    expect(within(menu).getByText("ada@example.com").closest("[data-slot=account-identity]")).toHaveTextContent("You, ownerada@example.com");
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out" }));
    expect(signOut).toHaveBeenCalledOnce();
  });

  it("keeps the role in the line when the account has no address", async () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { session: { email: null }, role: "auditor" });
    const menu = await openMenu("Your account");
    expect(within(menu).getByText("You, auditor")).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
  });

  it("puts the address and Sign out in the phone's More sheet", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { session });
    fireEvent.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" }));
    const sheet = screen.getByRole("dialog", { name: "More" });
    expect(sheet.querySelector("[data-slot=more-identity]")).toHaveTextContent("You, ownerada@example.com");
    fireEvent.click(within(sheet).getByRole("button", { name: "Sign out" }));
    expect(signOut).toHaveBeenCalledOnce();
  });
});
