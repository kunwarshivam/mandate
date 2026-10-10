"use client";

import { type MouseEvent, type ReactNode, createContext, useContext } from "react";
import type { AppId } from "./windows";

/**
 * How the page inside the home window opens the desktop's other windows; the desktop provides it.
 * The click, when there is one, is where the window zooms out of.
 */
export const OpenAppContext = createContext<(id: AppId, e?: MouseEvent<Element>) => void>(() => {});

/** A control in the page that opens one of the desktop's windows, as its icon on the desktop does. */
export function OpenApp({ app, className, children }: { app: AppId; className?: string; children: ReactNode }) {
  const open = useContext(OpenAppContext);
  return (
    <button type="button" onClick={(e) => open(app, e)} className={className} data-opens={app}>
      {children}
    </button>
  );
}
