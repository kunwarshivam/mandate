import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as agent from "@/app/(app)/agents/[agentId]/page";
import * as approval from "@/app/(app)/approvals/[approvalId]/page";
import * as design from "@/app/(app)/design/page.dev";
import * as setup from "@/app/(app)/new/page";
import * as home from "@/app/(app)/page";
import * as settings from "@/app/(app)/settings/page";
import { AppShell } from "@/components/shell/app-shell";
import { StopControl } from "@/components/shell/stop-control";
import { recordHref } from "@/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { canOpen, homeFor, routeNeeds } from "@/lib/access";
import { ROLES, type Role, can } from "@/lib/roles";
import { SCREENS, agentHref } from "@/lib/screens";
import { dockStop, isDisabled, renderWithRuntime } from "@/test/harness";
import { pageFor } from "@/test/app-routes";
import { setPathname } from "@/test/navigation";

const AGENT = AGENT_IDS.swing;
const CONNECTION = buildWorkspace("normal").connection.connection_id;

/** The kill-switch and release record screens (D10, D11). */
const STOP_RECORDS = [recordHref("kill", AGENT_IDS.btc), recordHref("release", AGENT_IDS.btc), recordHref("stop_all", CONNECTION), recordHref("close_all", CONNECTION)];

const PATHS = [...SCREENS.map((s) => s.href), agentHref(AGENT), `/approvals/${APPROVAL_IDS.swingXyz}`, "/design", ...STOP_RECORDS];

async function renderPath(path: string) {
  setPathname(path);
  const page = await pageFor(path);
  return renderWithRuntime(<AppShell>{page}</AppShell>);
}

beforeEach(() => setPathname("/"));

describe("route coverage", () => {
  it.each(PATHS)("%s resolves to a page with the paper badge and an enabled Stop", async (path) => {
    await renderPath(path);
    const [header] = screen.getAllByRole("banner");
    expect(within(header).getByText("PAPER")).toBeInTheDocument();
    expect(isDisabled(dockStop())).toBe(false);
    expect(within(screen.getByRole("main")).getAllByRole("heading", { level: 1 }).length).toBeGreaterThan(0);
  });

  it.each(PATHS)("%s renders no link to a path without a page", async (path) => {
    await renderPath(path);
    for (const href of internalHrefs()) await expect(pageFor(href), href).resolves.toBeDefined();
  });

  it("sends an agent id of the wrong shape, and the old tabs, to not found", async () => {
    await expect(pageFor("/agents/nope")).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(pageFor(`/agents/${AGENT}/orders`)).rejects.toThrow("no page");
    await expect(pageFor("/messages")).rejects.toThrow("no page");
  });
});

describe("titles", () => {
  const generic = /^[A-Z][a-z]+( [a-z]+)*$/;

  it("keeps static titles generic: no agent, instrument or amount", () => {
    for (const mod of [home, setup, agent, approval, settings, design]) {
      const title = mod.metadata.title;
      const text = typeof title === "object" && title && "absolute" in title ? title.absolute.replace(/ · Owlhead$/, "") : String(title);
      expect(text).toMatch(generic);
    }
  });
});

/** PX-11, restated here without `routeNeeds`, so the two can disagree and fail. */
function mayOpen(role: Role, path: string): boolean {
  const stop = /\/(kill-switch|release|stop-all|close-all)$/.test(path);
  const workspace = path === "/settings" || path === "/design";
  switch (role) {
    case "owner":
    case "operator":
      return true;
    case "approver":
    case "viewer":
      return !stop;
    case "auditor":
      return workspace;
    default: {
      const unhandled: never = role;
      throw new Error(`unhandled role ${String(unhandled)}`);
    }
  }
}

function internalHrefs(root: ParentNode = document): string[] {
  return Array.from(root.querySelectorAll("a[href]"))
    .map((a) => a.getAttribute("href") ?? "")
    .filter((h) => h.startsWith("/") && !h.startsWith("//"))
    .map((h) => h.split("#")[0].split("?")[0]);
}

const ROLE_IDS = ROLES.map((r) => r.id);

describe("role-based route access (PX-11)", () => {
  it("has an access rule for every path with a page", () => {
    for (const path of PATHS) expect(routeNeeds(path), path).not.toBeNull();
  });

  it.each(ROLE_IDS.flatMap((role) => PATHS.map((path) => [role, path] as const)))(
    "as %s, %s renders the page only if the role may open it, and every link on screen goes where the role may go",
    async (role, path) => {
      setPathname(path);
      const page = await pageFor(path);
      renderWithRuntime(<AppShell>{page}</AppShell>, "normal", { role });
      const open = mayOpen(role, path);
      expect(canOpen(role, path)).toBe(open);

      const main = screen.getByRole("main");
      const denied = main.querySelector("[data-slot=access-denied]");
      if (open) {
        expect(denied).toBeNull();
      } else {
        expect(denied).not.toBeNull();
        expect(main.children).toHaveLength(1);
        expect(within(main).getByRole("heading", { level: 1 })).toHaveTextContent("Not available to your role");
        expect(within(denied as HTMLElement).getByRole("link")).toHaveAttribute("href", homeFor(role).href);
      }

      for (const href of internalHrefs()) expect(canOpen(role, href), `${href} shown at ${path}`).toBe(true);
      const [owl] = screen.getAllByRole("link", { name: /^Owlhead, / });
      expect(owl).toHaveAttribute("href", homeFor(role).href);
    },
  );

  it("sends an auditor to Settings, the one screen theirs", async () => {
    setPathname("/");
    renderWithRuntime(<AppShell>{await pageFor("/")}</AppShell>, "normal", { role: "auditor" });
    const denied = screen.getByRole("main").querySelector("[data-slot=access-denied]")!;
    expect(within(denied as HTMLElement).getByRole("link", { name: "Go to the settings" })).toHaveAttribute("href", "/settings");
  });

  it.each(ROLE_IDS.filter((role) => can(role, "stop.open")).flatMap((role) => ["/", `/agents/${AGENT_IDS.btc}`].map((path) => [role, path] as const)))(
    "as %s, the Stop sheet at %s links only to record screens the role may open",
    (role, path) => {
      setPathname(path);
      renderWithRuntime(<StopControl />, "normal", { role });
      fireEvent.click(screen.getByRole("button", { name: "Stop" }));
      const links = internalHrefs(screen.getByRole("dialog"));
      if (!can(role, "stop.full")) expect(links).toEqual([]);
      for (const href of links) expect(canOpen(role, href), href).toBe(true);
    },
  );

  it.each(ROLE_IDS)("as %s, the dock and command palette list exactly the screens the role may open", (role) => {
    for (const s of SCREENS) expect(can(role, s.needs), s.href).toBe(canOpen(role, s.href));
  });
});
