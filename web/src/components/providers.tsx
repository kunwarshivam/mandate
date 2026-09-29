"use client";

import { type ReactNode, forwardRef } from "react";
import NextLink from "next/link";
import { MotionConfig } from "motion/react";
import { Toasty } from "@cloudflare/kumo/components/toast";
import { TooltipProvider } from "@cloudflare/kumo/components/tooltip";
import { KumoLocaleProvider, LinkProvider, type LinkComponentProps } from "@cloudflare/kumo/utils";
import { JournalToasts } from "@/components/shell/journal-toasts";
import type { Workspace } from "@/fixtures/types";
import { RuntimeProvider } from "@/lib/mock-runtime";

const AppLink = forwardRef<HTMLAnchorElement, LinkComponentProps>(function AppLink({ href, to, ...rest }, ref) {
  return <NextLink ref={ref} href={href ?? to ?? "#"} {...rest} />;
});

export function Providers({
  workspace,
  children,
  recordAfterMs,
  tick,
}: {
  workspace: Workspace;
  children: ReactNode;
  recordAfterMs?: number;
  tick?: boolean;
}) {
  return (
    <MotionConfig reducedMotion="user">
      <KumoLocaleProvider>
        <LinkProvider component={AppLink}>
          <TooltipProvider delay={300}>
            <Toasty>
              <RuntimeProvider initial={workspace} recordAfterMs={recordAfterMs} tick={tick}>
                {children}
                <JournalToasts />
              </RuntimeProvider>
            </Toasty>
          </TooltipProvider>
        </LinkProvider>
      </KumoLocaleProvider>
    </MotionConfig>
  );
}
