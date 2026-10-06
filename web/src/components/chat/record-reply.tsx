"use client";

import { useState } from "react";
import Link from "next/link";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Pause } from "pixelarticons/react/Pause.js";
import { StopOctagon } from "@/components/icon";
import { KEY } from "@/components/kumo/key";
import { ChangeReview } from "@/components/mandate/change-review";
import { openStop } from "@/components/shell/stop-control";
import { CommandEntry } from "@/components/stop/command-log";
import { findAgent } from "@/fixtures/workspace";
import type { AnswerTable, Cite, Reply } from "@/lib/ask-record";
import { type Edits, propose } from "@/lib/mandate-change";
import { type Command, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { agentHref } from "@/lib/screens";
import { cn } from "@/lib/utils";
import { Markdown, TABLE } from "./markdown";

/** A record a reply read, as a link back to it. */
const CITE =
  "press inline-flex min-h-9 items-center gap-1 rounded-full border border-border bg-card pr-2 pl-3 text-sm font-medium text-lapis outline-none hover:bg-lapis-soft focus-visible:ring-3 focus-visible:ring-ring max-lg:min-h-11";

/** A question the owner can ask next: their words, not an action. */
export const ASK_CHIP =
  "press inline-flex min-h-9 items-center rounded-full bg-background px-3.5 text-sm text-foreground outline-none hover:bg-border/60 focus-visible:ring-3 focus-visible:ring-ring max-lg:min-h-11";

/** Pause is ink, as everywhere: the least drastic way to lower risk. */
const PAUSE_KEY =
  "press inline-flex h-11 items-center gap-1.5 rounded-xl bg-ink pr-4.5 pl-3 font-semibold text-ink-foreground outline-none hover:bg-ink/85 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2";
const QUIET_KEY =
  "press inline-flex h-11 items-center rounded-xl border border-foreground/25 bg-card px-4 font-medium text-foreground outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";

/** What the owner sent, as they formatted it: Markdown, with every line break they typed. */
export function OwnerSaid({ text }: { text: string }) {
  return (
    <div data-slot="owner-message" className="ml-auto max-w-[min(85%,44rem)] min-w-0 rounded-3xl bg-background px-4 py-2.5">
      <span className="sr-only">You: </span>
      <Markdown text={text} />
    </div>
  );
}

function Cites({ cites }: { cites: Cite[] }) {
  if (cites.length === 0) return null;
  return (
    <ul aria-label="Records read" className="flex flex-wrap gap-2 pt-1">
      {cites.map((c) => (
        <li key={c.href}>
          <Link href={c.href} className={CITE}>
            {c.label}
            <ArrowRight aria-hidden className="-my-1 size-6" />
          </Link>
        </li>
      ))}
    </ul>
  );
}

/** On a phone, starter questions sit on one row that scrolls sideways, so the thread keeps the screen. */
export const ONE_ROW = "max-lg:-mx-(--page-x) max-lg:flex-nowrap max-lg:overflow-x-auto max-lg:px-(--page-x) max-lg:*:shrink-0 max-lg:*:whitespace-nowrap [scrollbar-width:none]";

export function AskChips({ asks, onAsk, label = "Ask next", className }: { asks: readonly string[]; onAsk: (text: string) => void; label?: string; className?: string }) {
  if (asks.length === 0) return null;
  return (
    <div role="group" aria-label={label} className={cn("flex flex-wrap gap-2", className)} data-slot="ask-chips">
      {asks.map((a) => (
        <button key={a} type="button" className={ASK_CHIP} onClick={() => onAsk(a)}>
          {a}
        </button>
      ))}
    </div>
  );
}

/** An answer's figures, in the same frame as a table in a message. */
function FiguresTable({ table }: { table: AnswerTable }) {
  return (
    <div data-slot="answer-table" tabIndex={0} role="region" aria-label={table.label} className={cn(TABLE.frame, "my-1")}>
      <table className={TABLE.table}>
        <caption className="sr-only">{table.label}</caption>
        <thead className={TABLE.head}>
          <tr>
            {table.columns.map((c) => (
              <th key={c} scope="col" className={TABLE.th}>
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className={TABLE.body}>
          {table.rows.map((row) => (
            <tr key={row.join("|")}>
              {row.map((cell, i) => (
                <td key={i} className={TABLE.td}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PauseCard({ agentId }: { agentId: string }) {
  const { ws, send, commands } = useRuntime();
  const canPause = useCan("stop.pause");
  const [sent, setSent] = useState<string | null>(null);
  const [declined, setDeclined] = useState(false);
  const agent = findAgent(ws, agentId);
  const label = agent?.label ?? "The agent";
  const command: Command | undefined = sent ? commands.find((c) => c.id === sent) : undefined;

  if (declined) return <p className="text-sm text-muted-foreground">Not paused. Nothing was sent.</p>;
  return (
    <section aria-label={`Pause ${label}`} data-slot="pause-card" className="grid max-w-md gap-3 rounded-2xl border border-border bg-card p-4">
      <p className="w-fit rounded-md bg-background px-2 py-0.5 text-label">Lowers risk</p>
      <div className="grid gap-1">
        <p className="font-semibold">Pause {label}</p>
        <p className="text-sm text-pretty text-muted-foreground">No new orders. Resting protection stays in place. You can resume later. No passkey needed.</p>
      </div>
      {!canPause ? (
        <p className="text-sm">Your role can&apos;t pause agents. An owner, operator or approver can.</p>
      ) : command ? (
        <div role="status" className="grid gap-2">
          <CommandEntry command={command} label={label} />
          {command.phase === "recorded" ? (
            <Link href={agentHref(agentId, "activity")} className={cn(CITE, "w-fit")}>
              See it in the record
              <ArrowRight aria-hidden className="-my-1 size-6" />
            </Link>
          ) : null}
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <button type="button" className={PAUSE_KEY} onClick={() => setSent(send("pause", agentId).id)}>
            <Pause aria-hidden className="size-6" />
            Pause
          </button>
          <button type="button" className={QUIET_KEY} onClick={() => setDeclined(true)}>
            Not now
          </button>
        </div>
      )}
    </section>
  );
}

function StopCard({ agentId, resume }: { agentId: string | null; resume: boolean }) {
  const { ws } = useRuntime();
  const label = agentId ? (findAgent(ws, agentId)?.label ?? "the agent") : null;
  return (
    <section aria-label="Stop" data-slot="stop-card" className="grid max-w-md gap-3 rounded-2xl border border-border bg-card p-4">
      <p className="text-sm text-pretty">
        {resume
          ? `Resuming ${label ?? "an agent"} lets it trade again, so it needs your passkey. It's in Stop.`
          : `Stopping ${label ?? "an agent"}, a kill switch, or closing positions needs your passkey and the full list of what it touches. It's in Stop, where pausing comes first.`}
      </p>
      <button type="button" onClick={openStop} className={cn(QUIET_KEY, "w-fit gap-1.5 border-2 border-ink pl-3 font-semibold text-ink")}>
        <StopOctagon aria-hidden className="size-6" />
        Open Stop
      </button>
    </section>
  );
}

function CreateCard({ text, onCreate }: { text: string | null; onCreate: (text: string | null) => void }) {
  return (
    <section aria-label="Set up an agent" data-slot="create-card" className="grid max-w-md gap-3 rounded-2xl border border-border bg-card p-4">
      <p className="text-sm text-pretty">
        A new agent is set up in a conversation that ends with one summary of every value, which you confirm with your passkey.
        {text ? " Your words go with you, ready to send." : " Setup starts by asking what it should do."}
      </p>
      <button type="button" className={cn(KEY, "w-fit")} onClick={() => onCreate(text)}>
        Continue in setup
        <ArrowRight aria-hidden className="size-6" />
      </button>
    </section>
  );
}

/** The version is proposed once, when the reply opens, so what the owner reviews never shifts under them. */
function ChangeCard({ agentId, edits, quote }: { agentId: string; edits: Edits; quote: string }) {
  const { ws } = useRuntime();
  const [proposal] = useState(() => {
    const agent = findAgent(ws, agentId);
    return agent ? propose(ws, agent, edits) : null;
  });
  const [kept, setKept] = useState(false);
  if (!proposal) return null;
  if (kept) return <p className="text-sm text-muted-foreground">Kept as is. Nothing was sent.</p>;
  return <ChangeReview proposal={proposal} origin={{ kind: "message", quote }} onKeep={() => setKept(true)} fixHint="Tell me a different value." />;
}

/** One reply: the record's answer with the records it read, or the one card the owner asked for. */
export function RecordReply({ reply, onAsk, onCreate }: { reply: Reply; onAsk: (text: string) => void; onCreate: (text: string | null) => void }) {
  switch (reply.kind) {
    case "answer":
      return (
        <div data-slot="record-answer" className="grid max-w-3xl min-w-0 gap-2">
          <span className="sr-only">Owlhead: </span>
          {reply.lines.map((line, i) => (
            <p key={i} className="max-w-measure text-pretty">
              {line}
            </p>
          ))}
          {reply.table ? <FiguresTable table={reply.table} /> : null}
          <Cites cites={reply.cites} />
          <AskChips asks={reply.follow} onAsk={onAsk} />
        </div>
      );
    case "pause":
      return <PauseCard agentId={reply.agentId} />;
    case "stop":
      return <StopCard agentId={reply.agentId} resume={reply.resume} />;
    case "create":
      return <CreateCard text={reply.text} onCreate={onCreate} />;
    case "change":
      return <ChangeCard agentId={reply.agentId} edits={reply.edits} quote={reply.quote} />;
    default: {
      const unhandled: never = reply;
      throw new Error(`unhandled reply ${JSON.stringify(unhandled)}`);
    }
  }
}
