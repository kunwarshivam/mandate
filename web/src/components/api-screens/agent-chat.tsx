"use client";

import { useRef, useState } from "react";
import Link from "next/link";
import type { AgentDetail } from "@/api/read-models";
import { Composer } from "@/components/chat/composer";
import { KEY } from "@/components/kumo/key";
import { Section } from "@/components/screens/common";
import { recordedLine, stepUpLine } from "@/components/stop/commands";
import { EXAMPLES, readCommand } from "@/lib/agent-command";
import { quantity, usd } from "@/lib/format";
import { type AskOutcome, type CommandRecord, type ExitOutcome, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

/**
 * Telling one agent what to do in plain words (the founder: "type what I want in simple words, and
 * that does things"). `readCommand` reads the words as one fixed action, with no model. The action is
 * shown as one line, and nothing is sent until the owner taps Send. Each action goes through the path
 * the rest of the app uses: an ask to buy through the agent's builder, gate and autonomy rules
 * (DEC-2151), the owner commands with their own step-up, and owner exits (rule 13). It gives no advice.
 */

type Command = "pause" | "resume" | "stop" | "kill";
type Action =
  | { kind: "ask"; instrument: string; symbol: string; size: string }
  | { kind: "command"; command: Command }
  | { kind: "exit"; instruments: Array<{ id: string; symbol: string }> };

type Reply =
  | { kind: "text"; lines: string[]; links?: Array<{ href: string; label: string }> }
  | { kind: "proposal"; preview: string; action: Action; state: "proposed" | "canceled" | "sending" | "done"; outcome?: string; commandId?: string; approvals?: boolean };

interface Entry {
  id: number;
  said: string;
  reply: Reply;
}

const NOT_REPORTED = "text-muted-foreground";
const EXAMPLE_LINE = `Try: ${EXAMPLES.map((e) => `“${e}”`).join(", ")}.`;

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
            ? `Not sent. This agent's mandate doesn't include ${symbol}, so it can't be asked for it.`
            : outcome.reason === "invalid_size"
              ? "Not sent. The quantity must be above zero."
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

export function AgentChat({ agent }: { agent: AgentDetail }) {
  const { askAgent, send, ownerExit, holdings, catalogue, commands, ws } = useRuntime();
  const canAct = useCan("stop.pause");
  const [entries, setEntries] = useState<Entry[]>([]);
  const next = useRef(0);
  const name = agent.label ?? "your agent";

  const put = (id: number, reply: Reply) => setEntries((list) => list.map((e) => (e.id === id ? { ...e, reply } : e)));
  const patch = (id: number, change: Partial<Extract<Reply, { kind: "proposal" }>>) =>
    setEntries((list) => list.map((e) => (e.id === id && e.reply.kind === "proposal" ? { ...e, reply: { ...e.reply, ...change } } : e)));

  const held = async (): Promise<Array<{ id: string; symbol: string; qty: string; value: string | null }> | null> => {
    if (!holdings || !catalogue) return null;
    const [read, listed] = await Promise.all([holdings(agent.agent_id), catalogue()]);
    if (!read.ok) return null;
    const symbols = new Map(listed.ok ? listed.instruments.map((i) => [i.instrument_id, i.symbol]) : []);
    return read.items
      .filter((h) => !h.filled_qty.startsWith("-") && !/^0(\.0*)?$/.test(h.filled_qty))
      .map((h) => ({ id: h.instrument_id, symbol: symbols.get(h.instrument_id) ?? "an instrument not in the catalogue", qty: h.filled_qty, value: h.market_value }));
  };

  const understand = async (id: number, said: string) => {
    const command = readCommand(said);
    switch (command.kind) {
      case "unknown":
        return put(id, { kind: "text", lines: ["I didn't catch that.", EXAMPLE_LINE] });
      case "advice":
        return put(id, { kind: "text", lines: ["I can't advise on what to trade. I can do what you ask, within this agent's mandate.", EXAMPLE_LINE] });
      case "status": {
        const lines = await held();
        if (lines === null) return put(id, { kind: "text", lines: ["Its positions are not reported right now."], links: [{ href: "/positions", label: "Positions" }] });
        return put(id, {
          kind: "text",
          lines: [
            lines.length === 0 ? `${name} holds nothing.` : `${name} holds:`,
            ...lines.map((h) => `${h.symbol}: ${quantity(h.qty)}, market value ${h.value ? usd(h.value) : "not reported"}.`),
            "P&L is not reported by your deployment yet.",
          ],
          links: [
            { href: "/positions", label: "Positions" },
            { href: "/audit/timeline", label: "The record" },
          ],
        });
      }
      case "ask_buy": {
        const listed = catalogue ? await catalogue() : null;
        if (!listed?.ok) return put(id, { kind: "text", lines: ["Your deployment's instruments are not reported right now, so I can't look up that symbol."] });
        const found = listed.instruments.find((i) => i.symbol.toUpperCase() === command.symbol);
        if (!found) return put(id, { kind: "text", lines: [`${command.symbol} isn't registered on your deployment.`], links: [{ href: "/instruments", label: "Search instruments" }] });
        return put(id, {
          kind: "proposal",
          state: "proposed",
          preview: `Ask ${name} to buy ${quantity(command.size)} ${found.symbol}. It sizes the order within its mandate, and its rules decide whether it asks you first.`,
          action: { kind: "ask", instrument: found.instrument_id, symbol: found.symbol, size: command.size },
        });
      }
      case "exit": {
        const lines = await held();
        if (lines === null) return put(id, { kind: "text", lines: ["Its positions are not reported right now, so there is nothing I can sell yet."] });
        const chosen = command.symbol ? lines.filter((h) => h.symbol.toUpperCase() === command.symbol) : lines;
        if (chosen.length === 0) return put(id, { kind: "text", lines: [command.symbol ? `${name} holds no ${command.symbol}.` : `${name} holds nothing to sell.`] });
        return put(id, {
          kind: "proposal",
          state: "proposed",
          preview: `Sell all of ${name}'s ${chosen.map((h) => `${quantity(h.qty)} ${h.symbol}`).join(", ")}, as your own exit. Equities sell in the regular session.`,
          action: { kind: "exit", instruments: chosen.map((h) => ({ id: h.id, symbol: h.symbol })) },
        });
      }
      case "stop": {
        const lines = await held();
        if (lines !== null && lines.length > 0)
          return put(id, { kind: "text", lines: [`${name} still holds positions. Say “sell everything” first, or “kill switch” to sell them and stop it at once.`] });
        return put(id, { kind: "proposal", state: "proposed", preview: stepUpLine("stop", name, 0), action: { kind: "command", command: "stop" } });
      }
      case "pause":
      case "resume":
      case "kill":
        return put(id, { kind: "proposal", state: "proposed", preview: stepUpLine(command.kind, name, 0), action: { kind: "command", command: command.kind } });
    }
  };

  const say = (said: string) => {
    next.current += 1;
    const id = next.current;
    setEntries((list) => [...list, { id, said, reply: { kind: "text", lines: ["Reading that."] } }]);
    void understand(id, said);
  };

  const record = (preview: string, title: string): CommandRecord => ({ screen: "D10", environment: ws.environment, title, shown: [preview], modes: [{ agent: agent.agent_id, badge: agent.mode ?? "not reported" }] });

  const confirm = (entry: Entry) => {
    const reply = entry.reply;
    if (reply.kind !== "proposal" || reply.state !== "proposed") return;
    const { action, preview } = reply;
    patch(entry.id, { state: "sending" });
    if (action.kind === "ask") {
      if (!askAgent) return patch(entry.id, { state: "done", outcome: "Asking an agent is not available here." });
      void askAgent(agent.agent_id, action.instrument, action.size, () => {}).then((outcome) => {
        const words = askWords(outcome, action.symbol);
        patch(entry.id, { state: "done", outcome: words.line, approvals: words.approvals });
      });
      return;
    }
    if (action.kind === "exit") {
      if (!ownerExit) return patch(entry.id, { state: "done", outcome: "Exits are not available here." });
      void (async () => {
        const said: string[] = [];
        for (const one of action.instruments) {
          const outcome = await ownerExit(agent.agent_id, one.id, record(preview, `Exit ${one.symbol}`), () => {});
          said.push(`${one.symbol}: ${exitWords(outcome)}`);
        }
        patch(entry.id, { state: "done", outcome: `${said.join("; ")}.` });
      })();
      return;
    }
    const sent = send(action.command, agent.agent_id, record(preview, preview));
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

  return (
    <Section title={`Tell ${name}`}>
      <p className="max-w-measure text-sm text-muted-foreground">
        Say what you want in plain words. Each request is shown first and sent only when you tap Send. It acts within this agent&rsquo;s mandate, and gives no trade advice.
      </p>
      {entries.length > 0 ? (
        <ol className="grid gap-4" aria-label="Your requests" data-slot="agent-chat-log">
          {entries.map((entry) => (
            <li key={entry.id} className="grid gap-2" data-slot="agent-chat-entry">
              <p className="justify-self-end rounded-lg bg-muted px-3 py-2 text-sm">{entry.said}</p>
              {entry.reply.kind === "text" ? (
                <div className="grid gap-1 text-sm" data-slot="agent-chat-reply">
                  {entry.reply.lines.map((line, i) => (
                    <p key={i}>{line}</p>
                  ))}
                  {entry.reply.links ? (
                    <p className="flex flex-wrap gap-3">
                      {entry.reply.links.map((l) => (
                        <Link key={l.href} href={l.href} className="underline underline-offset-4">
                          {l.label}
                        </Link>
                      ))}
                    </p>
                  ) : null}
                </div>
              ) : (
                <div className="grid gap-2 rounded-lg border border-border p-3" data-slot="agent-chat-proposal" data-state={entry.reply.state}>
                  <p className="text-sm font-medium">{entry.reply.preview}</p>
                  {entry.reply.state === "proposed" ? (
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
                    {entry.reply.state === "canceled" ? <p className={NOT_REPORTED}>Canceled. Nothing was sent.</p> : null}
                    {entry.reply.state === "sending" ? <p>Sending.</p> : null}
                    {entry.reply.state === "done" && entry.reply.action.kind === "command" && entry.reply.commandId ? <p>{commandLine(entry.reply.commandId, entry.reply.action.command)}</p> : null}
                    {entry.reply.state === "done" && entry.reply.outcome ? (
                      <p data-slot="agent-chat-outcome">
                        {entry.reply.outcome}{" "}
                        {entry.reply.approvals ? (
                          <Link href="/approvals" className="underline underline-offset-4">
                            Approvals
                          </Link>
                        ) : null}
                      </p>
                    ) : null}
                  </div>
                </div>
              )}
            </li>
          ))}
        </ol>
      ) : null}
      <Composer label={`Tell ${name} what to do`} placeholder="buy 10 AAPL, pause, how am I doing" onSend={say} disabled={!canAct} />
    </Section>
  );
}
