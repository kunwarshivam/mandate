"use client";

import { type KeyboardEvent as ReactKeyboardEvent, type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { CommandPalette } from "@cloudflare/kumo/components/command-palette";
import { LayerCard } from "@cloudflare/kumo/components/layer-card";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Search } from "pixelarticons/react/Search.js";
import { useRuntime } from "@/lib/mock-runtime";
import { can, useRole } from "@/lib/roles";
import { GROUP_LABEL, listedScreens } from "@/lib/screens";
import { OPEN_STOP_EVENT } from "./stop-control";

/** Opens the palette from elsewhere: Search in the phone's More sheet. */
export const OPEN_COMMAND_EVENT = "owlhead:open-command";

interface Command {
  id: string;
  title: string;
  hint?: string;
  run: () => void;
}

interface CommandGroup {
  label: string;
  items: Command[];
}

const TABBABLE = "a[href], button, input, select, textarea, [tabindex]";

/**
 * The palette's focus trap. The dialog is not modal, so Stop stays in reach (C-21), and Tab and
 * Shift+Tab wrap within the palette instead of leaving it; Escape, a press outside, and choosing a
 * command close it, and "Stop…" is in its list for the keyboard.
 */
function keepTabInside(event: ReactKeyboardEvent<HTMLElement>) {
  if (event.key !== "Tab") return;
  const tabbable = [...event.currentTarget.querySelectorAll<HTMLElement>(TABBABLE)].filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled") && el.checkVisibility(),
  );
  const first = tabbable[0];
  const last = tabbable.at(-1);
  if (!first || !last) return;
  const active = document.activeElement;
  const atEdge = event.shiftKey ? active === first || !event.currentTarget.contains(active) : active === last;
  if (!atEdge) return;
  event.preventDefault();
  (event.shiftKey ? last : first).focus();
}

/** The palette's popup, found from inside it. */
const PALETTE = "[data-slot=command-palette]";

/** The element that had focus before the palette opened; one inside the palette means it was already open. */
function focusOutsidePalette(): HTMLElement | null {
  const active = document.activeElement;
  return active instanceof HTMLElement && !active.closest(PALETTE) ? active : null;
}

/**
 * Once the palette has closed, focus goes back to the element that had it before it opened: the bar,
 * or wherever it was before ⌘K. Only when nothing else has taken focus meanwhile: a press on Stop, or
 * "Stop…", hands focus to the Stop sheet, and it stays there. An opener that has gone, like Search
 * with its More sheet, is left to its sheet.
 */
function returnFocus(to: HTMLElement | null) {
  const active = document.activeElement;
  const lost = active === null || active === document.body || active.closest(PALETTE) !== null;
  if (lost && to?.isConnected) to.focus();
}

/**
 * ⌘K. Go to comes first and "Stop…" last, under Safety, for a role that may stop (DEC-513): first in
 * the list it read as the default act, and it is on every screen already. Titles come from the screen
 * list and from owner-given agent labels only: model output never becomes a command title. Nothing
 * typed here is kept; there are no recents. From `lg` the trigger is a wide bar, centred in the
 * header between its two sides; below it, Search in the tab bar's More sheet opens the same palette.
 *
 * It keeps Tab inside but is not modal, and it and its scrim mount in the frame's sheet layer, over the
 * header and under the dock and the tab bar, so the frame's Stop stays in view, in the accessibility
 * tree and one press away while it is open (C-21, DEC-452). Kumo's own palette dialog is modal: it
 * hid the frame from assistive technology and laid its scrim over Stop, so a press there only closed
 * the palette. This is Kumo's panel in a dialog of our own with Kumo's look, named "Command palette".
 * When it closes, focus goes back to the element that opened it (`returnFocus`).
 */
