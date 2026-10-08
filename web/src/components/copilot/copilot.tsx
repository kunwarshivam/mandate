"use client";

import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import { Calendar } from "pixelarticons/react/Calendar.js";
import { ChevronRight } from "pixelarticons/react/ChevronRight.js";
import { Close } from "pixelarticons/react/Close.js";
import { Inbox } from "pixelarticons/react/Inbox.js";
import { Pause } from "pixelarticons/react/Pause.js";
import { Plus } from "pixelarticons/react/Plus.js";
import { Sliders } from "pixelarticons/react/Sliders.js";
import { BrandOwl } from "@/components/brand/brand-owl";
import { Composer, type ComposerHandle } from "@/components/chat/composer";
import { JumpToLatest, useFollow } from "@/components/chat/follow";
import { OwnerSaid, RecordReply } from "@/components/chat/record-reply";
import type { Icon } from "@/components/icon";
import { agentIdFrom } from "@/components/shell/stop-control";
import { findAgent } from "@/fixtures/workspace";
import { type Reply, interpret } from "@/lib/ask-record";
import { useStartSetup } from "@/lib/handoff";
import { useRuntime } from "@/lib/mock-runtime";
import { crumbsFor } from "@/lib/screens";
import { cn } from "@/lib/utils";

/**
 * The panel's width on a desktop. From 110rem the page and the dock make room for it; narrower, the
 * page keeps its own breakpoints' width, so the panel floats over its right side, below the header
 * and above the dock, where the paper badge and Stop stay in view.
 */
export const COPILOT_WIDTH = "26rem";

const DESKTOP = "(min-width: 64rem)";

interface Copilot {
  open: boolean;
  show: () => void;
  hide: () => void;
  toggle: () => void;
}

const CLOSED: Copilot = { open: false, show: () => {}, hide: () => {}, toggle: () => {} };

const CopilotContext = createContext<Copilot>(CLOSED);

export function useCopilot(): Copilot {
  return useContext(CopilotContext);
}

/**
 * Whether Owlhead is open (DEC-479 item 8). On a desktop it is docked and stays open across pages,
 * following the screen it looks at. On a phone it is a sheet over one page and closes when the owner
 * leaves that page. ⌘J or Ctrl+J opens and closes it anywhere in the app.
 */
export function CopilotProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [state, setState] = useState<{ sheetOn: string | null } | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const open = state !== null && (state.sheetOn === null || state.sheetOn === pathname);

  const show = useCallback(() => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const docked = window.matchMedia?.(DESKTOP).matches ?? true;
    setState({ sheetOn: docked ? null : pathname });
  }, [pathname]);
  const hide = useCallback(() => {
    setState(null);
    opener.current?.focus();
  }, []);
  const toggle = useCallback(() => (open ? hide() : show()), [open, hide, show]);

  useEffect(() => {
    const keys = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "j") {
        event.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [toggle]);

  const value = useMemo<Copilot>(() => ({ open, show, hide, toggle }), [open, show, hide, toggle]);
  return <CopilotContext.Provider value={value}>{children}</CopilotContext.Provider>;
}

/** In the header from `lg`: the brand owl, "Ask Owlhead" from 100rem, and the shortcut. */
export function CopilotButton({ className }: { className?: string }) {
  const { open, toggle } = useCopilot();
  return (
    <button
      type="button"
      onClick={toggle}
      aria-expanded={open}
      aria-controls="owlhead-copilot"
      aria-keyshortcuts="Meta+J Control+J"
      data-slot="copilot-button"
      className={cn(
        "press inline-flex h-10 shrink-0 items-center gap-1.5 rounded-lg px-2 text-sm font-medium text-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring aria-expanded:bg-muted",
        className,
      )}
    >
      <BrandOwl still className="size-8" />
      <span className="max-[100rem]:sr-only">Ask Owlhead</span>
      <kbd className="rounded-sm border border-border px-1 font-mono text-[0.6875rem] text-muted-foreground max-[100rem]:hidden" aria-hidden>
        ⌘J
      </kbd>
    </button>
  );
}

interface Turn {
  id: number;
  said: string;
  reply: Reply;
}

const STARTS: ReadonlyArray<{ say: string; label: string; icon: Icon }> = [
  { say: "What needs me?", label: "What needs me?", icon: Inbox },
  { say: "How close are my agents to their limits?", label: "How close are my agents to their limits?", icon: Sliders },
  { say: "What happened today?", label: "What happened today?", icon: Calendar },
  { say: "Pause an agent", label: "Pause an agent", icon: Pause },
  { say: "Create an agent", label: "Create an agent", icon: Plus },
];

const ICON_BUTTON =
  "press inline-flex size-11 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring";

