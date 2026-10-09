"use client";

import { type ReactNode, useRef, useState } from "react";
import Link from "next/link";
import { Collapsible } from "@cloudflare/kumo/primitives/collapsible";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { ChevronDown } from "pixelarticons/react/ChevronDown.js";
import { Close } from "pixelarticons/react/Close.js";
import { SquareAlert } from "pixelarticons/react/SquareAlert.js";
import { StopOctagon } from "@/components/icon";
import { cn } from "@/lib/utils";
import { ModeBadge, SourceTag } from "@/components/domain/mode";
import { AgentOwl } from "@/components/domain/owl";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { Agent } from "@/fixtures/types";
import { quantity } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { CommandEntry, UnreachableAlert } from "./command-log";
import { type SheetKind, needsStepUp, recordHref, stepUpLine } from "./commands";
import { KillSwitchButton } from "./kill-switch-button";
import { StepUpDialog } from "./step-up-dialog";

type Tone = "ink" | "outline";

/**
 * The Stop sheet popup's `data-slot`. The command palette reads it so that its focus return never
 * runs while the sheet is open (rule 13).
 */
export const STOP_SHEET_SLOT = "stop-sheet";

/**
 * Pausing is ink, like a paused agent. The kill switch is `KillSwitchButton`, the only crimson. The
 * account's three choices share one weight, an outlined row each, so the most drastic is not also
 * the loudest (DEC-515): pausing comes first and reads first.
 */
const TONE: Record<Tone, string> = {
  ink: "bg-ink text-ink-foreground hover:bg-ink/85",
  outline: "border-2 border-foreground/40 bg-card text-foreground hover:bg-background",
};

/**
 * Every choice stays enabled in every state: loading, stale, unreachable, or mid-command. A choice
 * with `href` opens its record screen, a page, and closes the sheet on the way (brief §4.1).
 */
function Choice({ tone, title, children, onClick, href }: { tone: Tone; title: string; children?: ReactNode; onClick: () => void; href?: string }) {
  const className = cn("press grid min-h-11 w-full gap-1 rounded-xl px-4 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2", TONE[tone]);
  const body = (
    <>
      <span className="text-base font-semibold">{title}</span>
      {children ? <span className={cn("text-sm", tone === "ink" ? "" : "text-muted-foreground")}>{children}</span> : null}
    </>
  );
  return href ? (
    <Link href={href} data-tone={tone} onClick={onClick} className={className}>
      {body}
    </Link>
  ) : (
    <button type="button" data-tone={tone} onClick={onClick} className={className}>
      {body}
    </button>
  );
}

interface Pending {
  kind: SheetKind;
  agent: Agent | null;
}

