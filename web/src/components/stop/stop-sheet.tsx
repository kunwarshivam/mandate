"use client";

import { type ReactNode, useRef, useState } from "react";
import { Collapsible } from "@cloudflare/kumo/primitives/collapsible";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { CaretDown, Plugs, WarningCircle, X } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { ModeBadge, SourceTag } from "@/components/domain/mode";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { Agent } from "@/fixtures/types";
import { clock, quantity } from "@/lib/format";
import { type Command, type CommandKind, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { commandTitle, needsStepUp, recordedLine, stepUpLine } from "./commands";
import { KillSwitchButton } from "./kill-switch-button";
import { StepUpDialog } from "./step-up-dialog";

type Tone = "ink" | "outline";

/** Pausing is ink, like a paused agent. The kill switch is `KillSwitchButton`, the only crimson. */
const TONE: Record<Tone, string> = {
  ink: "bg-ink text-ink-foreground hover:bg-ink/85",
  outline: "border-2 border-foreground bg-card text-foreground hover:bg-muted",
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
      {children ? <span className={cn("text-sm", tone === "ink" ? "" : "text-muted-foreground")}>{children}</span> : null}
    </button>
  );
}

interface Pending {
  kind: CommandKind;
  agent: Agent | null;
}

function AgentChoices({ agent, choose, full }: { agent: Agent; choose: (kind: CommandKind, agent: Agent) => void; full: boolean }) {
  if (agent.mode === "stopped") {
    return <p className="bg-ink px-4 py-3 text-sm text-ink-foreground">Stopped. There is nothing more to stop for this agent.</p>;
  }
  if (!full) {
    return agent.mode === "paused" ? (
      <p className="bg-muted px-4 py-3 text-sm">Paused. Your role can pause; an owner or operator can resume or stop it.</p>
    ) : (
      <div className="grid gap-(--seam)">
        <Choice tone="ink" title={`Pause ${agent.label}`} onClick={() => choose("pause", agent)}>
          No new orders. Resting protection stays. An owner or operator can resume it.
        </Choice>
      </div>
    );
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
            <WarningCircle className="size-4 shrink-0" aria-hidden />
          </span>
          An order has an unknown state. Exits in that instrument are held until the broker answers; the kill switch still works.
        </p>
      ) : null}
      {holding ? (
        <>
          <KillSwitchButton title="Kill switch: close and stop" onClick={() => choose("kill", agent)}>
            Cancels only this agent&apos;s orders, sells only its positions, and ends it. Other agents and your own holdings are untouched. Needs your passkey.
          </KillSwitchButton>
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
          <KillSwitchButton title="Kill switch: cancel and stop" onClick={() => choose("kill", agent)}>
            Cancels this agent&apos;s orders and ends it. It holds nothing to sell. Needs your passkey.
          </KillSwitchButton>
        </>
      )}
    </div>
  );
}

