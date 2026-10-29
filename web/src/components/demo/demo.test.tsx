import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { type ReactNode, isValidElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { isPublicPath } from "@/lib/auth-routes";
import { pageFor, pathsFor } from "@/test/app-routes";
import { BySource } from "@/components/api-screens/api-screens";
import { DemoApp } from "./demo-app";
import { DEMO_PATH, DEMO_SOURCE, addressPath, isDemoCommand, isDemoReport } from "./demo-messages";
import { demoPath, demoScreen } from "./demo-routes";

/**
 * The example app in the landing page's browser tab (DEC-906): the app's own screens over the fixture
 * workspace, at a public path that reads nothing from a request.
 */

/** An element as a tree of types and props, without the debug fields React adds in development. */
function shape(node: ReactNode): unknown {
  if (!isValidElement(node)) return node;
  const { children, ...props } = node.props as { children?: ReactNode };
  return { type: node.type, props, children: Array.isArray(children) ? children.map(shape) : shape(children) };
}

/**
 * A page as Next renders it over the fixtures: its component called, down to the screen it returns. A
 * page that picks its screen by the data source (DEC-1096) shows its fixture screen here, as `/demo`'s
 * fixture runtime does.
 */
async function rendered(node: ReactNode): Promise<ReactNode> {
  if (isValidElement(node) && node.type === BySource) return rendered((node.props as { fixtures: ReactNode }).fixtures);
  if (isValidElement(node) && typeof node.type === "function" && node.type.name.endsWith("Page"))
    return rendered(await (node.type as (p: unknown) => ReactNode | Promise<ReactNode>)(node.props));
  return node;
}

const SCENARIOS: Scenario[] = ["approvals", "normal"];

describe("the example app's routes", () => {
  it("shows the screen the app's own page shows, at every path the example workspace can reach", async () => {
    for (const scenario of SCENARIOS)
      for (const path of pathsFor(scenario).filter((p) => p !== "/design")) {
        const screen = demoScreen(path);
        expect(screen, path).not.toBeNull();
        expect(shape(screen), path).toEqual(shape(await rendered(await pageFor(path))));
      }
  });

  it("has no screen for an address the app has none for, an ID that is not one, or a page that is not the app's", () => {
    for (const path of [
      "/design",
      "/palette",
      "/demo",
      "/welcome",
      "/login",
      "/agents",
      "/agents/nope",
      "/agents/new/x",
      "/positions",
      "/messages",
      "/approvals/apr_bad",
      "/positions/x",
      "/alerts/x",
      "/messages/x",
      "/connections/x/stop-all",
      "/settings/nope",
      "/nowhere",
    ])
      expect(demoScreen(path), path).toBeNull();
  });

  it("reads the path alone, without its query, fragment or trailing slash", () => {
    expect(demoPath("/agents/?x=1#top")).toBe("/agents");
    expect(demoPath("?x=1")).toBe("/");
    expect(demoPath("/")).toBe("/");
  });
});

describe("what the browser and the app say to each other", () => {
  it("turns a typed address into a path in the app, and refuses anything that leaves it", () => {
    expect(addressPath("app.owlhead.ai/agents")).toBe("/agents");
    expect(addressPath("  https://app.owlhead.ai/approvals  ")).toBe("/approvals");
    expect(addressPath("HTTP://APP.OWLHEAD.AI")).toBe("/");
    expect(addressPath("/positions")).toBe("/positions");
    for (const typed of [
      "",
      "   ",
      "agents",
      "https://elsewhere.example/x",
      "app.owlhead.ai.elsewhere.example/x",
      "//elsewhere.example",
      "/a b",
      "/a\\b",
      "/a\u0000",
      "javascript:alert(1)",
    ])
      expect(addressPath(typed), typed).toBeNull();
  });

  it("takes only well-formed messages of its own kind", () => {
    expect(isDemoReport({ source: DEMO_SOURCE, type: "at", path: "/", back: false, forward: true })).toBe(true);
    expect(isDemoReport({ source: DEMO_SOURCE, type: "at", path: "agents", back: false, forward: true })).toBe(false);
    expect(isDemoReport({ source: "other", type: "at", path: "/", back: false, forward: false })).toBe(false);
    expect(isDemoReport({ source: DEMO_SOURCE, type: "at", path: "/" })).toBe(false);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "where" })).toBe(true);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "go", to: "restart" })).toBe(true);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "go", to: "away" })).toBe(false);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "open", path: "/agents" })).toBe(true);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "open", path: "//elsewhere.example" })).toBe(false);
    expect(isDemoCommand({ source: DEMO_SOURCE, type: "open", path: "https://elsewhere.example" })).toBe(false);
    expect(isDemoCommand(null)).toBe(false);
    expect(isDemoCommand("go")).toBe(false);
  });
});

