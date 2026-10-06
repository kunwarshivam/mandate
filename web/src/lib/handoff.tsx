"use client";

import { type ReactNode, createContext, useContext, useMemo, useRef } from "react";

/**
 * Words the owner typed in Messages or the copilot, carried into agent setup's message field
 * (DEC-479). Held in memory until setup has read them, then cleared; never written to browser storage.
 */
export interface Handoff {
  give: (text: string) => void;
  peek: () => string | null;
  clear: () => void;
}

const NONE: Handoff = { give: () => {}, peek: () => null, clear: () => {} };

const HandoffContext = createContext<Handoff>(NONE);

export function HandoffProvider({ children }: { children: ReactNode }) {
  const held = useRef<string | null>(null);
  const value = useMemo<Handoff>(
    () => ({
      give: (text) => {
        held.current = text;
      },
      peek: () => held.current,
      clear: () => {
        held.current = null;
      },
    }),
    [],
  );
  return <HandoffContext.Provider value={value}>{children}</HandoffContext.Provider>;
}

export function useHandoff(): Handoff {
  return useContext(HandoffContext);
}
