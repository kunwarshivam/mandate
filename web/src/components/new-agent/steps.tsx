"use client";

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** The id of the flow's heading, so the flow can move focus to it when it starts over. */
export const STEP_HEADING = "new-agent-step";

export function StepHeading({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h1 id={STEP_HEADING} tabIndex={-1} className={cn("text-h1 text-balance outline-none", className)}>
      {children}
    </h1>
  );
}