function Panel({ onClose }: { onClose: () => void }) {
  const { ws, now } = useRuntime();
  const pathname = usePathname();
  const start = useStartSetup();
  const [turns, setTurns] = useState<Turn[]>([]);
  const [unscopedOn, setUnscopedOn] = useState<string | null>(null);
  const composer = useRef<ComposerHandle>(null);
  const { scroller, content, away, jump, pin } = useFollow("owlhead");

  const onPage = agentIdFrom(pathname);
  const agentId = unscopedOn === pathname ? null : onPage;
  const agent = agentId ? findAgent(ws, agentId) : undefined;
  const screen = crumbsFor(pathname, (id) => findAgent(ws, id)?.label).at(-1)?.label ?? "Home";
  const waiting = interpret("What needs me?", { ws, now, agentId: null });
  const summary = waiting.kind === "answer" ? waiting.lines[0] : "";

  useEffect(() => {
    composer.current?.focus();
  }, []);
  const ask = (said: string) => {
    pin();
    setTurns((t) => [...t, { id: t.length, said, reply: interpret(said, { ws, now, agentId: agent?.agent_id ?? null }) }]);
    composer.current?.focus();
  };
  const create = (text: string | null) => {
    start(text);
    onClose();
  };

  return (
    <aside
      id="owlhead-copilot"
      aria-label="Owlhead"
      data-slot="copilot"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
      className={cn(
        "z-[21] flex flex-col border-border bg-card",
        "max-lg:fixed max-lg:inset-x-0 max-lg:top-16 max-lg:group-has-[[data-slot=thread-pane]]/frame:top-0 max-lg:bottom-[calc(var(--tab-bar)+env(safe-area-inset-bottom))] max-lg:border-t",
        "lg:fixed lg:top-16 lg:right-0 lg:bottom-(--dock-clearance) lg:w-(--copilot-w) lg:rounded-bl-2xl lg:border-b lg:border-l",
        "min-[110rem]:sticky min-[110rem]:top-0 min-[110rem]:h-dvh min-[110rem]:shrink-0 min-[110rem]:rounded-none min-[110rem]:border-b-0",
      )}
    >
      <header className="flex h-14 shrink-0 items-center gap-2 border-b border-border/70 pr-2 pl-5 lg:h-16">
        <BrandOwl still className="size-8" />
        <h2 className="flex-1 text-h3">Owlhead</h2>
        {turns.length > 0 ? (
          <button type="button" aria-label="New conversation" className={ICON_BUTTON} onClick={() => setTurns([])}>
            <Plus aria-hidden className="size-6" />
          </button>
        ) : null}
        <button type="button" aria-label="Close Owlhead" className={ICON_BUTTON} onClick={onClose}>
          <Close aria-hidden className="size-6" />
        </button>
      </header>

      <div ref={scroller} className="relative min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div ref={content} className="grid grid-cols-[minmax(0,1fr)] content-start gap-5 px-5 py-5" data-slot="copilot-body">
          <p className="flex flex-wrap items-center gap-2 text-caption text-muted-foreground" data-slot="looking-at">
            Looking at
            <span className={cn("inline-flex h-8 items-center gap-0.5 rounded-full border border-border pl-2.5 font-medium text-foreground", agent ? "pr-0.5" : "pr-2.5")}>
              {agent ? agent.label : screen}
              {agent ? (
                <button
                  type="button"
                  aria-label={`Ask about all agents, not only ${agent.label}`}
                  className="press inline-flex size-8 items-center justify-center rounded-full outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring max-lg:-my-1.5 max-lg:size-11"
                  onClick={() => setUnscopedOn(pathname)}
                >
                  <Close aria-hidden className="size-6" />
                </button>
              ) : null}
            </span>
          </p>

          {turns.length === 0 ? (
            <div className="grid gap-4" data-slot="copilot-start">
              <BrandOwl className="size-12" />
              <div className="grid gap-1">
                <p className="text-h3 text-pretty">Ask about your agents.</p>
                <p className="text-sm text-pretty text-muted-foreground">{summary}</p>
              </div>
              <ul aria-label="Start with" className="grid border-t border-border/70">
                {STARTS.map(({ say, label, icon: Glyph }) => (
                  <li key={say} className="border-b border-border/70">
                    <button type="button" onClick={() => ask(say)} className="press flex min-h-12 w-full items-center gap-3 rounded-md px-1 text-left text-sm outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring">
                      <Glyph aria-hidden className="size-6 shrink-0 text-muted-foreground" />
                      <span className="flex-1">{label}</span>
                      <ChevronRight aria-hidden className="size-6 shrink-0 text-muted-foreground" />
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <div role="log" aria-label="Owlhead conversation" className="grid gap-5">
              {turns.map((turn) => (
                <div key={turn.id} className="grid gap-3">
                  <OwnerSaid text={turn.said} />
                  <RecordReply reply={turn.reply} onAsk={ask} onCreate={create} />
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="relative shrink-0 px-5 pt-2 pb-4">
        <JumpToLatest
          away={away}
          onJump={() => {
            jump();
            composer.current?.focus();
          }}
        />
        <Composer ref={composer} slot="copilot-composer" label="Ask Owlhead" placeholder="Ask, or tell Owlhead what to do" onSend={ask} />
      </div>
    </aside>
  );
}

/** Mounted only while open, so nothing asked outlives the panel (DEC-479 item 8). */
export function CopilotPanel() {
  const { open, hide } = useCopilot();
  return open ? <Panel onClose={hide} /> : null;
}
