"use client";

import { useState } from "react";
import Link from "next/link";
import { ArrowLeft } from "@phosphor-icons/react";
import { ModeBadge } from "@/components/domain/mode";
import { ScreenSkeleton } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { useFrozen } from "@/lib/frozen";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { CommandEntry, UnreachableAlert } from "./command-log";
import type { RecordKind } from "./commands";
import { KillSwitchButton } from "./kill-switch-button";
import { type AgentModeLine, MODES_HEADING, RECORD_TITLE, type RecordList, type StopRecord, buildRecord, commandRecord } from "./record";
import { StepUpDialog } from "./step-up-dialog";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

const BACK = "-ml-2 inline-flex h-11 w-fit items-center gap-1.5 rounded-lg px-2 text-sm text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring lg:h-9";

function List({ list }: { list: RecordList }) {
  const id = `record-${list.key}`;
  return (
    <section aria-labelledby={id} data-list={list.key} className="grid gap-2 border-t border-border/70 pt-4">
      <h2 id={id} className="text-h3">
        {list.heading}
      </h2>
      {list.items.length > 0 ? (
        <ul className="grid list-disc gap-1 pl-5 text-sm">
          {list.items.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-muted-foreground">{list.empty}</p>
      )}
      {list.note ? <p className="text-sm">{list.note}</p> : null}
    </section>
  );
}

function Activate({ record, onClick }: { record: StopRecord; onClick: () => void }) {
  switch (record.kind) {
    case "kill":
      return (
        <KillSwitchButton title="Activate the kill switch" onClick={onClick}>
          Needs your passkey.
        </KillSwitchButton>
      );
    case "stop_all":
      return (
        <KillSwitchButton title="Stop all agents on this account" onClick={onClick}>
          Needs your passkey.
        </KillSwitchButton>
      );
    case "close_all":
      return (
        <KillSwitchButton appearance="outline" title="Close everything on this account" onClick={onClick}>
          Needs your passkey.
        </KillSwitchButton>
      );
    case "release":
      return (
        <button
          type="button"
          data-tone="outline"
          onClick={onClick}
          className="press grid min-h-11 w-full gap-1 rounded-xl border border-foreground/25 bg-card px-4 py-3 text-left text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
        >
          <span className="text-base font-semibold">Stop and release to me</span>
          <span className="text-sm text-muted-foreground">Needs your passkey.</span>
        </button>
      );
    default: {
      const unhandled: never = record.kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

function Missing({ kind }: { kind: RecordKind }) {
  const agent = kind === "kill" || kind === "release";
  return (
    <section data-slot="record-missing" className="grid gap-4 pt-4">
      <p className="max-w-measure text-muted-foreground">{agent ? "This workspace has no agent with that ID." : "This workspace has no broker connection with that ID."}</p>
      <Link href="/" className={cn("w-fit", KEY)}>
        Go to the dashboard
      </Link>
    </section>
  );
}

function Modes({ modes }: { modes: AgentModeLine[] }) {
  return (
    <ul className="grid gap-1.5">
      {modes.map((m) => (
        <li key={m.agentId} className="flex items-center justify-between gap-3 text-sm">
          {m.label}
          <ModeBadge mode={m.mode} />
        </li>
      ))}
    </ul>
  );
}

/**
 * D10 and D11. A record screen, so a page and never a modal (brief §4.1): everything the owner
 * confirms, mode badges included, is shown expanded, fixed at first render, and sent with the
 * command. A change before they confirm asks for a fresh render; live progress after they confirm
 * sits outside the record. The passkey check (G3) opens from this page; the Stop sheet only links here.
 */
export function StopRecordScreen({ kind, targetId }: { kind: RecordKind; targetId: string }) {
  const { ws, send, commands } = useRuntime();
  const full = useCan("stop.full");
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [sent, setSent] = useState<string[]>([]);
  const confirmed = sent.length > 0;
  const { shown, stale, refresh } = useFrozen(ws.status === "ready" ? buildRecord(kind, ws, targetId) : null, confirmed);
  if (stale && asking) setAsking(false);

  const confirm = () => {
    if (!shown || stale) return;
    const command = send(kind, shown.agentId, commandRecord(shown));
    setNotice(null);
    setSent((ids) => [...ids, command.id]);
  };

  return (
    <article className="mx-auto grid w-full max-w-2xl grid-cols-1 gap-6" aria-labelledby="record-title" data-slot="record-screen" data-kind={kind}>
      {shown ? (
        <Link href={shown.back.href} className={BACK}>
          <ArrowLeft className="size-4" aria-hidden />
          {shown.back.label}
        </Link>
      ) : null}

      <div data-slot="record" className="grid grid-cols-1 gap-5">
        <header className="reveal grid gap-2">
          <h1 id="record-title" className="flex flex-wrap items-center gap-3 text-h1">
            {RECORD_TITLE[kind]}
            <EnvironmentBadge environment={shown?.environment ?? ws.environment} />
          </h1>
          {shown ? <p className="text-lg font-medium">{shown.subject}</p> : null}
        </header>

        {shown ? (
          <>
            <p className="text-base text-pretty" data-slot="record-scope">
              {shown.scope}
            </p>
            {shown.warning ? (
              <p role="note" data-slot="release-warning" className="rounded-2xl bg-mandate px-5 py-4 font-medium text-mandate-foreground">
                {shown.warning}
              </p>
            ) : null}
            {shown.lists.map((list) => (
              <List key={list.key} list={list} />
            ))}
            <p className="border-t border-border/70 pt-4 text-sm">{shown.afterwards}</p>

            <section aria-labelledby="record-modes" className="grid gap-2 border-t border-border/70 pt-4">
              <h2 id="record-modes" className="text-sm font-medium text-muted-foreground">
                {MODES_HEADING}
              </h2>
              <Modes modes={shown.modes} />
            </section>
          </>
        ) : null}
      </div>

      {ws.status === "loading" ? <ScreenSkeleton rows={1} /> : null}
      {ws.status === "unreachable" ? <UnreachableAlert /> : null}
      {ws.status === "ready" && !shown ? <Missing kind={kind} /> : null}

      {shown ? (
        <>
          <section aria-label="Confirm" className="grid gap-3 rounded-2xl bg-background px-5 py-5">
            {confirmed ? (
              <p className="text-sm" data-slot="confirmed">
                You confirmed the record above. It stays as you saw it; progress is under “After you confirmed”.
              </p>
            ) : stale ? (
              <div data-slot="record-changed" className="grid gap-2">
                <p className="text-sm font-medium">This changed since the page opened, so the record above is out of date. Nothing was sent.</p>
                <button
                  type="button"
                  onClick={refresh}
                  className={cn("w-fit", KEY)}
                >
                  Show the current version
                </button>
              </div>
            ) : shown.nothingToDo ? (
              <p className="text-sm" data-slot="nothing-to-do">
                {shown.nothingToDo}
              </p>
            ) : !full ? (
              <p className="text-sm">Stopping and closing are for an owner or operator.</p>
            ) : (
              <>
                <Activate record={shown} onClick={() => setAsking(true)} />
                <Link href={shown.back.href} className="press -ml-2 inline-flex h-11 w-fit items-center rounded-lg px-3 text-sm font-medium text-muted-foreground outline-none hover:bg-card hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
                  Back without changing anything
                </Link>
              </>
            )}
          </section>

          <div role="status" aria-live="polite" className="grid gap-2">
            {notice ? <p className="rounded-xl bg-background px-4 py-3 text-sm">{notice}</p> : null}
            {confirmed ? (
              <section aria-labelledby="after-confirm" data-slot="after-confirm" className="grid gap-2 rounded-2xl border border-dashed border-muted-foreground px-5 py-4">
                <h2 id="after-confirm" className="text-h3">
                  After you confirmed
                </h2>
                <p className="text-sm text-muted-foreground">Live progress. It is not part of the record above.</p>
                {commands
                  .filter((c) => sent.includes(c.id))
                  .map((c) => (
                    <CommandEntry key={c.id} command={c} label={shown.label} />
                  ))}
                <Modes modes={shown.modes.map((m) => ({ ...m, mode: ws.agents.find((a) => a.agent_id === m.agentId)?.mode ?? m.mode }))} />
              </section>
            ) : null}
          </div>

          <StepUpDialog
            open={asking}
            action={shown.stepUp}
            onVerified={() => {
              setAsking(false);
              confirm();
            }}
            onCancel={() => {
              setAsking(false);
              setNotice("Passkey check canceled. Nothing was sent.");
            }}
            onFailed={() => {
              setAsking(false);
              setNotice("Passkey check failed. Nothing was sent.");
            }}
          />
        </>
      ) : null}
    </article>
  );
}
