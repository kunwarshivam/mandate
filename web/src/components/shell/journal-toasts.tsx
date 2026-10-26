"use client";

import { useEffect, useRef } from "react";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { type CommandKind, useRuntime } from "@/lib/mock-runtime";

/** What a toast may say: the kind of action, never an agent, instrument, or amount. */
function actionName(kind: CommandKind): string {
  switch (kind) {
    case "pause":
    case "pause_all":
      return "Pause";
    case "resume":
      return "Resume";
    case "stop":
    case "stop_all":
      return "Stop";
    case "kill":
      return "Kill switch";
    case "release":
      return "Stop and release";
    case "close_all":
      return "Close everything";
    default: {
      const unhandled: never = kind;
      throw new Error(`unhandled command ${String(unhandled)}`);
    }
  }
}

/**
 * A toast appears only once the runtime has journaled the action. Sending raises nothing; the
 * Stop sheet and the approval screen say "sent" in their own status text. A command the agent's
 * runtime still answers (`follows`: a resume or Stop under step-up, DEC-2140) is announced only once
 * applied: recorded alone, it may yet be refused (web PRODUCT.md P8).
 */
export function JournalToasts() {
  const { commands, responses, setUps } = useRuntime();
  const toasts = useKumoToastManager();
  const announced = useRef(new Set<string>());

  useEffect(() => {
    for (const c of commands) {
      const key = `cmd:${c.id}`;
      const done = c.follows ? c.phase === "applied" : c.phase === "recorded";
      if (!done || announced.current.has(key)) continue;
      announced.current.add(key);
      toasts.add({ title: "Recorded in the journal", description: `${actionName(c.kind)}, as you asked.` });
    }
    for (const r of Object.values(responses)) {
      const key = `resp:${r.approvalId}`;
      if ((r.phase !== "recorded" && r.phase !== "decided") || announced.current.has(key)) continue;
      announced.current.add(key);
      toasts.add({ title: "Recorded in the journal", description: r.response === "skip" ? "Your skip." : "Your approval." });
    }
    for (const s of setUps) {
      const steps = [
        ["version", s.created.phase, "Your mandate version."],
        ["confirm", s.confirmed?.phase, "Your confirmed mandate."],
        ["deploy", s.deployed?.phase, "Your agent, on paper."],
      ] as const;
      for (const [step, phase, description] of steps) {
        const key = `set:${s.id}:${step}`;
        if (phase !== "recorded" || announced.current.has(key)) continue;
        announced.current.add(key);
        toasts.add({ title: "Recorded in the journal", description });
      }
    }
  }, [commands, responses, setUps, toasts]);

  return null;
}
