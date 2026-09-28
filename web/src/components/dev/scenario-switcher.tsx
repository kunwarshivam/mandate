"use client";

import { useRouter } from "next/navigation";
import { FlaskConical } from "lucide-react";
import type { Scenario } from "@/fixtures/types";
import { SCENARIOS } from "@/fixtures/workspace";
import { SCENARIO_COOKIE } from "@/lib/scenario";

/** Development only; the layout does not render it in a production build. */
export function ScenarioSwitcher({ scenario }: { scenario: Scenario }) {
  const router = useRouter();
  return (
    <label className="fixed bottom-20 left-3 z-40 flex items-center gap-2 rounded-lg border bg-card/95 py-1 pr-1 pl-2.5 text-caption shadow-whisper backdrop-blur lg:bottom-3">
      <FlaskConical className="size-3.5 text-muted-foreground" aria-hidden />
      <span className="text-muted-foreground">Scenario</span>
      <select
        value={scenario}
        onChange={(e) => {
          document.cookie = `${SCENARIO_COOKIE}=${e.target.value}; path=/; samesite=strict`;
          router.refresh();
        }}
        className="h-7 rounded-md border bg-background px-1.5 text-caption text-foreground"
      >
        {SCENARIOS.map((s) => (
          <option key={s.id} value={s.id}>
            {s.label}
          </option>
        ))}
      </select>
    </label>
  );
}