function AgentChoices({ agent, choose, leave, full }: { agent: Agent; choose: (kind: SheetKind, agent: Agent) => void; leave: () => void; full: boolean }) {
  if (agent.mode === "stopped") {
    return <p className="rounded-xl bg-ink px-4 py-3 text-sm text-ink-foreground">Stopped. There is nothing more to stop for this agent.</p>;
  }
  if (!full) {
    return agent.mode === "paused" ? (
      <p className="rounded-xl bg-background px-4 py-3 text-sm">Paused. Your role can pause; an owner or operator can resume or stop it.</p>
    ) : (
      <div className="grid gap-2">
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
    <div className="grid gap-2">
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
        <p className="grid gap-1.5 rounded-xl bg-lapis-soft px-4 py-3 text-sm" data-source="account">
          <SourceTag source="account" />
          Checking with the broker. It resumes on its own once the check confirms; nothing is needed from you.
        </p>
      ) : null}
      {unknown ? (
        <p className="grid gap-1.5 rounded-xl bg-lapis-soft px-4 py-3 text-sm" data-source="account">
          <span className="flex items-center gap-2">
            <SourceTag source="account" />
            <SquareAlert className="size-6 shrink-0" aria-hidden />
          </span>
          An order has an unknown state. Exits in that instrument are held until the broker answers; the kill switch still works.
        </p>
      ) : null}
      {holding ? (
        <>
          <KillSwitchButton title="Kill switch: close and stop" href={recordHref("kill", agent.agent_id)} onClick={leave}>
            Cancels only this agent&apos;s orders, sells only its positions, and ends it. Other agents and your own holdings are untouched. Opens the full list to confirm with
            your passkey.
          </KillSwitchButton>
          <Choice tone="outline" title="Stop and release positions to me" href={recordHref("release", agent.agent_id)} onClick={leave}>
            Ends the agent and makes its positions yours, <strong className="font-semibold text-foreground">without protection</strong>: its protective orders are canceled and
            nothing watches {agent.positions.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(" and ")}. Opens the full list to confirm with your passkey.
          </Choice>
        </>
      ) : (
        <>
          <Choice tone="outline" title={`Stop ${agent.label}`} onClick={() => choose("stop", agent)}>
            Ends the agent for good. It holds nothing, so nothing is sold. Needs your passkey.
          </Choice>
          <KillSwitchButton title="Kill switch: cancel and stop" href={recordHref("kill", agent.agent_id)} onClick={leave}>
            Cancels this agent&apos;s orders and ends it. It holds nothing to sell. Opens the full list to confirm with your passkey.
          </KillSwitchButton>
        </>
      )}
    </div>
  );
}

