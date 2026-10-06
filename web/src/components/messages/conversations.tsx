"use client";

import { type ReactNode, createContext, useCallback, useContext, useMemo, useState } from "react";
import type { Iso } from "@/fixtures/types";
import type { Reply } from "@/lib/ask-record";

/** One question the owner asked and the reply the record gave, at the time it was asked. */
export interface Turn {
  id: string;
  at: Iso;
  said: string;
  reply: Reply;
}

interface Store {
  turns: Record<string, Turn[]>;
  add: (threadId: string, turn: Turn) => void;
}

const ConversationsContext = createContext<Store | null>(null);

/**
 * What the owner asked in each thread, held while they stay in Messages so moving between threads,
 * or between Chat and Desk, keeps it. Leaving Messages drops it: nothing is written to browser
 * storage, and there is no history to reopen (DEC-476).
 */
export function ConversationsProvider({ children }: { children: ReactNode }) {
  const [turns, setTurns] = useState<Record<string, Turn[]>>({});
  const add = useCallback((threadId: string, turn: Turn) => setTurns((all) => ({ ...all, [threadId]: [...(all[threadId] ?? []), turn] })), []);
  const value = useMemo(() => ({ turns, add }), [turns, add]);
  return <ConversationsContext.Provider value={value}>{children}</ConversationsContext.Provider>;
}

/** A thread's turns, from the provider when there is one, else from this component alone. */
export function useTurns(threadId: string): [Turn[], (turn: Turn) => void] {
  const store = useContext(ConversationsContext);
  const [own, setOwn] = useState<Turn[]>([]);
  const addOwn = useCallback((turn: Turn) => setOwn((list) => [...list, turn]), []);
  const add = useCallback((turn: Turn) => store?.add(threadId, turn), [store, threadId]);
  return store ? [store.turns[threadId] ?? [], add] : [own, addOwn];
}
