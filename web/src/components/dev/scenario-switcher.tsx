"use client";

import { useRouter } from "next/navigation";
import { Flask } from "@phosphor-icons/react";
import type { Scenario } from "@/fixtures/types";
import { SCENARIOS } from "@/fixtures/workspace";
import { ROLES, type Role, useRole } from "@/lib/roles";
import { SCENARIO_COOKIE } from "@/lib/scenario";

const SELECT = "h-8 border bg-background px-1.5 text-caption text-foreground";

/**
 * Development only; the layout does not render it in a production build. The scenario is a cookie
 * so the server renders it; the role is React state only and resets on reload.
 */
export function ScenarioSwitcher({ scenario }: { scenario: Scenario }) {
  const router = useRouter();
  const { role, setRole } = useRole();
  return (
    <div className="fixed right-3 bottom-20 z-40 flex flex-wrap items-center gap-2 border-2 border-dashed border-foreground bg-card py-1 pr-1 pl-2.5 text-caption lg:bottom-3">
      <Flask className="size-3.5 text-muted-foreground" aria-hidden />
      <label className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Scenario</span>
        <select
          value={scenario}
          onChange={(e) => {
            document.cookie = `${SCENARIO_COOKIE}=${e.target.value}; path=/; samesite=strict`;
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
    </div>
  );
}
