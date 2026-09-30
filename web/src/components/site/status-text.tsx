"use client";

import { useEffect, useRef, useState } from "react";

const IDLE = "Document: Done";

/**
 * The status bar's message. As a browser of the time did, it shows the address of the link under the
 * pointer or the keyboard, and says the document is done otherwise.
 */
export function StatusText({ origin, className }: { origin: string; className?: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [text, setText] = useState(IDLE);

  useEffect(() => {
    const root = ref.current?.closest("[data-slot=browser]");
    if (!root) return;
    const linkOf = (event: Event) => (event.target instanceof Element ? event.target.closest("a[href]") : null);
    const show = (event: Event) => {
      const link = linkOf(event);
      if (link) setText(new URL(link.getAttribute("href")!, origin).href);
    };
    const clear = (event: Event) => {
      if (linkOf(event)) setText(IDLE);
    };
    root.addEventListener("pointerover", show);
    root.addEventListener("pointerout", clear);
    root.addEventListener("focusin", show);
    root.addEventListener("focusout", clear);
    return () => {
      root.removeEventListener("pointerover", show);
      root.removeEventListener("pointerout", clear);
      root.removeEventListener("focusin", show);
      root.removeEventListener("focusout", clear);
    };
  }, [origin]);

  return (
    <span ref={ref} className={className} data-slot="status-text">
      {text}
    </span>
  );
}
