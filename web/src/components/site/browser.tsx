"use client";

import { type KeyboardEvent, type MouseEvent, type ReactNode, createContext, useContext, useState } from "react";
import { flushSync } from "react-dom";
import { Key } from "pixelarticons/react/Key.js";
import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import { APP_ORIGIN, APP_SCREENS, AppTab } from "./app-tab";
import { type Guide, LocationField, MenuBar, Toolbar } from "./browser-chrome";
import { PIXEL, PLAIN_BUTTON, RAISED, SUNKEN } from "./letter";
import { OpenApp } from "./open-app";
import { StatusText } from "./status-text";

type Tab = "site" | "app";

const TABS: { id: Tab; title: string }[] = [
  { id: "site", title: "Owlhead Home Page" },
  { id: "app", title: "Inside the app" },
];

const ShowTab = createContext<(tab: Tab) => void>(() => {});

const tabId = (tab: Tab) => `browser-tab-${tab}`;

/** A control in the page that brings one of the browser's tabs to the front, and the keyboard with it. */
export function TabLink({ tab, className, children }: { tab: Tab; className?: string; children: ReactNode }) {
  const show = useContext(ShowTab);
  return (
    <button type="button" onClick={() => show(tab)} className={className} data-shows={tab}>
      {children}
    </button>
  );
}

/** The row under the address, where a browser kept its guides. Here they open parts of the page, or its windows on the desktop. */
export const DIRECTORY: Guide[] = [
  { label: "What's New?", href: "#status" },
  { label: "What's Cool?", app: "record" },
  { label: "Handbook", href: "#how" },
  { label: "Questions", app: "questions" },
];

/**
 * The page open in a browser of 1996, inside its desktop window: menus, toolbar, the address, the
 * guide buttons, two tabs, and a status bar that shows where a link goes. The first tab is the home
 * page, which scrolls on its own; the second is the app itself (DEC-905). What a browser could do
 * for this page works; what it could not is greyed out, as it was then. The status bar alone is
 * scenery, hidden from assistive technology. A link to a part of the home page brings its tab back.
 */
export function Browser({ address, bookmarks, children }: { address: string; bookmarks: { id: string; title: string }[]; children: ReactNode }) {
  const [tab, setTab] = useState<Tab>("site");
  const [at, setAt] = useState(0);
  const shown = tab === "site" ? address : `${APP_ORIGIN}${APP_SCREENS[at].path}`;

  const show = (next: Tab) => {
    flushSync(() => setTab(next));
    document.getElementById(tabId(next))?.focus({ preventScroll: true });
  };

  const backToPage = (e: MouseEvent<HTMLDivElement>) => {
    if (tab !== "site" && e.target instanceof Element && e.target.closest('a[href^="#"]')) flushSync(() => setTab("site"));
  };

  const tabKeys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    show(tab === "site" ? "app" : "site");
  };

  return (
    <ShowTab value={show}>
      <div className="flex min-h-0 flex-1 flex-col" data-slot="browser" onClickCapture={backToPage}>
        <MenuBar bookmarks={bookmarks} directory={DIRECTORY} />

        <div className="shrink-0 border-t border-b border-t-card border-b-foreground/40 px-1.5 py-1.5">
          <div className="flex items-stretch justify-between gap-2">
            <Toolbar />
            <span aria-hidden className={cn(SUNKEN, "grid w-14 shrink-0 place-items-center bg-foreground text-highlight")}>
              <BrandOwl className="size-8" />
            </span>
          </div>

          <div className="mt-1.5 flex items-center gap-2">
            <span aria-hidden className={cn("shrink-0 text-[0.9375rem]", PIXEL)}>
              Location:
            </span>
            <LocationField address={shown} />
          </div>

          <nav aria-label="Guides" className="mt-1.5">
            <ul className="grid grid-cols-2 gap-1 sm:flex sm:flex-wrap">
              {DIRECTORY.map((d) => (
                <li key={d.label} className="grid">
                  {"href" in d ? (
                    <a href={d.href} className={PLAIN_BUTTON}>
                      {d.label}
                    </a>
                  ) : (
                    <OpenApp app={d.app} className={PLAIN_BUTTON}>
                      {d.label}
                    </OpenApp>
                  )}
                </li>
              ))}
            </ul>
          </nav>
        </div>

        <div role="tablist" aria-label="Pages" onKeyDown={tabKeys} className={cn("flex shrink-0 gap-1 pt-1 pb-0.5", PIXEL)} data-slot="browser-tabs">
          {TABS.map((t) => {
            const on = tab === t.id;
            return (
              <button
                key={t.id}
                type="button"
                role="tab"
                id={tabId(t.id)}
                aria-selected={on}
                aria-controls={`${tabId(t.id)}-panel`}
                tabIndex={on ? 0 : -1}
                onClick={() => show(t.id)}
                className={cn(
                  on ? cn(SUNKEN, "bg-card") : cn(RAISED, "bg-muted"),
                  "flex h-8 min-w-0 cursor-pointer items-center gap-1.5 px-2.5 text-[0.9375rem] outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground sm:w-56",
                )}
              >
                <BrandOwl className="size-4 shrink-0" />
                <span className="truncate">{t.title}</span>
              </button>
            );
          })}
        </div>

        <div
          role="tabpanel"
          id={`${tabId("site")}-panel`}
          aria-labelledby={tabId("site")}
          hidden={tab !== "site"}
          className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto overscroll-contain bg-card")}
          data-scroll-root
        >
          {children}
        </div>
        <div
          role="tabpanel"
          id={`${tabId("app")}-panel`}
          aria-labelledby={tabId("app")}
          hidden={tab !== "app"}
          className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto overscroll-contain bg-card")}
        >
          <AppTab at={at} onAt={setAt} />
        </div>

        <div aria-hidden className={cn("mt-0.5 flex shrink-0 gap-0.5 text-[0.875rem]", PIXEL)}>
          <span className={cn(SUNKEN, "grid w-8 shrink-0 place-items-center")}>
            <Key className="size-6" />
          </span>
          <StatusText origin={shown} className={cn(SUNKEN, "min-w-0 flex-1 truncate px-2 py-0.5")} />
          <span className={cn(SUNKEN, "hidden w-56 truncate px-2 py-0.5 sm:block")}>{tab === "site" ? "Private beta" : "Example workspace, paper money"}</span>
        </div>
      </div>
    </ShowTab>
  );
}