export function CommandMenu({ container }: { container?: RefObject<HTMLElement | null> }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const router = useRouter();
  const { ws } = useRuntime();
  const { role } = useRole();
  const opener = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        opener.current = focusOutsidePalette() ?? opener.current;
        setOpen((o) => !o);
      }
    };
    const onOpen = () => {
      opener.current = focusOutsidePalette();
      setOpen(true);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_COMMAND_EVENT, onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_COMMAND_EVENT, onOpen);
    };
  }, []);

  const groups = useMemo<CommandGroup[]>(() => {
    const go = (href: string) => () => router.push(href);
    const safety: CommandGroup = {
      label: "Safety",
      items: can(role, "stop.open")
        ? [{ id: "stop", title: "Stop…", hint: "Pause, stop, or use a kill switch", run: () => window.dispatchEvent(new Event(OPEN_STOP_EVENT)) }]
        : [],
    };
    const screens: CommandGroup = {
      label: "Go to",
      items: listedScreens().filter((s) => can(role, s.needs)).map((s) => ({
        id: s.key,
        title: s.label,
        hint: GROUP_LABEL[s.group] ?? undefined,
        run: go(s.href),
      })),
    };
    const agents: CommandGroup = {
      label: "Agents",
      items: can(role, "agents.view") ? ws.agents.map((a) => ({ id: a.agent_id, title: a.label, hint: "Agent", run: go(`/agents/${a.agent_id}`) })) : [],
    };
    const q = query.trim().toLowerCase();
    return [screens, agents, safety]
      .map((g) => ({ ...g, items: q ? g.items.filter((i) => i.title.toLowerCase().includes(q)) : g.items }))
      .filter((g) => g.items.length > 0);
  }, [query, role, router, ws.agents]);

  const select = (item: Command) => {
    setOpen(false);
    setQuery("");
    item.run();
  };

  /** Kumo's input closes its own modal dialog on Escape; this dialog is ours, so it closes it. */
  const closeOnEscape = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    setOpen(false);
    setQuery("");
  };

  return (
    <>
      <button
        type="button"
        onClick={(event) => {
          opener.current = event.currentTarget;
          setOpen(true);
        }}
        aria-keyshortcuts="Meta+K Control+K"
        data-slot="command-bar"
        className="press hidden h-10 min-w-60 flex-[0_1_23.75rem] items-center gap-2.5 rounded-lg border border-border bg-muted pr-1.5 pl-3 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring lg:flex min-[100rem]:flex-[0_1_28.75rem]"
      >
        <Search className="size-6 shrink-0" aria-hidden />
        <span className="min-w-0 flex-1 truncate text-left">Jump to an agent or screen…</span>
        <kbd className="inline-flex h-6 shrink-0 items-center rounded-md border border-border bg-card px-1.5 font-sans text-label text-muted-foreground">⌘K</kbd>
      </button>
      <Dialog.Root
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) setQuery("");
        }}
        onOpenChangeComplete={(isOpen) => {
          if (!isOpen) returnFocus(opener.current);
        }}
        modal={false}
      >
        <Dialog.Portal container={container}>
          <Dialog.Backdrop
            data-slot="command-backdrop"
            className="fixed inset-0 z-30 bg-kumo-overlay opacity-80 transition-all duration-150 data-[ending-style]:opacity-0 data-[starting-style]:opacity-0"
          />
          <LayerCard
            render={<Dialog.Popup data-slot="command-palette" aria-label="Command palette" />}
            className="fixed top-[10vh] left-1/2 z-30 w-full max-w-2xl -translate-x-1/2 overflow-hidden rounded-lg duration-150 data-[ending-style]:scale-90 data-[ending-style]:opacity-0 data-[starting-style]:scale-90 data-[starting-style]:opacity-0"
            style={{ transitionProperty: "scale, opacity", transitionTimingFunction: "var(--default-transition-timing-function)" }}
            onKeyDown={keepTabInside}
          >
            <CommandPalette.Panel<CommandGroup, Command>
              open={open}
              items={groups}
              value={query}
              onValueChange={setQuery}
              itemToStringValue={(g) => g.label}
              filter={() => true}
              onSelect={(item) => select(item)}
              getSelectableItems={(gs) => gs.flatMap((g) => g.items)}
            >
              <CommandPalette.Input placeholder="Go to a screen or an agent, or type Stop" aria-label="Command" onKeyDown={closeOnEscape} />
              <CommandPalette.List>
                <CommandPalette.Results>
                  {(group: CommandGroup) => (
                    <CommandPalette.Group key={group.label} items={group.items}>
                      <CommandPalette.GroupLabel>{group.label}</CommandPalette.GroupLabel>
                      <CommandPalette.Items>
                        {(item: Command) => (
                          <CommandPalette.ResultItem key={item.id} title={item.title} description={item.hint} value={item} onClick={() => select(item)} />
                        )}
                      </CommandPalette.Items>
                    </CommandPalette.Group>
                  )}
                </CommandPalette.Results>
                <CommandPalette.Empty>No screen or agent by that name.</CommandPalette.Empty>
              </CommandPalette.List>
            </CommandPalette.Panel>
          </LayerCard>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}
