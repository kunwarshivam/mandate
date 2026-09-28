"use client";

import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { CommandPalette } from "@cloudflare/kumo/components/command-palette";
import { MagnifyingGlass } from "@phosphor-icons/react";
import { useRuntime } from "@/lib/mock-runtime";
import { can, useRole } from "@/lib/roles";
import { GROUP_LABEL, SCREENS } from "@/lib/screens";
import { OPEN_STOP_EVENT } from "./stop-control";

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

/**
 * ⌘K. "Stop…" is always the first command for a role that may stop. Titles come from the screen
 * list and from owner-given agent labels only: model output never becomes a command title. Nothing
 * typed here is kept; there are no recents.
 */
export function CommandMenu() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const router = useRouter();
  const { ws } = useRuntime();
  const { role } = useRole();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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
      items: SCREENS.filter((s) => can(role, s.needs)).map((s) => ({
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
    return [safety, screens, agents]
      .map((g) => ({ ...g, items: q ? g.items.filter((i) => i.title.toLowerCase().includes(q)) : g.items }))
      .filter((g) => g.items.length > 0);
  }, [query, role, router, ws.agents]);

  const select = (item: Command) => {
    setOpen(false);
    setQuery("");
    item.run();
  };

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-keyshortcuts="Meta+K Control+K"
        className="press inline-flex h-11 shrink-0 items-center gap-2 border border-border bg-card px-3 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
      >
        <MagnifyingGlass className="size-4" aria-hidden />
        <span className="max-lg:sr-only">Go to…</span>
        <kbd className="hidden border border-border px-1 font-mono text-label lg:inline">⌘K</kbd>
      </button>
      <CommandPalette.Root<CommandGroup, Command>
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) setQuery("");
        }}
        items={groups}
        value={query}
        onValueChange={setQuery}
        itemToStringValue={(g) => g.label}
        filter={() => true}
        onSelect={(item) => select(item)}
        getSelectableItems={(gs) => gs.flatMap((g) => g.items)}
      >
        <CommandPalette.Input placeholder="Go to a screen or an agent, or type Stop" aria-label="Command" />
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
      </CommandPalette.Root>
    </>
  );
}
