import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_DESKTOP, DESKTOP_COOKIE, desktopStyleFrom, type DesktopStyle } from "@/lib/desktop-style";
import { THEME_COOKIE } from "@/lib/theme";
import { DesktopStyleProvider } from "./desktop-style";
import { Landing } from "./landing";
import { Logon } from "./logon";

/**
 * The desktop style switch (DEC-904): the signed-out pages wear Windows 98 or System 7, chosen with a
 * button on the desktop and kept in a cookie. Windows stays the default until the Mac is finished.
 */

const frame = () => new Promise((r) => requestAnimationFrame(r));

async function press(el: HTMLElement) {
  await act(async () => {
    fireEvent.click(el);
    await frame();
  });
}

function renderOn(style: DesktopStyle) {
  return render(
    <DesktopStyleProvider initial={style}>
      <Landing />
    </DesktopStyleProvider>,
  );
}

const os = () => document.querySelector("[data-os]")?.getAttribute("data-os");
const menuBar = () => document.querySelector<HTMLElement>("[data-slot=menu-bar]");
const barTitle = (name: string) => within(menuBar()!).getByRole("button", { name });
const win = (name: string) => screen.getByRole("region", { name });

afterEach(() => {
  document.cookie = `${DESKTOP_COOKIE}=; path=/; max-age=0`;
  document.cookie = `${THEME_COOKIE}=; path=/; max-age=0`;
  const root = document.documentElement;
  delete root.dataset.mode;
  delete root.dataset.themePref;
  root.classList.remove("dark");
  root.style.colorScheme = "";
});

describe("the desktop style cookie", () => {
  it("reads Windows unless the cookie names the Mac, so a missing or tampered cookie draws the default", () => {
    expect(DEFAULT_DESKTOP).toBe("windows");
    expect(desktopStyleFrom(undefined)).toBe("windows");
    expect(desktopStyleFrom("")).toBe("windows");
    expect(desktopStyleFrom("os2")).toBe("windows");
    expect(desktopStyleFrom("Mac")).toBe("windows");
    expect(desktopStyleFrom("mac")).toBe("mac");
    expect(desktopStyleFrom("windows")).toBe("windows");
  });
});

describe("the style switch", () => {
  const computer = (name: string) => within(screen.getByRole("list", { name: "Desktop, right" })).getByRole("button", { name });

  it("is the other desktop's computer among the icons, named for the computer alone, and swaps the taskbar for the Mac's menu bar, keeping the choice", async () => {
    renderOn("windows");
    expect(os()).toBe("windows");
    expect(document.querySelector("[data-slot=taskbar]")).not.toBeNull();
    expect(menuBar()).toBeNull();
    const mac = computer("Mac");
    expect(mac.textContent, "on screen it is a computer, with no name under it").toBe("");
    expect(mac, "assistive technology hears what it does").toHaveAccessibleDescription("Changes the desktop to a 1990s Mac's.");
    expect(document.querySelector("[data-slot=tray]")?.textContent, "nothing at the bottom names it").not.toMatch(/Mac|style/i);
    await press(mac);
    expect(os()).toBe("mac");
    expect(document.querySelector("[data-slot=taskbar]")).toBeNull();
    expect(menuBar()).not.toBeNull();
    expect(menuBar()?.textContent, "nor does the menu bar").not.toMatch(/Windows|style/i);
    expect(document.cookie).toContain(`${DESKTOP_COOKIE}=mac`);
    const pc = computer("PC");
    expect(pc.textContent).toBe("");
    expect(pc).toHaveAccessibleDescription("Changes the desktop back to Windows 98.");
    await press(pc);
    expect(os()).toBe("windows");
    expect(document.cookie).toContain(`${DESKTOP_COOKIE}=windows`);
  });

  it("is in Display for the keyboard, since the icons are out of the tab order, as the two computers' pictures, and in no menu", async () => {
    renderOn("windows");
    await press(screen.getByRole("button", { name: "Start" }));
    expect(within(screen.getByRole("menu", { name: "Start" })).queryByRole("menuitem", { name: /Mac|PC|Windows/ })).toBeNull();
    await press(within(screen.getByRole("menu", { name: "Start" })).getByRole("menuitem", { name: "Display" }));
    const choice = document.querySelector<HTMLElement>("[data-slot=desktop-choice]")!;
    expect(choice.textContent, "neither computer is written").toBe("Desktop");
    expect(within(choice).getByRole("radio", { name: "Windows 98" })).toBeChecked();
    await act(async () => fireEvent.click(within(choice).getByRole("radio", { name: "System 7" })));
    expect(os()).toBe("mac");
    expect(document.cookie).toContain(`${DESKTOP_COOKIE}=mac`);
    await press(barTitle("Owlhead"));
    expect(within(screen.getByRole("menu", { name: "Owlhead" })).queryByRole("menuitem", { name: /Mac|PC|Windows/ })).toBeNull();
  });

  it("leaves the windows where they were", async () => {
    renderOn("windows");
    await press(screen.getByRole("button", { name: "Maximize Owlhead Home Page" }));
    await press(computer("Mac"));
    expect(win("Owlhead Home Page")).toHaveAttribute("data-maximized", "true");
  });
});

