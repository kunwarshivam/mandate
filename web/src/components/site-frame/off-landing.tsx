"use client";

import type { ReactNode } from "react";
import { usePathname } from "next/navigation";

/** The landing page is one plain document with its own title, so the site header steps aside there. */
export function OffLanding({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  return pathname === "/" || pathname === "/welcome" ? null : children;
}
