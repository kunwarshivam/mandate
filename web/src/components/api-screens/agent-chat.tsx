"use client";

import { useRef, useState } from "react";
import Link from "next/link";
import type { ChatCard } from "@/api/chat";
import type { AgentDetail } from "@/api/read-models";
import { Composer } from "@/components/chat/composer";
import { KEY } from "@/components/kumo/key";
import { Section } from "@/components/screens/common";
import { recordedLine, stepUpLine } from "@/components/stop/commands";
import { quantity } from "@/lib/format";
import { type AskOutcome, type CommandRecord, type ExitOutcome, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

/**
 * Telling an agent what to do in plain words (the founder: "type what I want in simple words, and
 * that does things"). The words go to the deployment's chat route, which asks the model and answers
 * with its reply and at most one typed card. The reply is shown as the assistant's quoted text. The
 * card is shown as one line this page writes from its typed values, and nothing is sent until the
 * owner taps Send. Each action then goes through the path the rest of the app uses: an ask to buy
 * through the agent's builder, gate and autonomy rules (DEC-2151), the owner commands with their own
 * step-up and record, and owner exits (rule 13).
 */

type Command = "pause" | "resume" | "stop" | "kill";

type Proposal = { card: ChatCard; preview: string; state: "proposed" | "canceled" | "sending" | "done"; outcome?: string; commandId?: string; approvals?: boolean };

interface Entry {
  id: number;
  said: string;
  /** `null` while the deployment is answering. */
  reply: string | null;
  failed: boolean;
  proposal: Proposal | null;
}

const NOT_REPORTED = "text-muted-foreground";
const COMMAND_OF: Readonly<Partial<Record<ChatCard["action"], Command>>> = { pause: "pause", resume: "resume", stop: "stop", kill_switch: "kill" };

function askWords(outcome: AskOutcome, symbol: string): { line: string; approvals: boolean } {
  switch (outcome.phase) {
    case "taken":
      return { line: "Your agent took the request. If its mandate asks you first, it is waiting in Approvals.", approvals: true };
    case "recorded":
      return { line: "Your request is recorded. Your agent has not answered yet; check Approvals before asking again.", approvals: true };
    case "refused":
      return { line: "Your agent did not take the request. Its record is in the agent's activity.", approvals: false };
    case "unknown":
      return { line: "Whether your request was recorded is not known yet. Check Approvals before asking again.", approvals: true };
    case "undelivered":
      return {
        line:
          outcome.reason === "instrument_not_pinned"
            ? `Can't: this agent's mandate doesn't include ${symbol}.`
            : outcome.reason === "invalid_size"
              ? "Can't: the quantity must be above zero."
              : "Not sent. Your deployment did not take the request; nothing was recorded.",
        approvals: false,
      };
  }
}

function exitWords(outcome: ExitOutcome): string {
  switch (outcome.phase) {
    case "applied":
    case "ended":
      return "sell sent to the account";
    case "recorded":
      return "recorded; the agent sells it";
    case "refused":
      return "refused; the record says why";
    case "undelivered":
      return "not sent";
    case "unknown":
      return "not known yet";
  }
}

/** The one line the owner confirms, written here from the card's typed values, never from the model's text. */
export function previewOf(card: ChatCard, name: string): string | null {
  switch (card.action) {
    case "ask_to_buy":
      return `Ask ${name} to buy ${quantity(card.size)} ${card.symbol}. It sizes the order within its mandate, and its rules decide whether it asks you first.`;
    case "owner_exit":
      return `Sell all of ${name}'s ${card.instruments.map((i) => `${quantity(i.qty)} ${i.symbol}`).join(", ")}, as your own exit. Equities sell in the regular session.`;
    case "pause":
    case "resume":
    case "stop":
      return stepUpLine(card.action, name, 0);
    case "kill_switch":
      return stepUpLine("kill", name, 0);
    case "create_agent":
      return null;
  }
}

export function AgentChat({ agent }: { agent: AgentDetail }) {
  const { chat, askAgent, send, ownerExit, commands, ws } = useRuntime();
  const canAct = useCan("stop.pause");
  const [entries, setEntries] = useState<Entry[]>([]);
  const next = useRef(0);
  const name = agent.label ?? "your agent";

  const update = (id: number, change: Partial<Entry>) => setEntries((list) => list.map((e) => (e.id === id ? { ...e, ...change } : e)));
  const patch = (id: number, change: Partial<Proposal>) => setEntries((list) => list.map((e) => (e.id === id && e.proposal ? { ...e, proposal: { ...e.proposal, ...change } } : e)));

  const say = (said: string) => {
    if (!chat) return;
    next.current += 1;
    const id = next.current;
    setEntries((list) => [...list, { id, said, reply: null, failed: false, proposal: null }]);
    void chat(agent.agent_id, said).then((answer) => {
      if (!answer.ok) return update(id, { failed: true, reply: answer.reason === "unknown" || answer.reason === "network" ? "Your deployment's answer was lost. Nothing was done." : "Your deployment didn't answer that. Nothing was done." });
      const { card, reply } = answer.value;
      const preview = card ? previewOf(card, name) : null;
      update(id, { reply, proposal: card && preview ? { card, preview, state: "proposed" } : null });
    });
  };

  const record = (preview: string, title: string): CommandRecord => ({ screen: "D10", environment: ws.environment, title, shown: [preview], modes: [{ agent: agent.agent_id, badge: agent.mode ?? "not reported" }] });

  const confirm = (entry: Entry) => {
    const proposal = entry.proposal;
    if (!proposal || proposal.state !== "proposed") return;
    const { card, preview } = proposal;
    patch(entry.id, { state: "sending" });
    if (card.action === "ask_to_buy") {
      if (!askAgent) return patch(entry.id, { state: "done", outcome: "Asking an agent is not available here." });
      void askAgent(agent.agent_id, card.instrument_id, card.size, () => {}).then((outcome) => {
        const words = askWords(outcome, card.symbol);
        patch(entry.id, { state: "done", outcome: words.line, approvals: words.approvals });
      });
      return;
    }
    if (card.action === "owner_exit") {
      if (!ownerExit) return patch(entry.id, { state: "done", outcome: "Exits are not available here." });
      void (async () => {
        const said: string[] = [];
        for (const one of card.instruments) {
          const outcome = await ownerExit(agent.agent_id, one.instrument_id, record(preview, `Exit ${one.symbol}`), () => {});
          said.push(`${one.symbol}: ${exitWords(outcome)}`);
        }
        patch(entry.id, { state: "done", outcome: `${said.join("; ")}.` });
      })();
      return;
    }
    const command = COMMAND_OF[card.action];
    if (!command) return patch(entry.id, { state: "done", outcome: "That is not available here." });
    const sent = send(command, agent.agent_id, record(preview, preview));
    patch(entry.id, { state: "done", commandId: sent.id });
  };

  const commandLine = (commandId: string, command: Command): string => {
    const c = commands.find((x) => x.id === commandId);
    if (!c) return "Sent.";
    if (c.awaitingPasskey) return "Waiting for your passkey.";
    if (c.phase === "undelivered") return "Not sent. Nothing was recorded.";
    if (c.phase === "unknown") return "The result is unknown; checking.";
    if (c.settled === "refused") return "Your deployment refused it. The record says why.";
    if (c.phase === "recorded" && (!c.awaitsRuntime || c.settled === "applied" || c.settled === "ended")) return recordedLine(command, name);
    return c.phase === "recorded" ? "Recorded. Waiting for the agent to act on it." : "Sending.";
  };

  if (!chat) return null;
  return (
    <Section title={`Tell ${name}`}>
      <p className="max-w-measure text-sm text-muted-foreground">
        Say what you want in plain words. Anything it would do is shown first and sent only when you tap Send, within this agent&rsquo;s mandate.
      </p>
      {entries.length > 0 ? (
        <ol className="grid gap-4" aria-label="Your messages" data-slot="agent-chat-log">
          {entries.map((entry) => {
            const p = entry.proposal;
            const command = p ? COMMAND_OF[p.card.action] : undefined;
            return (
              <li key={entry.id} className="grid gap-2" data-slot="agent-chat-entry">
                <p className="justify-self-end rounded-lg bg-muted px-3 py-2 text-sm whitespace-pre-wrap">{entry.said}</p>
                {entry.reply === null ? (
                  <p className={`text-sm ${NOT_REPORTED}`}>Thinking.</p>
                ) : (
                  <blockquote className={`text-sm whitespace-pre-wrap ${entry.failed ? NOT_REPORTED : ""}`} data-slot="agent-chat-reply">
                    {entry.reply}
                  </blockquote>
                )}
                {p ? (
                  <div className="grid gap-2 rounded-lg border border-border p-3" data-slot="agent-chat-proposal" data-state={p.state}>
                    <p className="text-sm font-medium">{p.preview}</p>
                    {p.state === "proposed" ? (
                      <div className="flex gap-2">
                        <button type="button" className={KEY} onClick={() => confirm(entry)} disabled={!canAct}>
                          Send
                        </button>
                        <button type="button" className={KEY} onClick={() => patch(entry.id, { state: "canceled" })}>
                          Cancel
                        </button>
                      </div>
                    ) : null}
                    <div role="status" aria-live="polite" className="text-sm">
                      {p.state === "canceled" ? <p className={NOT_REPORTED}>Canceled. Nothing was sent.</p> : null}
                      {p.state === "sending" ? <p>Sending.</p> : null}
                      {p.state === "done" && command && p.commandId ? <p>{commandLine(p.commandId, command)}</p> : null}
                      {p.state === "done" && p.outcome ? (
                        <p data-slot="agent-chat-outcome">
                          {p.outcome}{" "}
                          {p.approvals ? (
                            <Link href="/approvals" className="underline underline-offset-4">
                              Approvals
                            </Link>
                          ) : null}
                        </p>
                      ) : null}
                    </div>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ol>
      ) : null}
      <Composer label={`Tell ${name} what to do`} placeholder="buy 10 AAPL, pause, how am I doing" onSend={say} disabled={!canAct} />
    </Section>
  );
}
