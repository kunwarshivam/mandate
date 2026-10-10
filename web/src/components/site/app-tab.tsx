"use client";

import { type RefObject, useEffect, useState } from "react";
import { DEMO_PATH, DEMO_SOURCE, type DemoAsk, addressPath, isDemoReport } from "@/components/demo/demo-messages";
import type { Nav } from "./browser-chrome";

export const APP_ORIGIN = "https://app.owlhead.ai";

/** What the page's `<html>` carries that the app draws by: the theme, light or dark, and the colour-blind palette. */
const THEME_ATTRIBUTES = ["class", "style", "data-mode", "data-theme-pref", "data-cvd"];

type Where = { path: string; back: boolean; forward: boolean };

/** The app in the tab: where it is, whether it has drawn yet, the browser's controls for it, and the address bar's way in. */
export type AppTab = {
  where: Where;
  ready: boolean;
  nav: Nav;
  go: (typed: string) => void;
  /** Asks the app where it is once its document has loaded, and dresses it in the page's theme. */
  loaded: () => void;
};

function tell(frame: RefObject<HTMLIFrameElement | null>, ask: DemoAsk) {
  frame.current?.contentWindow?.postMessage({ source: DEMO_SOURCE, ...ask }, window.location.origin);
}

function dress(frame: RefObject<HTMLIFrameElement | null>) {
  const inside = frame.current?.contentDocument?.documentElement;
  if (!inside) return;
  for (const name of THEME_ATTRIBUTES) {
    const value = document.documentElement.getAttribute(name);
    if (value === null) inside.removeAttribute(name);
    else inside.setAttribute(name, value);
  }
}

/**
 * The browser's side of its app tab (DEC-906): it hears where the app in the frame is, and sends it
 * back, forward, home, to a typed address, or to start over. It takes reports only from its own frame
 * and this origin. The app wears the page's theme, kept in step while the visitor changes it.
 */
export function useAppTab(frame: RefObject<HTMLIFrameElement | null>): AppTab {
  const [where, setWhere] = useState<Where>({ path: "/", back: false, forward: false });
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const hear = (e: MessageEvent) => {
      if (e.origin !== window.location.origin || e.source !== frame.current?.contentWindow || !isDemoReport(e.data)) return;
      setWhere({ path: e.data.path, back: e.data.back, forward: e.data.forward });
      setReady(true);
      dress(frame);
    };
    const themes = new MutationObserver(() => dress(frame));
    themes.observe(document.documentElement, { attributes: true, attributeFilter: THEME_ATTRIBUTES });
    window.addEventListener("message", hear);
    tell(frame, { type: "where" });
    return () => {
      themes.disconnect();
      window.removeEventListener("message", hear);
    };
  }, [frame]);

  return {
    where,
    ready,
    nav: {
      back: where.back ? () => tell(frame, { type: "go", to: "back" }) : null,
      forward: where.forward ? () => tell(frame, { type: "go", to: "forward" }) : null,
      home: () => tell(frame, { type: "open", path: "/" }),
      reload: () => tell(frame, { type: "go", to: "restart" }),
    },
    go: (typed) => {
      const path = addressPath(typed);
      if (path) tell(frame, { type: "open", path });
    },
    loaded: () => {
      dress(frame);
      tell(frame, { type: "where" });
    },
  };
}

/**
 * The browser's second tab: the app itself, live, on the example workspace and paper money
 * (DEC-906). Every screen, link and button in it works as it does in the app, against fixtures, and
 * nothing in it reaches an account. It draws in a frame of its own, so its keyboard, its layers and
 * its routing stay out of the page around it.
 */
export function AppFrame({ frame, tab }: { frame: RefObject<HTMLIFrameElement | null>; tab: AppTab }) {
  return (
    <iframe
      ref={frame}
      src={DEMO_PATH}
      title="Owlhead app, example workspace"
      onLoad={tab.loaded}
      className="block size-full border-0 bg-background transition-opacity duration-300 data-[ready=false]:opacity-0"
      data-ready={tab.ready}
      data-slot="app-frame"
    />
  );
}