export function StopSheet({ open, onOpenChange, agentId }: { open: boolean; onOpenChange: (open: boolean) => void; agentId: string | null }) {
  const { ws, reachable, commands, send, now } = useRuntime();
  const full = useCan("stop.full");
  const [pending, setPending] = useState<Pending | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const contextAgent = agentId ? ws.agents.find((a) => a.agent_id === agentId) ?? null : null;
  const own = ws.external_positions;

  const focusOnMount = (node: HTMLDivElement | null) => {
    popupRef.current = node;
    node?.focus();
  };

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
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-50 bg-ink/40" />
        <Dialog.Popup
          ref={focusOnMount}
          initialFocus={popupRef}
          data-slot="stop-sheet"
          className="fixed inset-y-0 right-0 z-50 flex w-full flex-col overflow-y-auto overscroll-contain border-l-2 border-foreground bg-card text-foreground outline-none sm:max-w-md"
        >
          <div className="grid gap-1.5 bg-ink px-4 pt-4 pb-4 pr-14 text-ink-foreground">
            <Dialog.Title className="flex flex-wrap items-center gap-3 text-display text-ink-foreground">
              Stop
              <EnvironmentBadge environment={ws.environment} />
            </Dialog.Title>
            <Dialog.Description className="text-ink-foreground">Pausing is the least drastic and comes first. Paper account; simulated funds.</Dialog.Description>
            <Dialog.Close
              aria-label="Close"
              className="absolute top-3 right-3 grid size-11 place-items-center text-ink-foreground outline-none hover:bg-ink-foreground/15 focus-visible:ring-3 focus-visible:ring-ring"
            >
              <X className="size-5" aria-hidden />
            </Dialog.Close>
          </div>

          <div className="grid gap-(--section-gap) p-4">
            {reachable ? null : (
              <div role="alert" className="grid gap-1.5 border-t-4 border-foreground bg-muted px-4 py-3 text-foreground" data-slot="unreachable">
                <p className="flex items-center gap-2 text-base font-bold">
                  <Plugs className="size-4 shrink-0" aria-hidden />
                  Cannot reach your deployment
                </p>
                <div className="grid gap-2 text-sm">
                  <p>
                    It has not answered since {clock(ws.health.deployment.as_of)}. A request from here cannot be delivered, and the screen will say so rather than show it as done.
                  </p>
                  <p>
                    To stop trading now, go to the broker directly: sign in to your Alpaca paper dashboard, cancel open orders, and close positions there. Protective orders
                    already resting at the broker stay in place until you cancel them.
                  </p>
                </div>
              </div>
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
                <AgentChoices agent={contextAgent} choose={choose} full={full} />
              </section>
            ) : ws.agents.length > 0 ? (
              <section className="grid" aria-labelledby="stop-one-agent">
                <h3 id="stop-one-agent" className="border-b-2 border-foreground pb-1.5 text-heading">
                  One agent
                </h3>
                {ws.agents.map((agent) => (
                  <Collapsible.Root key={agent.agent_id} className="border-b">
                    <Collapsible.Trigger className="group flex min-h-14 w-full items-center justify-between gap-3 px-1 py-2.5 text-left outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring">
                      <span className="grid">
                        <span className="font-bold">{agent.label}</span>
                        <span className="text-caption text-muted-foreground" translate="no">
                          {agent.mandate.name}
                        </span>
                      </span>
                      <span className="flex items-center gap-2">
                        <ModeBadge mode={agent.mode} />
                        <CaretDown className="size-5 transition-transform duration-200 ease-(--ease-in-out) group-data-[panel-open]:rotate-180" aria-hidden />
                      </span>
                    </Collapsible.Trigger>
                    <Collapsible.Panel className="pb-3">
                      <AgentChoices agent={agent} choose={choose} full={full} />
                    </Collapsible.Panel>
                  </Collapsible.Root>
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
              {full ? (
                <>
                  <KillSwitchButton title="Stop all agents on this account" onClick={() => choose("stop_all", null)}>
                    Each agent&apos;s own kill switch: cancels its orders, sells its positions, and ends it. Your own holdings are untouched. Needs your passkey.
                  </KillSwitchButton>
                  <KillSwitchButton appearance="outline" title="Close everything on this account" onClick={() => choose("close_all", null)}>
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
                  </KillSwitchButton>
                </>
              ) : (
                <p className="px-1 text-sm text-muted-foreground">Stopping and closing are for an owner or operator.</p>
              )}
            </section>

            <section aria-label="What happened" className="grid gap-2">
              <div role="status" aria-live="polite" className="grid gap-2">
                {notice ? <p className="bg-muted px-4 py-3 text-sm">{notice}</p> : null}
                {commands.map((c) => (
                  <p key={c.id} data-phase={c.phase} className="reveal border-t-2 border-foreground bg-muted px-4 py-3 text-sm">
                    <span className="font-bold">{commandTitle(c.kind, labelFor(c.agentId))}.</span> <PhaseLine phase={c.phase} at={clock(c.recordedAt ?? now)} recorded={recordedLine(c.kind, labelFor(c.agentId))} />
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
          />
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function PhaseLine({ phase, at, recorded }: { phase: Command["phase"]; at: string; recorded: string }) {
  switch (phase) {
    case "sent":
      return "Sent; waiting for the runtime to record it.";
    case "recorded":
      return `Recorded at ${at}. ${recorded}`;
    case "undelivered":
      return "Not delivered: your deployment did not answer, so nothing changed from this request. Use the broker directly, as described above.";
    case "unknown":
      return "The result is unknown; we are checking. Nothing on screen changes until the journal answers.";
    default: {
      const unhandled: never = phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}
