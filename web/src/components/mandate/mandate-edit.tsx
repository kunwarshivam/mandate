"use client";

import { useId, useState } from "react";
import { FIELD } from "@/components/auth/buttons";
import type { Agent } from "@/fixtures/types";
import { type EditPath, type EditableField, type Edits, type FieldGroup, type Proposal, editableFields, inputText, propose, readInput, valueAt } from "@/lib/mandate-change";
import { changeLabel } from "@/lib/mandate-paths";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import { ChangeReview } from "./change-review";

const GROUP_TITLE: Record<FieldGroup, string> = {
  capital: "Capital and loss limits",
  size: "Size and pace",
  protection: "Protection",
  approvals: "Approvals",
  quiet: "Quiet hours",
};

const GROUPS: readonly FieldGroup[] = ["capital", "size", "protection", "approvals", "quiet"];

/** What a figure means, beside its unit, so a percentage is never read against the wrong base. */
function hintFor(field: EditableField, agent: Agent): string {
  switch (field.path) {
    case "/capital/allocation_usd":
      return "Dollars of the account this agent may use.";
    case "/capital/max_loss_from_allocation":
      return "Percent of capital. Reached, the agent only exits.";
    case "/risk/max_daily_loss":
      return "Percent of the day's starting equity. Reached, the agent only exits for the day.";
    case "/risk/max_order_usd":
    case "/risk/max_position_usd":
    case "/risk/max_gross_exposure_usd":
      return "Dollars.";
    case "/risk/max_position_fraction":
      return "Percent of the agent's equity.";
    case "/risk/max_orders_per_day":
      return "A whole number, 1 to 10,000.";
    case "/protection/stop_distance":
      return "Percent below cost.";
    case "/autonomy/approval/timeout_s":
      return "Minutes before an unanswered request expires and nothing is sent.";
    case "/autonomy/approval/two_approver_above_usd":
      return "Dollars. Leave it empty and one approver is enough.";
    case "/notifications/quiet_hours/start":
    case "/notifications/quiet_hours/end":
      return `24-hour time, ${agent.mandate.notifications.quiet_hours?.timezone ?? "UTC"}.`;
    default: {
      const unhandled: never = field.path;
      throw new Error(`unhandled path ${String(unhandled)}`);
    }
  }
}

const INPUT_MODE: Record<EditableField["unit"], "decimal" | "numeric" | "text"> = {
  usd: "decimal",
  percent: "decimal",
  count: "numeric",
  minutes: "numeric",
  time: "text",
};

type Raw = Partial<Record<EditPath, string>>;

function rawOf(agent: Agent): Raw {
  return Object.fromEntries(editableFields(agent.mandate).map((f) => [f.path, inputText(f, valueAt(agent.mandate, f.path))]));
}

/**
 * Mandate › Edit (A6): the limits a running agent's mandate lets the owner change, each in its own
 * unit, and below them the review of the version they make, confirmed once. A confirmed version
 * keeps its card while it applies, and the fields start again from the version in effect.
 */
export function MandateEdit({ agent }: { agent: Agent }) {
  const { ws } = useRuntime();
  const id = useId();
  const [basis, setBasis] = useState(agent.mandate_version);
  const [raw, setRaw] = useState<Raw>(() => rawOf(agent));
  const [touched, setTouched] = useState<ReadonlySet<EditPath>>(new Set());
  const [card, setCard] = useState<{ key: number; sent: Proposal | null }>({ key: 0, sent: null });

  if (basis !== agent.mandate_version) {
    setBasis(agent.mandate_version);
    setRaw(rawOf(agent));
    setTouched(new Set());
  }

  const fields = editableFields(agent.mandate);
  const edits: Edits = {};
  const errors = new Map<EditPath, string>();
  for (const f of fields) {
    const parsed = readInput(f, raw[f.path] ?? "");
    if (parsed.ok) edits[f.path] = parsed.value;
    else errors.set(f.path, parsed.error);
  }
  const live = errors.size === 0 ? propose(ws, agent, edits) : null;
  const shown = card.sent ?? (live && live.changes.length > 0 ? live : null);

  const edit = (path: EditPath, value: string) => {
    setRaw((r) => ({ ...r, [path]: value }));
    if (card.sent) setCard((c) => ({ key: c.key + 1, sent: null }));
  };
  const reset = () => {
    setRaw(rawOf(agent));
    setTouched(new Set());
    setCard((c) => ({ key: c.key + 1, sent: null }));
  };

  return (
    <div className="grid gap-(--section-gap)">
      <p className="max-w-measure text-sm text-pretty text-muted-foreground">
        Instruments, signal models, your rules, the goal and the environment are not changed here. You can also ask for any of these limits in the agent&apos;s thread; the same
        review comes back there.
      </p>

      <form aria-label={`Edit ${agent.label}'s mandate`} className="grid max-w-2xl gap-6" onSubmit={(e) => e.preventDefault()} noValidate>
        {GROUPS.filter((g) => fields.some((f) => f.group === g)).map((group) => (
          <fieldset key={group} className="grid gap-4">
            <legend className="mb-3 text-h3">{GROUP_TITLE[group]}</legend>
            {fields
              .filter((f) => f.group === group)
              .map((f) => {
                const inputId = `${id}-${f.path}`;
                const error = touched.has(f.path) ? errors.get(f.path) : undefined;
                return (
                  <div key={f.path} className="grid gap-1.5" data-slot="edit-field" data-path={f.path}>
                    <label htmlFor={inputId} className="field-label">
                      {changeLabel(f.path)}
                    </label>
                    <input
                      id={inputId}
                      value={raw[f.path] ?? ""}
                      inputMode={INPUT_MODE[f.unit]}
                      autoComplete="off"
                      spellCheck={false}
                      aria-invalid={error ? true : undefined}
                      aria-describedby={`${inputId}-hint${error ? ` ${inputId}-error` : ""}`}
                      onChange={(e) => edit(f.path, e.target.value)}
                      onBlur={() => setTouched((t) => new Set(t).add(f.path))}
                      className={cn(FIELD, "h-11 max-w-[16rem] font-mono tabular-nums", error ? "border-foreground" : null)}
                    />
                    <p id={`${inputId}-hint`} className="text-caption text-pretty text-muted-foreground">
                      {hintFor(f, agent)}
                    </p>
                    {error ? (
                      <p id={`${inputId}-error`} className="text-sm font-semibold">
                        {error}
                      </p>
                    ) : null}
                  </div>
                );
              })}
          </fieldset>
        ))}
      </form>

      <div className="grid gap-3">
        {shown ? (
          <ChangeReview
            key={card.key}
            proposal={shown}
            origin={{ kind: "form" }}
            fixHint="Change a value above to continue."
            onKeep={card.sent ? undefined : reset}
            onSent={() => setCard((c) => ({ ...c, sent: shown }))}
          />
        ) : errors.size > 0 ? (
          <p className="text-sm text-muted-foreground">
            {[...errors.keys()].some((p) => touched.has(p)) ? "Correct the value marked above to see what the change does." : "Finish the value you are writing to see what the change does."}
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">Change a value to see what it does.</p>
        )}
      </div>
    </div>
  );
}
