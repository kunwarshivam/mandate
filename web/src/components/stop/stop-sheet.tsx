"use client";

import { type ReactNode, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { ChevronDown, CircleAlert, Unplug } from "lucide-react";
import { ModeBadge, SourceTag } from "@/components/domain/mode";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import type { Agent } from "@/fixtures/types";
import { clock, quantity } from "@/lib/format";
import { type CommandKind, useRuntime } from "@/lib/mock-runtime";
import { commandTitle, needsStepUp, recordedLine, stepUpLine } from "./commands";
import { StepUpDialog } from "./step-up-dialog";

type Tone = "ink" | "outline" | "kill" | "kill-outline";

/** Pausing is ink, like a paused agent; the kill switch is the only crimson in the product. */
const TONE: Record<Tone, string> = {
  ink: "bg-ink text-ink-foreground hover:bg-ink/85",
  outline: "border-2 border-foreground bg-card text-foreground hover:bg-muted",
  kill: "bg-crimson text-crimson-foreground hover:bg-crimson/90",
  "kill-outline": "border-2 border-crimson bg-card text-foreground hover:bg-muted",
};

/** Every choice stays enabled in every state: loading, stale, unreachable, or mid-command. */
function Choice({ tone, title, children, onClick }: { tone: Tone; title: string; children?: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      data-tone={tone}
      onClick={onClick}
      className={cn("press grid min-h-11 w-full gap-1 px-4 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2", TONE[tone])}
    >
      <span className="text-base font-bold">{title}</span>
      {children ? <span className={cn("text-sm", tone === "ink" || tone === "kill" ? "" : "text-muted-foreground")}>{children}</span> : null}
    </button>
  );
}

interface Pending {
  kind: CommandKind;
  agent: Agent | null;
}

function AgentChoices({ agent, choose }: { agent: Agent; choose: (kind: CommandKind, agent: Agent) => void }) {
  if (agent.mode === "stopped") {
    return <p className="bg-ink px-4 py-3 text-sm text-ink-foreground">Stopped. There is nothing more to stop for this agent.</p>;
  }
  const holding = agent.positions.length > 0;
  const ownerPaused = agent.restrictions.some((r) => r.code === "owner_pause");
  const reconciling = agent.startup === "reconciling";
  const unknown = agent.orders.some((o) => o.state === "Unknown");
  return (
    <div className="grid gap-(--seam)">
      {agent.mode === "paused" ? null : (
        <Choice tone="ink" title={`Pause ${agent.label}`} onClick={() => choose("pause", agent)}>
          No new orders. Resting protection stays. You can resume later.
        </Choice>
      )}
      {ownerPaused ? (
        <Choice tone="outline" title={`Resume ${agent.label}`} onClick={() => choose("resume", agent)}>
          It trades again within its mandate. Needs your passkey.
        </Choice>
      ) : null}
      {reconciling ? (
        <p className="grid gap-1.5 bg-lapis-soft px-4 py-3 text-sm" data-source="account">
          <SourceTag source="account" />
          Checking with the broker. It resumes on its own once the check confirms; nothing is needed from you.
        </p>
      ) : null}
      {unknown ? (
        <p className="grid gap-1.5 bg-lapis-soft px-4 py-3 text-sm" data-source="account">
          <span className="flex items-center gap-2">
            <SourceTag source="account" />
            <CircleAlert className="size-4 shrink-0" aria-hidden />
          </span>
          An order has an unknown state. Exits in that instrument are held until the broker answers; the kill switch still works.
        </p>
      ) : null}
      {holding ? (
        <>
          <Choice tone="kill" title="Kill switch: close and stop" onClick={() => choose("kill", agent)}>
            Cancels only this agent&apos;s orders, sells only its positions, and ends it. Other agents and your own holdings are untouched. Needs your passkey.
          </Choice>
          <Choice tone="outline" title="Stop and release positions to me" onClick={() => choose("release", agent)}>
            Ends the agent and makes its positions yours, <strong className="font-semibold text-foreground">without protection</strong>: its protective orders are canceled and
            nothing watches {agent.positions.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(" and ")}. Needs your passkey.
          </Choice>
        </>
      ) : (
        <>
          <Choice tone="outline" title={`Stop ${agent.label}`} onClick={() => choose("stop", agent)}>
            Ends the agent for good. It holds nothing, so nothing is sold. Needs your passkey.
          </Choice>
          <Choice tone="kill" title="Kill switch: cancel and stop" onClick={() => choose("kill", agent)}>
            Cancels this agent&apos;s orders and ends it. It holds nothing to sell. Needs your passkey.
          </Choice>
        </>
      )}
    </div>
  );
}

export function StopSheet({ open, onOpenChange, agentId }: { open: boolean; onOpenChange: (open: boolean) => void; agentId: string | null }) {
  const { ws, reachable, commands, send, now } = useRuntime();
  const [pending, setPending] = useState<Pending | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [shownOpen, setShownOpen] = useState(open);
  if (open !== shownOpen) {
    setShownOpen(open);
    if (!open) setPending(null);
  }
  const contentRef = useRef<HTMLDivElement>(null);
  const contextAgent = agentId ? ws.agents.find((a) => a.agent_id === agentId) ?? null : null;
  const own = ws.external_positions;

  const dispatch = (kind: CommandKind, agent: Agent | null) => {
    setNotice(null);
    send(kind, agent?.agent_id ?? null);
  };

  const choose = (kind: CommandKind, agent: Agent | null) => {
    if (needsStepUp(kind)) setPending({ kind, agent });
    else dispatch(kind, agent);
  };

  const labelFor = (id: string | null) => (id ? ws.agents.find((a) => a.agent_id === id)?.label ?? "The agent" : "Account");

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        ref={contentRef}
        tabIndex={-1}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          contentRef.current?.focus();
        }}
        className="gap-0 overflow-y-auto overscroll-contain outline-none data-[side=right]:w-full data-[side=right]:sm:max-w-md"
        closeClassName="text-ink-foreground hover:bg-ink-foreground/15 hover:text-ink-foreground"
        data-slot="stop-sheet"
      >
        <SheetHeader className="gap-1.5 bg-ink px-4 pt-4 pb-4 pr-14 text-ink-foreground">
          <SheetTitle className="text-display text-ink-foreground">Stop</SheetTitle>
          <SheetDescription className="text-ink-foreground">Pausing is the least drastic and comes first. Paper account; simulated funds.</SheetDescription>
        </SheetHeader>

        <div className="grid gap-(--section-gap) p-4">
          {reachable ? null : (
            <Alert className="gap-1.5 rounded-none border-0 border-t-4 border-foreground bg-muted px-4 py-3 text-foreground" data-slot="unreachable">
              <Unplug aria-hidden />
              <AlertTitle className="text-base font-bold">Cannot reach your deployment</AlertTitle>
              <AlertDescription className="grid gap-2 text-foreground">
                <p>
                  It has not answered since {clock(ws.health.deployment.as_of)}. A request from here cannot be delivered, and the screen will say so rather than show it as done.
                </p>
                <p>
                  To stop trading now, go to the broker directly: sign in to your Alpaca paper dashboard, cancel open orders, and close positions there. Protective orders already
                  resting at the broker stay in place until you cancel them.
                </p>
              </AlertDescription>
            </Alert>
          )}

          {ws.status === "loading" ? (
            <p className="bg-muted px-4 py-3 text-sm">Agent details are still loading. The choices for the whole account below work without them.</p>
          ) : null}

          {contextAgent ? (
            <section className="grid gap-(--block-gap)" aria-labelledby="stop-this-agent">
              <h3 id="stop-this-agent" className="flex items-center justify-between gap-2 border-b-2 border-foreground pb-1.5 text-heading">
                This agent: {contextAgent.label}
                <ModeBadge mode={contextAgent.mode} />
              </h3>
              <AgentChoices agent={contextAgent} choose={choose} />
            </section>
          ) : ws.agents.length > 0 ? (
            <section className="grid" aria-labelledby="stop-one-agent">
              <h3 id="stop-one-agent" className="border-b-2 border-foreground pb-1.5 text-heading">
                One agent
              </h3>
              {ws.agents.map((agent) => (
                <Collapsible key={agent.agent_id} className="border-b">
                  <CollapsibleTrigger className="group flex min-h-14 w-full items-center justify-between gap-3 px-1 py-2.5 text-left outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring">
                    <span className="grid">
                      <span className="font-bold">{agent.label}</span>
                      <span className="text-caption text-muted-foreground" translate="no">
                        {agent.mandate.name}
                      </span>
                    </span>
                    <span className="flex items-center gap-2">
                      <ModeBadge mode={agent.mode} />
                      <ChevronDown className="size-5 transition-transform duration-200 ease-(--ease-in-out) group-data-[state=open]:rotate-180" aria-hidden />
                    </span>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="pb-3">
                    <AgentChoices agent={agent} choose={choose} />
                  </CollapsibleContent>
                </Collapsible>
              ))}
            </section>
          ) : null}

          <section className="grid gap-(--seam)" aria-labelledby="stop-account">
            <h3 id="stop-account" className="flex items-baseline justify-between gap-2 border-b-2 border-foreground pb-1.5 text-heading">
              Everything on this account
            </h3>
            <p className="mb-1 flex items-center gap-2 text-caption text-muted-foreground">
              <span className="bg-lapis px-1.5 label-caps text-lapis-foreground">Account</span>
              {ws.connection.broker}
            </p>
            <Choice tone="ink" title="Pause all agents on this account" onClick={() => choose("pause_all", null)}>
              No new orders from any agent. Resting protection stays. No passkey needed.
            </Choice>
            <Choice tone="kill" title="Stop all agents on this account" onClick={() => choose("stop_all", null)}>
              Each agent&apos;s own kill switch: cancels its orders, sells its positions, and ends it. Your own holdings are untouched. Needs your passkey.
            </Choice>
            <Choice tone="kill-outline" title="Close everything on this account" onClick={() => choose("close_all", null)}>
              <span className="grid gap-1">
                <span>The broker&apos;s cancel-all and close-all. It:</span>
                <span className="grid list-disc gap-0.5 pl-4 [&>span]:list-item">
                  <span>cancels every open order on the account, including ones Owlhead did not place;</span>
                  <span>
                    closes every position, including your own
                    {own.length > 0 ? ` ${own.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(", ")}` : " holdings"} that no agent manages;
                  </span>
                  <span>ends every agent.</span>
                </span>
                <span>Needs your passkey.</span>
              </span>
            </Choice>
          </section>

          <section aria-label="What happened" className="grid gap-2">
            <div role="status" aria-live="polite" className="grid gap-2">
              {notice ? <p className="bg-muted px-4 py-3 text-sm">{notice}</p> : null}
              {commands.map((c) => (
                <p key={c.id} data-phase={c.phase} className="reveal border-t-2 border-foreground bg-muted px-4 py-3 text-sm">
                  <span className="font-bold">{commandTitle(c.kind, labelFor(c.agentId))}.</span>{" "}
                  {c.phase === "sent" ? "Sent; waiting for the runtime to record it." : null}
                  {c.phase === "recorded" ? `Recorded at ${clock(c.recordedAt ?? now)}. ${recordedLine(c.kind, labelFor(c.agentId))}` : null}
                  {c.phase === "undelivered"
                    ? "Not delivered: your deployment did not answer, so nothing changed from this request. Use the broker directly, as described above."
                    : null}
                </p>
              ))}
            </div>
          </section>
        </div>

        <StepUpDialog
          open={pending !== null}
          action={pending ? stepUpLine(pending.kind, pending.agent?.label ?? "", pending.agent?.positions.length ?? 0) : ""}
          onVerified={() => {
            if (pending) dispatch(pending.kind, pending.agent);
            setPending(null);
          }}
          onCancel={() => {
            setPending(null);
            setNotice("Passkey check canceled. Nothing was sent.");
          }}
          onFailed={() => {
            setPending(null);
            setNotice("Passkey check failed. Nothing was sent.");
          }}
        />
      </SheetContent>
    </Sheet>
  );
}
