"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatMessage, ChatTarget } from "@/api/chat";
import { useRuntime } from "@/lib/mock-runtime";

/** How often a thread is read while its agent is replying, and otherwise (§4.6.2: no socket). */
export const POLL_RUNNING_MS = 1500;
export const POLL_IDLE_MS = 15000;

export interface ThreadState {
  messages: ChatMessage[];
  /** Whether a run is in flight, and which. */
  running: boolean;
  runId: string | null;
  /** The setup this thread became on its first message, for a new agent's. */
  setupId: string | null;
  /** Why the last read or send failed, or `null`. */
  problem: string | null;
  /** Whether the thread has been read once. */
  loaded: boolean;
  send: (text: string) => void;
  stop: () => void;
  /** Reads the thread now. */
  refresh: () => void;
}

/** Words for a send or read that failed, by its code. */
export function problemWords(reason: string): string {
  switch (reason) {
    case "rate_limited":
      return "Your agent is still answering your last message. Wait for it, or press Stop.";
    case "network":
      return "Your message may not have reached your deployment. Check the thread before sending it again.";
    case "forbidden":
      return "Your role can't chat with this agent.";
    case "not_found":
      return "This agent isn't in your workspace.";
    case "invalid":
      return "That message is too long, or empty.";
    default:
      return "Your deployment didn't answer. Try again in a moment.";
  }
}

/**
 * One thread, read from the journal through the chat routes: every message after the cursor is
 * appended, nothing is shown that was not read back, and a sent message appears once the server has
 * journaled it (DEC-2293 item 7).
 */
export function useThread(target: ChatTarget): ThreadState {
  const { chat } = useRuntime();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [running, setRunning] = useState(false);
  const [runId, setRunId] = useState<string | null>(null);
  const [setupId, setSetupId] = useState<string | null>("setup" in target ? target.setup : null);
  const [problem, setProblem] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const cursor = useRef(0);
  const live = useRef(true);
  const agent = "agent" in target ? target.agent : null;
  const current = useCallback((): ChatTarget => (agent !== null ? { agent } : { setup: setupId }), [agent, setupId]);

  const read = useCallback(async () => {
    const answer = await chat.thread(current(), cursor.current);
    if (!live.current) return;
    setLoaded(true);
    if (!answer.ok) return setProblem(problemWords(answer.reason));
    setProblem(null);
    cursor.current = answer.value.cursor;
    if (answer.value.messages.length > 0)
      setMessages((known) => {
        const seen = new Set(known.map((m) => m.id));
        return [...known, ...answer.value.messages.filter((m) => !seen.has(m.id))];
      });
    setRunning(answer.value.running);
    setRunId(answer.value.run_id);
  }, [chat, current]);

  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  useEffect(() => {
    void read();
    const every = running ? POLL_RUNNING_MS : POLL_IDLE_MS;
    const timer = window.setInterval(() => void read(), every);
    return () => window.clearInterval(timer);
  }, [read, running]);

  const send = useCallback(
    (text: string) => {
      setProblem(null);
      void (async () => {
        const answer = await chat.send(current(), text);
        if (!live.current) return;
        if (!answer.ok) return setProblem(problemWords(answer.reason));
        if (answer.value.setup_id) setSetupId(answer.value.setup_id);
        setRunning(true);
        setRunId(answer.value.run_id);
      })();
    },
    [chat, current],
  );

  useEffect(() => {
    if (setupId !== null) void read();
  }, [setupId, read]);

  const stop = useCallback(() => {
    if (agent === null || runId === null) return;
    void chat.stop(agent, runId).then(() => live.current && void read());
  }, [chat, agent, runId, read]);

  return { messages, running, runId, setupId, problem, loaded, send, stop, refresh: () => void read() };
}
