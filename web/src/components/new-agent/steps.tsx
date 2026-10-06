"use client";

import type { ReactNode } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { ArrowLeft } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { KEY } from "@/components/kumo/key";

/** The id of every step's heading, so the flow can move focus to it when the step changes. */
export const STEP_HEADING = "new-agent-step";

/** A step's heading, the one focus target when the step changes. */
export function StepHeading({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h1 id={STEP_HEADING} tabIndex={-1} className={cn("text-h1 text-balance outline-none", className)}>
      {children}
    </h1>
  );
}

export function BackButton({ onClick, children = "Back" }: { onClick: () => void; children?: ReactNode }) {
  return (
    <Button type="button" variant="secondary" size="lg" className={cn("w-fit", KEY)} onClick={onClick}>
      <ArrowLeft className="size-4" aria-hidden />
      {children}
    </Button>
  );
}