describe("the Mac desktop", () => {
  it("draws System 7's title bar: the close box at the left, zoom and collapse at the right, all gone behind", async () => {
    renderOn("mac");
    const home = win("Owlhead Home Page");
    const bar = within(home).getAllByRole("button").slice(0, 3);
    expect(bar.map((b) => b.getAttribute("aria-label"))).toEqual(["Close Owlhead Home Page", "Maximize Owlhead Home Page", "Minimize Owlhead Home Page"]);
    await press(within(screen.getByRole("list", { name: "Desktop" })).getByRole("button", { name: "readme.txt" }));
    expect(within(home).queryByRole("button", { name: "Close Owlhead Home Page" }), "a window behind shows no boxes").toBeNull();
    expect(within(win("readme.txt - Notepad")).getByRole("button", { name: "Close readme.txt - Notepad" })).toBeInTheDocument();
  });

  it("collapses a window into the windows menu, which checks the window in front and brings one back", async () => {
    renderOn("mac");
    await press(screen.getByRole("button", { name: "Minimize Owlhead Home Page" }));
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
    await press(barTitle("Open windows"));
    const menu = screen.getByRole("menu", { name: "Open windows" });
    const item = within(menu).getByRole("menuitemradio", { name: /Owlhead/ });
    expect(item).toHaveAttribute("aria-checked", "false");
    await press(item);
    expect(win("Owlhead Home Page")).toBeVisible();
    expect(screen.queryByRole("menu", { name: "Open windows" }), "choosing an item closes the menu").toBeNull();
    await press(barTitle("Open windows"));
    expect(within(screen.getByRole("menu", { name: "Open windows" })).getByRole("menuitemradio", { name: /Owlhead/ })).toHaveAttribute("aria-checked", "true");
  });

  it("hides the window in front from the windows menu", async () => {
    renderOn("mac");
    await press(barTitle("Open windows"));
    await press(within(screen.getByRole("menu", { name: "Open windows" })).getByRole("menuitem", { name: /^Hide/ }));
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
  });

  it("names Sign in and Sign up in words in the menu bar, and Sign up opens the guestbook", async () => {
    const { container } = renderOn("mac");
    const bar = container.querySelector<HTMLElement>("[data-slot=menu-bar] [data-slot=account-buttons]")!;
    expect(within(bar).getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
    await press(within(bar).getByRole("button", { name: "Sign up" }));
    expect(screen.getByRole("region", { name: "guestbook.cgi" })).toHaveAttribute("data-front", "true");
  });

  it("opens the apps from the owl menu, and walks the menu bar with the arrow keys", async () => {
    renderOn("mac");
    await press(barTitle("Owlhead"));
    const owl = screen.getByRole("menu", { name: "Owlhead" });
    expect(within(owl).getByRole("menuitem", { name: "About Owlhead…" })).toHaveFocus();
    expect(within(owl).getByRole("menuitem", { name: /Sign in/ })).toHaveAttribute("href", "/login");
    fireEvent.keyDown(owl, { key: "ArrowRight" });
    await act(frame);
    expect(screen.queryByRole("menu", { name: "Owlhead" })).toBeNull();
    const file = screen.getByRole("menu", { name: "File" });
    expect(within(file).getByRole("menuitem", { name: "Open Owlhead Home Page" })).toHaveFocus();
    fireEvent.keyDown(file, { key: "ArrowLeft" });
    await act(frame);
    expect(within(screen.getByRole("menu", { name: "Owlhead" })).getByRole("menuitem", { name: "About Owlhead…" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("menu", { name: "Owlhead" }), { key: "ArrowLeft" });
    await act(frame);
    expect(screen.getByRole("menu", { name: "Open windows" }), "and wraps around").toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    await act(frame);
    expect(screen.queryByRole("menu", { name: "Open windows" })).toBeNull();
    expect(barTitle("Open windows"), "Escape gives focus back to the menu's title").toHaveFocus();
    await press(barTitle("Owlhead"));
    await press(within(screen.getByRole("menu", { name: "Owlhead" })).getByRole("menuitem", { name: /Guestbook/ }));
    expect(win("guestbook.cgi")).toHaveAttribute("data-front", "true");
  });

  it("calls the Recycle Bin the Trash, sets it at the bottom right, and empties it from Special", async () => {
    renderOn("mac");
    const right = screen.getByRole("list", { name: "Desktop, right" });
    expect(within(right).getByRole("button", { name: "Trash" })).toBeInTheDocument();
    await press(barTitle("Special"));
    await press(within(screen.getByRole("menu", { name: "Special" })).getByRole("menuitem", { name: "Empty Trash" }));
    await press(barTitle("Special"));
    expect(within(screen.getByRole("menu", { name: "Special" })).getByRole("menuitem", { name: "Empty Trash" })).toHaveAttribute("aria-disabled", "true");
  });

  it("keeps dark mode in Special, checked while it is on", async () => {
    renderOn("mac");
    await press(barTitle("Special"));
    const dark = within(screen.getByRole("menu", { name: "Special" })).getByRole("menuitemcheckbox", { name: "Dark" });
    expect(dark).toHaveAttribute("aria-checked", "false");
    await press(dark);
    expect(document.documentElement.dataset.mode).toBe("dark");
    await press(barTitle("Special"));
    expect(within(screen.getByRole("menu", { name: "Special" })).getByRole("menuitemcheckbox", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  });

  it("names the hard disk Owlhead HD at the top of the icons, and greys out the Edit menu, since nothing here can be edited", async () => {
    renderOn("mac");
    const icons = within(screen.getByRole("list", { name: "Desktop" })).getAllByRole("button");
    expect(icons[0]).toHaveAccessibleName("Owlhead HD");
    await press(icons[0]);
    expect(win("Owlhead Home Page")).toBeVisible();
    await press(barTitle("Edit"));
    const edits = within(screen.getByRole("menu", { name: "Edit" })).getAllByRole("menuitem");
    expect(edits.map((e) => e.textContent)).toEqual(["Undo", "Cut", "Copy", "Paste", "Clear"]);
    edits.forEach((e) => expect(e).toHaveAttribute("aria-disabled", "true"));
  });

  it("closes the window in front from File", async () => {
    renderOn("mac");
    await press(barTitle("File"));
    await press(within(screen.getByRole("menu", { name: "File" })).getByRole("menuitem", { name: "Close window" }));
    expect(screen.queryByRole("region", { name: "Owlhead Home Page" })).toBeNull();
  });

  it("keeps the sign-in window's close box a link home", () => {
    render(
      <DesktopStyleProvider initial="mac">
        <Logon title="Sign in to Owlhead">form</Logon>
      </DesktopStyleProvider>,
    );
    const close = screen.getByRole("link", { name: "Close, back to the Owlhead home page" });
    expect(close).toHaveAttribute("href", "/");
    expect(close.closest("[data-slot=title-bar]")).not.toBeNull();
  });
});
