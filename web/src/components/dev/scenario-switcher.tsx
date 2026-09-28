"use client";

import { useEffect } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Flask } from "@phosphor-icons/react";
import type { Scenario } from "@/fixtures/types";
import { SCENARIOS } from "@/fixtures/workspace";
import { CVD_COOKIE } from "@/lib/colour-pref";
import { ROLES, type Role, useRole } from "@/lib/roles";
import { SCENARIO_COOKIE } from "@/lib/scenario";

const SELECT = "h-8 border bg-background px-1.5 text-caption text-foreground";

function setCookie(name: string, value: string) {
  document.cookie = `${name}=${value}; path=/; samesite=strict`;
}

/**
 * Development only; the layout does not render it in a production build. The scenario and the
 * colour-blind friendly preference are cookies so the server renders them; the role is React state
 * only and resets on reload. Alt+Shift+C toggles colour-blind friendly.
 */
export function ScenarioSwitcher({ scenario, colourBlind }: { scenario: Scenario; colourBlind: boolean }) {
  const router = useRouter();
  const { role, setRole } = useRole();

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!e.altKey || !e.shiftKey || e.code !== "KeyC") return;
      e.preventDefault();
      setCookie(CVD_COOKIE, colourBlind ? "off" : "on");
      router.refresh();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [colourBlind, router]);

  return (
    <div className="fixed right-3 bottom-20 z-40 flex flex-wrap items-center gap-2 border border-dashed border-foreground bg-card py-1 pr-1 pl-2.5 text-caption lg:bottom-3">
      <Flask className="size-3.5 text-muted-foreground" aria-hidden />
      <label className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Scenario</span>
        <select
          value={scenario}
          onChange={(e) => {
            setCookie(SCENARIO_COOKIE, e.target.value);
            router.refresh();
          }}
          className={SELECT}
        >
          {SCENARIOS.map((s) => (
            <option key={s.id} value={s.id}>
              {s.label}
            </option>
          ))}
        </select>
      </label>
      <label className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Role</span>
        <select value={role} onChange={(e) => setRole(e.target.value as Role)} className={SELECT}>
          {ROLES.map((r) => (
            <option key={r.id} value={r.id}>
              {r.label}
            </option>
          ))}
        </select>
      </label>
      <label className="flex h-8 items-center gap-1.5" title="Alt+Shift+C">
        <input
          type="checkbox"
          checked={colourBlind}
          onChange={(e) => {
            setCookie(CVD_COOKIE, e.target.checked ? "on" : "off");
            router.refresh();
          }}
          className="size-4 accent-foreground"
        />
        Colour-blind friendly
      </label>
      <Link href="/palette" className="px-1.5 text-primary underline underline-offset-2">
        Palette
      </Link>
    </div>
  );
}