describe("the example app", () => {
  afterEach(() => vi.restoreAllMocks());

  const at = () => document.querySelector("[data-demo-path]")?.getAttribute("data-demo-path");

  async function ask(data: unknown, origin = window.location.origin, source: MessageEventSource | null = window.parent) {
    await act(async () => window.dispatchEvent(new MessageEvent("message", { data, origin, source })));
  }

  it("moves between its screens by its own links, keeping its history in memory and the page's address as it was", async () => {
    const address = window.location.href;
    render(<DemoApp workspace={buildWorkspace("approvals")} />);
    expect(at()).toBe("/");
    const link = document.querySelector<HTMLAnchorElement>('main a[href="/new"]')!;
    await act(async () => fireEvent.click(link));
    expect(at()).toBe("/new");
    expect(screen.getByRole("heading", { level: 1, name: "New agent" })).toBeInTheDocument();
    expect(window.location.href).toBe(address);
  });

  it("goes back, forward, to an address, and starts over when the browser asks, and says where it is", async () => {
    const said = vi.spyOn(window.parent, "postMessage");
    render(<DemoApp workspace={buildWorkspace("approvals")} />);
    await ask({ source: DEMO_SOURCE, type: "open", path: "/settings" });
    await ask({ source: DEMO_SOURCE, type: "open", path: "/new" });
    expect(at()).toBe("/new");
    await ask({ source: DEMO_SOURCE, type: "go", to: "back" });
    expect(at()).toBe("/settings");
    await ask({ source: DEMO_SOURCE, type: "go", to: "forward" });
    expect(at()).toBe("/new");
    await ask({ source: DEMO_SOURCE, type: "where" });
    expect(said).toHaveBeenLastCalledWith({ source: DEMO_SOURCE, type: "at", path: "/new", back: true, forward: false }, window.location.origin);
    await ask({ source: DEMO_SOURCE, type: "go", to: "restart" });
    expect(at()).toBe("/");
    await ask({ source: DEMO_SOURCE, type: "where" });
    expect(said).toHaveBeenLastCalledWith({ source: DEMO_SOURCE, type: "at", path: "/", back: false, forward: false }, window.location.origin);
  });

  it("ignores what does not come from its browser on this origin", async () => {
    render(<DemoApp workspace={buildWorkspace("approvals")} />);
    await ask({ source: DEMO_SOURCE, type: "open", path: "/agents" }, "https://elsewhere.example");
    await ask({ source: DEMO_SOURCE, type: "open", path: "/agents" }, window.location.origin, null);
    await ask({ type: "open", path: "/agents" });
    expect(at()).toBe("/");
  });

  it("holds its clock still, so a request waiting in it never runs out", async () => {
    vi.useFakeTimers();
    render(<DemoApp workspace={buildWorkspace("approvals")} />);
    await ask({ source: DEMO_SOURCE, type: "open", path: "/approvals" });
    const before = document.body.textContent;
    await act(async () => vi.advanceTimersByTime(20 * 60_000));
    expect(document.body.textContent).toBe(before);
    vi.useRealTimers();
  });

  it("says so at an address with no screen, with a way back to your agents and nothing more", async () => {
    render(<DemoApp workspace={buildWorkspace("approvals")} />);
    await ask({ source: DEMO_SOURCE, type: "open", path: "/nowhere" });
    const missing = screen.getByRole("heading", { level: 1, name: "No such page" });
    expect(missing.parentElement!.textContent).toBe("No such pageGo to your agents");
    await act(async () => fireEvent.click(screen.getByRole("link", { name: "Go to your agents" })));
    expect(at()).toBe("/");
  });
});

describe("the example app reads nothing from an account", () => {
  const ROOTS = ["src/app/demo", "src/components/demo"];
  const files = ROOTS.flatMap((root) => readdirSync(join(process.cwd(), root), { recursive: true, encoding: "utf8" }).map((f) => join(root, f))).filter(
    (f) => /\.tsx?$/.test(f) && !f.endsWith(".test.tsx"),
  );

  it("imports no session, cookie, request or workspace loader, and fetches nothing", () => {
    expect(files.length).toBeGreaterThanOrEqual(4);
    for (const f of files) {
      const source = readFileSync(join(process.cwd(), f), "utf8");
      for (const banned of ["@/lib/supabase", "@/lib/get-workspace", "next/headers", "cookies(", "fetch(", "@/lib/beta-store"])
        expect(source.includes(banned), `${f} uses ${banned}`).toBe(false);
    }
  });

  it("builds its page from the fixture workspace alone, with no signed-in user", () => {
    const page = readFileSync(join(process.cwd(), "src/app/demo/page.tsx"), "utf8");
    expect(page).toMatch(/<DemoApp workspace=\{buildWorkspace\("approvals"\)\} \/>/);
    expect(readFileSync(join(process.cwd(), "src/components/demo/demo-app.tsx"), "utf8")).toContain("<SessionProvider session={null}>");
  });

  it("is the one public path of its kind: the page itself, and nothing under it", () => {
    expect(isPublicPath(DEMO_PATH)).toBe(true);
    for (const path of ["/demo/", "/demo/agents", "/demox"]) expect(isPublicPath(path), path).toBe(false);
  });
});
