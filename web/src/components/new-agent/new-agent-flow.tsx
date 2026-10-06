"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { claimedBy, unallocatedUsd } from "@/lib/fixture-journey";
import { useRuntime } from "@/lib/mock-runtime";
import { Chat } from "./chat";
import { type Compiler, type CompilerInput, figuresOnly, fixtureCompiler, validateTurn } from "./compiler";
import { Confirmation } from "./confirmation";
import {
  type Checks,
  type Conversation,
  allConfirmed,
  answerCheck,
  applyTurn,
  chooseModel,
  confirmSection,
  draftOf,
  failTurn,
  inputFor,
  readWithoutModel,
  say,
  startConversation,
} from "./conversation";
import { mandateFrom } from "./draft";
import { STEP_HEADING } from "./steps";

const FIXTURE_COMPILER = fixtureCompiler();

/**
 * Setting up an agent (brief A0 to A2, then A5, DEC-473) on the fixture workspace: one conversation
 * drafts the mandate and confirms it section by section, then the record screen confirms it with a
 * passkey. Everything stays in this component's state until then; only the deployment reaches the
 * runtime, which repeats V-002 and V-006 when it applies it.
 */
export function NewAgentFlow({ compiler = FIXTURE_COMPILER }: { compiler?: Compiler }) {
  const { ws } = useRuntime();
  const [conversation, setConversation] = useState<Conversation>(startConversation);
  const [step, setStep] = useState<"chat" | "confirm">("chat");
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);

  const checks = useMemo<Checks>(() => ({ room: unallocatedUsd(ws), claimedBy: (symbol) => claimedBy(ws, symbol)?.label ?? null }), [ws]);
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
  const firstStep = useRef(true);
  useEffect(() => {
    if (firstStep.current) {
      firstStep.current = false;
      return;
    }
    root.current?.querySelector<HTMLElement>(`#${STEP_HEADING}`)?.focus();
  }, [step, attempt]);

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
  const request = useMemo(() => (draft && step === "confirm" ? mandateFrom(draft, ws.connection.connection_id) : null), [draft, step, ws.connection.connection_id]);

  if (step === "confirm" && draft && request) {
    return (
      <div ref={root} className="mx-auto w-full max-w-3xl reveal" key={`confirm-${attempt}`}>
        <Confirmation
          draft={draft}
          request={request}
          onBack={() => setStep("chat")}
          onStartOver={() => {
            setConversation(startConversation());
            setAttempt((n) => n + 1);
            setStep("chat");
          }}
        />
      </div>
    );
  }

  return (
    <div ref={root} className="mx-auto w-full max-w-2xl reveal" key={`chat-${attempt}`}>
      <Chat
        conversation={conversation}
        busy={busy}
        onSend={(text) => {
          const [next, input] = say(conversation, text);
          setConversation(next);
          void read(input, input.messages[input.messages.length - 1].id, !next.figuresOnly);
        }}
        actions={{
          onChoose: (model) => setConversation((c) => chooseModel(c, model)),
          onCheck: (yes) => setConversation((c) => answerCheck(c, yes, checks)),
          onConfirm: (key) => setConversation((c) => confirmSection(c, key, checks)),
          onReady: () => {
            if (allConfirmed(conversation)) setStep("confirm");
          },
          onRetry: (messageId) => void read(inputFor(conversation, messageId), messageId, !conversation.figuresOnly),
          onWithoutModel: (messageId) => {
            const next = readWithoutModel(conversation);
            setConversation(next);
            void read(inputFor(next, messageId), messageId, false);
          },
        }}
      />
    </div>
  );
}
