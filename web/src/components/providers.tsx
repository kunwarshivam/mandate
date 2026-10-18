"use client";

import { type ReactNode, forwardRef, useMemo } from "react";
import NextLink from "next/link";
import { MotionConfig } from "motion/react";
import { Toasty } from "@cloudflare/kumo/components/toast";
import { TooltipProvider } from "@cloudflare/kumo/components/tooltip";
import { KumoLocaleProvider, LinkProvider, type LinkComponentProps } from "@cloudflare/kumo/utils";
import { JournalToasts } from "@/components/shell/journal-toasts";
import type { Workspace } from "@/fixtures/types";
import { emptyWorkspace } from "@/api/reads";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { browserSessionKeeper } from "@/lib/api-session";
import type { ApiSource } from "@/lib/data-source";
import { HandoffProvider } from "@/lib/handoff";
import { RuntimeProvider } from "@/lib/mock-runtime";

const AppLink = forwardRef<HTMLAnchorElement, LinkComponentProps>(function AppLink({ href, to, ...rest }, ref) {
  return <NextLink ref={ref} href={href ?? to ?? "#"} {...rest} />;
});

function Frame({ children }: { children: ReactNode }) {
  return (
    <MotionConfig reducedMotion="user">
      <KumoLocaleProvider>
        <LinkProvider component={AppLink}>
          <TooltipProvider delay={300}>
            <Toasty>{children}</Toasty>
          </TooltipProvider>
        </LinkProvider>
      </KumoLocaleProvider>
    </MotionConfig>
  );
}

/**
 * The providers over a deployment's workspace API (DEC-975). `source` is `null` when the server's
 * configuration names the API but does not parse: the fixture stand-in then holds an unreachable
 * workspace with nothing in it, which takes no command, so nothing on screen passes for a record.
 */
export function ApiProviders({
  source,
  stubSignIn = false,
  children,
}: {
  source: Pick<ApiSource, "origin" | "workspaceId"> | null;
  /** Whether `next dev`'s server holds a stub sign-in for a dev cell (DEC-2406); never true in a production build. */
  stubSignIn?: boolean;
  children: ReactNode;
}) {
  const origin = source?.origin ?? null;
  const session = useMemo(() => (origin ? browserSessionKeeper(origin, stubSignIn) : null), [origin, stubSignIn]);
  const body = (
    <HandoffProvider>
      {children}
      <JournalToasts />
    </HandoffProvider>
  );
  return (
    <Frame>
      {source ? (
        <ApiRuntimeProvider source={source} session={session ?? undefined}>
          {body}
        </ApiRuntimeProvider>
      ) : (
        <RuntimeProvider initial={emptyWorkspace("unreachable", new Date().toISOString())} tick={false}>
          {body}
        </RuntimeProvider>
      )}
    </Frame>
  );
}

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
    <Frame>
      <RuntimeProvider initial={workspace} recordAfterMs={recordAfterMs} tick={tick}>
        <HandoffProvider>
          {children}
          <JournalToasts />
        </HandoffProvider>
      </RuntimeProvider>
    </Frame>
  );
}
