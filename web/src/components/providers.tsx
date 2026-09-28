"use client";

import type { ReactNode } from "react";
import { MotionConfig } from "motion/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Workspace } from "@/fixtures/types";
import { RuntimeProvider } from "@/lib/mock-runtime";

export function Providers({ workspace, children }: { workspace: Workspace; children: ReactNode }) {
  return (
    <MotionConfig reducedMotion="user">
      <TooltipProvider delayDuration={300}>
        <RuntimeProvider initial={workspace}>{children}</RuntimeProvider>
      </TooltipProvider>
    </MotionConfig>
  );
}