export function StopSheet({ open, onOpenChange, agentId }: { open: boolean; onOpenChange: (open: boolean) => void; agentId: string | null }) {
  const { ws, reachable, commands, send } = useRuntime();
  const full = useCan("stop.full");
  const [pending, setPending] = useState<Pending | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [shownOpen, setShownOpen] = useState(open);
  if (open !== shownOpen) {
    setShownOpen(open);
    if (!open) setPending(null);
  }
  const popupRef = useRef<HTMLDivElement>(null);
  const contextAgent = agentId ? ws.agents.find((a) => a.agent_id === agentId) ?? null : null;
  const own = ws.external_positions;

  const focusOnMount = (node: HTMLDivElement | null) => {
    popupRef.current = node;
    node?.focus();
  };

  const dispatch = (kind: SheetKind, agent: Agent | null) => {
    setNotice(null);
    send(kind, agent?.agent_id ?? null);
  };

  const choose = (kind: SheetKind, agent: Agent | null) => {
    if (needsStepUp(kind)) setPending({ kind, agent });
    else dispatch(kind, agent);
  };

  const leave = () => onOpenChange(false);

  const labelFor = (id: string | null) => (id ? ws.agents.find((a) => a.agent_id === id)?.label ?? "The agent" : "Account");

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-50 bg-ink/40" />
        <Dialog.Popup
          ref={focusOnMount}
          initialFocus={popupRef}
          data-slot={STOP_SHEET_SLOT}
          className="fixed inset-y-0 right-0 z-50 flex w-full flex-col overflow-y-auto overscroll-contain border-l border-border bg-card text-foreground shadow-2xl outline-none sm:max-w-md sm:rounded-l-3xl"
        >
          <div className="grid gap-2 border-b border-border/70 px-5 pt-6 pr-16 pb-5">
            <Dialog.Title className="flex flex-wrap items-center gap-3 text-h1">
              <StopOctagon aria-hidden className="size-6 text-ink" />
              Stop
              <EnvironmentBadge environment={ws.environment} />
            </Dialog.Title>
            <Dialog.Description className="text-muted-foreground">Pausing is the least drastic and comes first. Paper account; simulated funds.</Dialog.Description>
            <Dialog.Close
              aria-label="Close"
              className="absolute top-4 right-4 grid size-11 place-items-center rounded-lg text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
            >
              <Close className="size-6" aria-hidden />
            </Dialog.Close>
          </div>

          <div className="grid gap-8 px-5 pt-6 pb-8">
            {reachable ? null : <UnreachableAlert />}

            {ws.status === "loading" ? (
              <p className="rounded-xl bg-background px-4 py-3 text-sm">Agent details are still loading. The choices for the whole account below work without them.</p>
            ) : null}

            {contextAgent ? (
              <section className="grid gap-(--block-gap)" aria-labelledby="stop-this-agent">
                <h3 id="stop-this-agent" className="flex items-center justify-between gap-2 text-h3">
                  This agent: {contextAgent.label}
                  <ModeBadge mode={contextAgent.mode} />
                </h3>
                <AgentChoices agent={contextAgent} choose={choose} leave={leave} full={full} />
              </section>
            ) : ws.agents.length > 0 ? (
              <section className="grid" aria-labelledby="stop-one-agent">
                <h3 id="stop-one-agent" className="pb-1 text-h3">
                  One agent
                </h3>
                {ws.agents.map((agent) => (
                  <Collapsible.Root key={agent.agent_id} className="border-b border-border/70 last:border-b-0">
                    <Collapsible.Trigger className="group -mx-2 flex min-h-14 w-[calc(100%+1rem)] items-center justify-between gap-3 rounded-lg px-2 py-2.5 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring">
                      <AgentOwl agent={agent} className="size-8 shrink-0" />
                      <span className="grid grow">
                        <span className="font-medium">{agent.label}</span>
                        <span className="text-caption text-muted-foreground" translate="no">
                          {agent.mandate.name}
                        </span>
                      </span>
                      <span className="flex items-center gap-2">
                        <ModeBadge mode={agent.mode} />
                        <ChevronDown className="size-6 text-muted-foreground transition-transform duration-200 ease-(--ease-in-out) group-data-[panel-open]:rotate-180" aria-hidden />
                      </span>
                    </Collapsible.Trigger>
                    <Collapsible.Panel className="pb-3">
                      <AgentChoices agent={agent} choose={choose} leave={leave} full={full} />
                    </Collapsible.Panel>
                  </Collapsible.Root>
                ))}
              </section>
            ) : null}

            <section className="grid gap-3" aria-labelledby="stop-account">
              <h3 id="stop-account" className="flex items-baseline justify-between gap-2 text-h3">
                Everything on this account
              </h3>
              <p className="mb-1 flex items-center gap-2 text-caption text-muted-foreground">
                <span className="inline-flex h-6 items-center rounded-md bg-lapis-soft px-2.5 text-label text-lapis">Account</span>
                {ws.connection.broker}
              </p>
              <Choice tone="outline" title="Pause all agents on this account" onClick={() => choose("pause_all", null)}>
                No new orders from any agent. Resting protection stays. No passkey needed.
              </Choice>
              {full ? (
                <>
                  <KillSwitchButton appearance="outline" title="Stop all agents on this account" href={recordHref("stop_all", ws.connection.connection_id)} onClick={leave}>
                    Each agent&apos;s own kill switch: cancels its orders, sells its positions, and ends it. Your own holdings are untouched. Opens the full list to confirm with
                    your passkey.
                  </KillSwitchButton>
                  <KillSwitchButton appearance="outline" title="Close everything on this account" href={recordHref("close_all", ws.connection.connection_id)} onClick={leave}>
                    The broker&apos;s cancel-all and close-all: it cancels every open order, including ones Owlhead did not place, closes every position, including your own
                    {own.length > 0 ? ` ${own.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(", ")}` : " holdings"} that no agent manages, and ends every agent. Opens
                    the full list to confirm with your passkey.
                  </KillSwitchButton>
                </>
              ) : (
                <p className="px-1 text-sm text-muted-foreground">Stopping and closing are for an owner or operator.</p>
              )}
            </section>

            <section aria-label="What happened" className="grid gap-2">
              <div role="status" aria-live="polite" className="grid gap-2">
                {notice ? <p className="rounded-xl bg-background px-4 py-3 text-sm">{notice}</p> : null}
                {commands.map((c) => (
                  <CommandEntry key={c.id} command={c} label={labelFor(c.agentId)} />
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
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
