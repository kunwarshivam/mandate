"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { claimedBy, independentApprovalRequired, unallocatedUsd } from "@/lib/fixture-journey";
import { useHandoff } from "@/lib/handoff";
import { type SetUp, useRuntime } from "@/lib/mock-runtime";
import { Chat, type Creation } from "./chat";
import { type Compiler, type CompilerInput, figuresOnly, fixtureCompiler, validateTurn } from "./compiler";
import { type Checks, type Conversation, applyTurn, blocker, chooseModel, draftOf, failTurn, inputFor, readWithoutModel, say, startConversation } from "./conversation";
import { MODELS, type ModelChoice, mandateFrom, servedModelChoice } from "./draft";
import type { InstrumentRef } from "@/fixtures/types";
import { STEP_HEADING } from "./steps";

const FIXTURE_COMPILER = fixtureCompiler();

/** Over the API: the account's room, who trades a symbol, and the independent-approval policy are not served, so the deployment judges V-002, V-006 and V-047 itself. */
const SERVER_JUDGES: Checks = { room: null, claimedBy: () => null, independentApproval: false };

/** Over the API with no established connection: a version names its connection, so none can be created yet. */
export const NO_CONNECTION = "Connect your Alpaca paper account first, in Connections. A version names the account it trades on.";

/** The models and instruments a conversation offers: the fixture catalogue, or what the deployment registered. */
interface Catalogue {
  models: readonly ModelChoice[];
  instruments: Readonly<Record<string, InstrumentRef>> | null;
}

/**
 * Over the API, the catalogue the deployment registered (`GET /instruments`, `GET /signal-models`,
 * DEC-2136), read once when the set-up opens: the version names only registered asset ids and model
 * hashes, so nothing is made up in the browser. A model this page cannot ask about is not offered.
 * Until it is read the set-up waits, and a catalogue that cannot be read says so and offers nothing.
 */
export function NewAgentFlow({ compiler }: { compiler?: Compiler }) {
  const { source, catalogue } = useRuntime();
  const [served, setServed] = useState<{ ok: true; catalogue: Catalogue } | { ok: false } | null>(null);
  useEffect(() => {
    if (source !== "api" || !catalogue) return;
    let live = true;
    void catalogue().then((answer) => {
      if (!live) return;
      if (!answer.ok) return setServed({ ok: false });
      const models = answer.models.map(servedModelChoice).filter((m): m is ModelChoice => m !== null);
      const instruments = Object.fromEntries(answer.instruments.map((i) => [i.symbol, { asset_id: i.instrument_id, symbol: i.symbol, asset_class: i.asset_class } satisfies InstrumentRef]));
      setServed({ ok: true, catalogue: { models, instruments } });
    });
    return () => {
      live = false;
    };
  }, [source, catalogue]);
  if (source !== "api") return <Flow compiler={compiler} catalogue={{ models: MODELS, instruments: null }} />;
  if (served === null)
    return (
      <p className="mx-auto w-full max-w-2xl text-muted-foreground" data-slot="catalogue-reading">
        Reading the instruments and models your deployment has registered.
      </p>
    );
  if (!served.ok)
    return (
      <p role="alert" className="mx-auto w-full max-w-2xl text-pretty" data-slot="catalogue-unreported">
        Your deployment did not report the instruments and models it has registered, so no agent can be set up from here now. This page tries again when you reload it.
      </p>
    );
  return <Flow compiler={compiler} catalogue={served.catalogue} />;
}

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
function Flow({ compiler: given, catalogue }: { compiler?: Compiler; catalogue: Catalogue }) {
  const { ws, setUps, source } = useRuntime();
  /**
   * Over the API no compiler is served (spec §4.1: drafts and compile are planned), so the owner's
   * words are read by the deterministic figures-only reader, never by a stand-in model.
   */
  const overApi = source === "api";
  const compiler = given ?? (overApi ? figuresOnly : FIXTURE_COMPILER);
  const start = () => startConversation({ figuresOnly: overApi, models: catalogue.models, instruments: catalogue.instruments });
  const [conversation, setConversation] = useState<Conversation>(start);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  /** Each summary revision's latest set-up, by its id. */
  const [sent, setSent] = useState<Readonly<Record<number, string>>>({});

  const checks = useMemo<Checks>(
    () => (overApi ? SERVER_JUDGES : { room: unallocatedUsd(ws), claimedBy: (symbol) => claimedBy(ws, symbol)?.label ?? null, independentApproval: independentApprovalRequired(ws) }),
    [ws, overApi],
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
  const request = useMemo(() => (draft ? mandateFrom(draft, ws.connection.connection_id, conversation.instruments) : null), [draft, ws.connection.connection_id, conversation.instruments]);
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
    blocked: blocker(conversation, checks) ?? (overApi && !ws.connection.connection_id ? NO_CONNECTION : null),
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
            setConversation(start());
            setSent({});
            setAttempt((n) => n + 1);
          },
        }}
      />
    </div>
  );
}
