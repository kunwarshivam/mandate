"use client";

import { useState } from "react";
import Link from "next/link";
import { ArrowLeft } from "@phosphor-icons/react";
import { ModeBadge } from "@/components/domain/mode";
import { ScreenSkeleton } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { type CommandRecord, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { CommandEntry, UnreachableAlert } from "./command-log";
import type { RecordKind } from "./commands";
import { KillSwitchButton } from "./kill-switch-button";
import { RECORD_TITLE, type RecordList, type StopRecord, buildRecord, recordLines } from "./record";
import { StepUpDialog } from "./step-up-dialog";

const BACK = "inline-flex h-11 w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground lg:h-8";

function List({ list }: { list: RecordList }) {
  const id = `record-${list.key}`;
  return (
    <section aria-labelledby={id} data-list={list.key} className="grid gap-2 bg-card px-3 py-3 sm:px-4">
      <h2 id={id} className="text-heading">
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
          className="press grid min-h-11 w-full gap-1 border-2 border-foreground bg-card px-4 py-3 text-left text-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
        >
          <span className="text-base font-bold">Stop and release to me</span>
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
    <section data-slot="record-missing" className="grid gap-3 border-t-4 border-foreground bg-muted p-4 sm:p-6">
      <p className="max-w-prose">{agent ? "This workspace has no agent with that ID." : "This workspace has no broker connection with that ID."}</p>
      <Link href="/" className="press inline-flex h-11 w-fit items-center border-2 border-foreground bg-card px-4 font-bold hover:bg-muted">
        Go to the dashboard
      </Link>
    </section>
  );
}

/**
 * D10 and D11. A record screen, so a page and never a modal (brief §4.1): everything the owner
 * confirms is shown expanded, fixed at first render, and sent with the command. The passkey check
 * (G3) opens from this page; the Stop sheet only links here.
 */
export function StopRecordScreen({ kind, targetId }: { kind: RecordKind; targetId: string }) {
  const { ws, send, commands } = useRuntime();
  const full = useCan("stop.full");
  const [record, setRecord] = useState<StopRecord | null>(null);
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [sent, setSent] = useState<string[]>([]);

  const live = ws.status === "ready" && record === null ? buildRecord(kind, ws, targetId) : null;
  if (live) setRecord(live);
  const shown = record ?? live;

  const confirm = () => {
    if (!shown) return;
    const artifact: CommandRecord = { screen: kind === "release" ? "D11" : "D10", environment: ws.environment, title: shown.title, shown: recordLines(shown) };
    const command = send(kind, shown.agentId, artifact);
    setNotice(null);
    setSent((ids) => [...ids, command.id]);
  };

  return (
    <article className="grid w-full max-w-3xl grid-cols-1 gap-(--seam)" aria-labelledby="record-title" data-slot="record-screen" data-kind={kind}>
      {shown ? (
        <Link href={shown.back.href} className={BACK}>
          <ArrowLeft className="size-4" aria-hidden />
          {shown.back.label}
        </Link>
      ) : null}

      <header className="reveal grid gap-2 bg-card px-3 py-3 sm:px-4 sm:py-4">
        <h1 id="record-title" className="flex flex-wrap items-center gap-3 text-title sm:text-display">
          {RECORD_TITLE[kind]}
          <EnvironmentBadge environment={ws.environment} />
        </h1>
        {shown ? <p className="text-base font-bold">{shown.subject}</p> : null}
      </header>

      {ws.status === "loading" ? <ScreenSkeleton rows={1} /> : null}
      {ws.status === "unreachable" ? <UnreachableAlert /> : null}
      {ws.status === "ready" && !shown ? <Missing kind={kind} /> : null}

      {shown ? (
        <>
          <p className="bg-card px-3 py-3 text-base sm:px-4" data-slot="record-scope">
            {shown.scope}
          </p>
          {shown.warning ? (
            <p role="note" data-slot="release-warning" className="border-t-4 border-foreground bg-marigold px-3 py-3 font-bold text-marigold-foreground sm:px-4">
              {shown.warning}
            </p>
          ) : null}
          {shown.lists.map((list) => (
            <List key={list.key} list={list} />
          ))}
          <p className="bg-card px-3 py-3 text-sm sm:px-4">{shown.afterwards}</p>

          <section aria-labelledby="record-now" className="grid gap-2 bg-card px-3 py-3 sm:px-4">
            <h2 id="record-now" className="label-caps text-muted-foreground">
              Right now
            </h2>
            <ul className="grid gap-1.5">
              {shown.agentIds.map((id) => {
                const agent = ws.agents.find((a) => a.agent_id === id);
                return agent ? (
                  <li key={id} className="flex items-center justify-between gap-3 text-sm">
                    {agent.label}
                    <ModeBadge mode={agent.mode} />
                  </li>
                ) : null;
              })}
            </ul>
          </section>

          <section aria-label="Confirm" className="grid gap-2 border-t-4 border-foreground bg-muted px-3 py-3 sm:px-4">
            {shown.nothingToDo ? (
              <p className="text-sm" data-slot="nothing-to-do">
                {shown.nothingToDo}
              </p>
            ) : !full ? (
              <p className="text-sm">Stopping and closing are for an owner or operator.</p>
            ) : (
              <>
                <Activate record={shown} onClick={() => setAsking(true)} />
                <Link href={shown.back.href} className="press inline-flex h-11 w-fit items-center px-1 text-sm font-bold underline underline-offset-4 hover:bg-card">
                  Back without changing anything
                </Link>
              </>
            )}
            <section aria-label="What happened" className="grid gap-2">
              <div role="status" aria-live="polite" className="grid gap-2">
                {notice ? <p className="bg-card px-4 py-3 text-sm">{notice}</p> : null}
                {commands
                  .filter((c) => sent.includes(c.id))
                  .map((c) => (
                    <CommandEntry key={c.id} command={c} label={shown.label} />
                  ))}
              </div>
            </section>
          </section>

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
