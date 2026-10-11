"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import { AppRouterContext, type AppRouterInstance } from "next/dist/shared/lib/app-router-context.shared-runtime";
import { PathnameContext } from "next/dist/shared/lib/hooks-client-context.shared-runtime";
import { ConversationsProvider } from "@/components/messages/conversations";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import type { Workspace } from "@/fixtures/types";
import { RoleProvider } from "@/lib/roles";
import { SessionProvider } from "@/lib/session";
import { DEMO_PATH, DEMO_SOURCE, type DemoReport, isDemoCommand } from "./demo-messages";
import { demoPath, demoScreen, inMessages } from "./demo-routes";

/** Pages of the site, not the app: a link to one leaves the example for the real page. */
const SITE_PATHS = ["/login", "/welcome", "/auth/"];

type History = { stack: string[]; at: number };

function NoSuchScreen() {
  return (
    <section className="grid max-w-3xl gap-3">
      <h1 className="text-h1 sm:text-h1">No such page</h1>
      <Link href="/" className="w-fit font-semibold text-primary underline decoration-lapis/30 underline-offset-4 hover:decoration-current">
        Go to the dashboard
      </Link>
    </section>
  );
}

/**
 * The app over the example workspace, for the landing page's browser tab (DEC-906): the real shell
 * and screens on the fixture runtime, with no session and nothing from any account. It keeps its own
 * history in memory, so it never changes the page's address, and stands in for Next's router and
 * pathname, so every link, `router.push` and active tab inside moves between its screens. A link to a
 * sign-in page opens that page in the whole window. Its clock stands still: a request waiting in it
 * never runs out, and it never redraws on the main thread it shares with the page's desktop.
 */
export function DemoApp({ workspace }: { workspace: Workspace }) {
  const [history, setHistory] = useState<History>({ stack: ["/"], at: 0 });
  const [run, setRun] = useState(0);
  const path = history.stack[history.at];

  const router = useMemo<AppRouterInstance>(() => {
    const go = (href: string, replace: boolean) => {
      const next = demoPath(href);
      setHistory((h) => {
        if (replace)
          return {
            stack: h.stack.map((p, i) => (i === h.at ? next : p)),
            at: h.at,
          };
        if (h.stack[h.at] === next) return h;
        const stack = [...h.stack.slice(0, h.at + 1), next];
        return { stack, at: stack.length - 1 };
      });
      window.scrollTo(0, 0);
    };
    const step = (by: 1 | -1) =>
      setHistory((h) => ({
        ...h,
        at: Math.min(h.stack.length - 1, Math.max(0, h.at + by)),
      }));
    return {
      back: () => step(-1),
      forward: () => step(1),
      refresh: () => {},
      push: (href) => go(href, false),
      replace: (href) => go(href, true),
      prefetch: () => {},
      bfcacheId: "demo",
    };
  }, []);

  const report = useRef<DemoReport | null>(null);

  useEffect(() => {
    report.current = {
      source: DEMO_SOURCE,
      type: "at",
      path,
      back: history.at > 0,
      forward: history.at < history.stack.length - 1,
    };
    if (window.parent !== window) window.parent.postMessage(report.current, window.location.origin);
  }, [path, history]);

  useEffect(() => {
    const listen = (e: MessageEvent) => {
      if (e.origin !== window.location.origin || e.source !== window.parent || !isDemoCommand(e.data)) return;
      const c = e.data;
      switch (c.type) {
        case "where":
          if (report.current) window.parent.postMessage(report.current, window.location.origin);
          return;
        case "open":
          router.push(c.path);
          return;
        case "go":
          if (c.to === "back") router.back();
          else if (c.to === "forward") router.forward();
          else {
            setHistory({ stack: ["/"], at: 0 });
            setRun((r) => r + 1);
          }
          return;
        default: {
          const unhandled: never = c;
          throw new Error(`unhandled demo command ${JSON.stringify(unhandled)}`);
        }
      }
    };
    const follow = (e: MouseEvent) => {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      const link = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>("a[href]") : null;
      if (!link || link.hasAttribute("download") || (link.target && link.target !== "_self")) return;
      const url = new URL(link.href, window.location.href);
      if (url.origin !== window.location.origin || url.pathname === DEMO_PATH) return;
      e.preventDefault();
      if (SITE_PATHS.some((p) => url.pathname === p || (p.endsWith("/") && url.pathname.startsWith(p)))) window.top?.location.assign(url.href);
      else router.push(url.pathname);
    };
    window.addEventListener("message", listen);
    window.addEventListener("click", follow, true);
    return () => {
      window.removeEventListener("message", listen);
      window.removeEventListener("click", follow, true);
    };
  }, [router]);

  const body = (
    <div key={path} className="contents" data-demo-path={path}>
      {demoScreen(path) ?? <NoSuchScreen />}
    </div>
  );

  return (
    <AppRouterContext value={router}>
      <PathnameContext value={path}>
        <SessionProvider session={null}>
          <RoleProvider>
            <Providers key={run} workspace={workspace} tick={false}>
              <AppShell>{inMessages(path) ? <ConversationsProvider>{body}</ConversationsProvider> : body}</AppShell>
            </Providers>
          </RoleProvider>
        </SessionProvider>
      </PathnameContext>
    </AppRouterContext>
  );
}
