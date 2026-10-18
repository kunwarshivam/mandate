"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { claimedBy, independentApprovalRequired, unallocatedUsd } from "@/lib/fixture-journey";
import { useHandoff } from "@/lib/handoff";
import { type SetUp, useRuntime } from "@/lib/mock-runtime";
import { Chat, type Creation } from "./chat";
import { type Compiler, type CompilerInput, figuresOnly, fixtureCompiler, validateTurn } from "./compiler";
import { type Checks, type Conversation, applyTurn, blocker, chooseModel, draftOf, failTurn, inputFor, readWithoutModel, say, startConversation } from "./conversation";
import { mandateFrom } from "./draft";
import { STEP_HEADING } from "./steps";

const FIXTURE_COMPILER = fixtureCompiler();

/**
 * Setting up an agent on the fixture workspace (brief A0 to A2, A5 and B3, DEC-476, DEC-477,
 * DEC-2401): one conversation gathers the owner's values, shows the whole agent once, and then three
 * steps set it up, each its own record: create the version, confirm it with a passkey, deploy it to
 * paper with a passkey. Everything stays in this component's state until then; only the steps reach
 * the runtime, which repeats V-047, V-002 and V-006 when it validates the version and when it applies
 * the deployment. Under the workspace's
 * independent-approval policy the summary offers no step at all (V-047, interim until a
 * second user can approve). Words handed over from Messages or the
 * copilot wait in the message field; nothing is read until the owner sends them.
 */
export function NewAgentFlow({ compiler = FIXTURE_COMPILER }: { compiler?: Compiler }) {
  const { ws, setUps } = useRuntime();
  const [conversation, setConversation] = useState<Conversation>(startConversation);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  /** Each summary revision's latest set-up, by its id. */
  const [sent, setSent] = useState<Readonly<Record<number, string>>>({});

  const checks = useMemo<Checks>(
    () => ({ room: unallocatedUsd(ws), claimedBy: (symbol) => claimedBy(ws, symbol)?.label ?? null, independentApproval: independentApprovalRequired(ws) }),
    [ws],
  );
  const checksRef = useRef(checks);
  useEffect(() => {
    checksRef.current = checks;
  }, [checks]);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  const root = useRef<HTMLDivElement>(null);
  const firstAttempt = useRef(true);
  useEffect(() => {
    if (firstAttempt.current) {
      firstAttempt.current = false;
      return;
    }
    root.current?.scrollIntoView({ block: "start" });
    root.current?.querySelector<HTMLElement>(`#${STEP_HEADING}`)?.focus({ preventScroll: true });
  }, [attempt]);

  const read = async (input: CompilerInput, messageId: string, withModel: boolean) => {
    setBusy(true);
    let next: (c: Conversation) => Conversation;
    try {
      const answer = await (withModel ? compiler : figuresOnly)(input);
      const checked = validateTurn(answer, input);
      next = checked.ok ? (c) => applyTurn(c, messageId, checked.turn, checksRef.current) : (c) => failTurn(c, messageId, "invalid");
    } catch {
      next = (c) => failTurn(c, messageId, "unreachable");
    }
    if (!live.current) return;
    setConversation(next);
    setBusy(false);
  };

  const draft = useMemo(() => draftOf(conversation), [conversation]);
  const request = useMemo(() => (draft ? mandateFrom(draft, ws.connection.connection_id) : null), [draft, ws.connection.connection_id]);
  const shown = useMemo(() => {
    const out: Record<number, SetUp> = {};
    for (const [revision, id] of Object.entries(sent)) {
      const setUp = setUps.find((s) => s.id === id);
      if (setUp) out[Number(revision)] = setUp;
    }
    return out;
  }, [sent, setUps]);
  const creation: Creation = {
    draft,
    request,
    blocked: blocker(conversation, checks),
    setUps: shown,
  };

  const send = (text: string) => {
    const [next, input] = say(conversation, text);
    setConversation(next);
    void read(input, input.messages[input.messages.length - 1].id, !next.figuresOnly);
  };

  const handoff = useHandoff();
  const [handed] = useState(() => handoff.peek());
  useEffect(() => handoff.clear(), [handoff]);

  return (
    <div ref={root} className="mx-auto w-full max-w-2xl reveal" key={`chat-${attempt}`}>
      <Chat
        conversation={conversation}
        busy={busy}
        creation={creation}
        onSend={send}
        initialText={attempt === 0 ? handed : null}
        actions={{
          onChoose: (model) => setConversation((c) => chooseModel(c, model, checksRef.current)),
          onRetry: (messageId) => void read(inputFor(conversation, messageId), messageId, !conversation.figuresOnly),
          onWithoutModel: (messageId) => {
            const next = readWithoutModel(conversation);
            setConversation(next);
            void read(inputFor(next, messageId), messageId, false);
          },
          onCreated: (id, revision) => setSent((map) => ({ ...map, [revision]: id })),
          onStartOver: () => {
            setConversation(startConversation());
            setSent({});
            setAttempt((n) => n + 1);
          },
        }}
      />
    </div>
  );
}
