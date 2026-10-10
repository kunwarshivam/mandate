"use client";

import { type ReactNode, createContext, useContext, useState } from "react";
import { type DesktopStyle, writeDesktopStyle } from "@/lib/desktop-style";
import { cn } from "@/lib/utils";
import styles from "./letter.module.css";

const DesktopStyleContext = createContext<{ style: DesktopStyle; setStyle: (s: DesktopStyle) => void }>({ style: "windows", setStyle: () => {} });

/**
 * The signed-out pages' frame carries the desktop style as `data-os`, which the letter's Mac rules
 * key on (`letter.module.css`), and hands it to the components whose shape differs.
 */
export function DesktopStyleProvider({ initial, className, children }: { initial: DesktopStyle; className?: string; children: ReactNode }) {
  const [style, set] = useState(initial);
  const setStyle = (s: DesktopStyle) => {
    writeDesktopStyle(s);
    set(s);
  };
  return (
    <DesktopStyleContext value={{ style, setStyle }}>
      <div data-os={style} className={cn(styles.frame, className)}>
        {children}
      </div>
    </DesktopStyleContext>
  );
}

export function useDesktopStyle(): DesktopStyle {
  return useContext(DesktopStyleContext).style;
}

export function useSetDesktopStyle(): (s: DesktopStyle) => void {
  return useContext(DesktopStyleContext).setStyle;
}
