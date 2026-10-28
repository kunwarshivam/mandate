"use client";

import { type ReactNode, forwardRef, useEffect, useMemo, useState } from "react";
import NextLink from "next/link";
import { MotionConfig } from "motion/react";
import { Toasty } from "@cloudflare/kumo/components/toast";
import { TooltipProvider } from "@cloudflare/kumo/components/tooltip";
import { KumoLocaleProvider, LinkProvider, type LinkComponentProps } from "@cloudflare/kumo/utils";
import { JournalToasts } from "@/components/shell/journal-toasts";
import type { Workspace } from "@/fixtures/types";
import { type Resolved, resolveWorkspace } from "@/api/memberships";
import { emptyWorkspace } from "@/api/reads";
import type { SessionKeeper } from "@/api/session";
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
      {source && source.workspaceId !== null ? (
        <ApiRuntimeProvider source={{ origin: source.origin, workspaceId: source.workspaceId }} session={session ?? undefined}>
          {body}
        </ApiRuntimeProvider>
      ) : source ? (
        <FromMemberships origin={source.origin} session={session}>
          {body}
        </FromMemberships>
      ) : (
        <RuntimeProvider initial={emptyWorkspace("unreachable", new Date().toISOString())} tick={false}>
          {body}
        </RuntimeProvider>
      )}
    </Frame>
  );
}

/** What the screens hold while no workspace is named: nothing, in the state that says why. */
const UNNAMED: Readonly<Record<Exclude<Resolved, { ok: true }>["reason"] | "loading", Workspace["status"]>> = {
  loading: "loading",
  signed_out: "signed_out",
  none: "membership_unavailable",
  several: "membership_unavailable",
  unavailable: "unreachable",
};

/**
 * The workspace from the session's memberships (`GET /v1/me/workspaces`, DEC-2113), when the
 * configuration names none. The session is opened first, as every read needs it. Exactly one
 * membership is read; none or several hold an empty workspace that says so, and take no command.
 */
function FromMemberships({ origin, session, children, fetch }: { origin: string; session: SessionKeeper | null; children: ReactNode; fetch?: (input: string, init: RequestInit) => Promise<Response> }) {
  const [resolved, setResolved] = useState<Resolved | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      const opened = session ? await session.open() : "open";
      if (opened !== "open" && opened !== "outage") return live && setResolved({ ok: false, reason: opened === "unreachable" ? "unavailable" : "signed_out" });
      const answer = await resolveWorkspace(fetch ?? ((input, init) => window.fetch(input, init)), origin);
      if (live) setResolved(answer);
    })();
    return () => {
      live = false;
    };
  }, [origin, session, fetch]);
  if (resolved?.ok) {
    return (
      <ApiRuntimeProvider source={{ origin, workspaceId: resolved.workspaceId }} session={session ?? undefined}>
        {children}
      </ApiRuntimeProvider>
    );
  }
  const status = UNNAMED[resolved ? resolved.reason : "loading"];
  return (
    <RuntimeProvider key={status} initial={emptyWorkspace(status, new Date().toISOString())} tick={false} offline>
      {children}
    </RuntimeProvider>
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
