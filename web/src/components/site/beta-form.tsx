"use client";

import { type FormEvent, useState } from "react";
import { BETA_REQUEST_PATH } from "@/lib/auth-routes";
import { BETA_ROLES, type BetaRole } from "@/lib/beta";
import { cn } from "@/lib/utils";
import { BUTTON, FIELD, PIXEL, SUNKEN } from "./letter";

export const ROLE_LABELS: Record<BetaRole, string> = {
  own: "Trading my own account",
  clients: "Managing money for others",
  desk: "Running a trading desk",
  builder: "Connecting an agent I've built",
};

type Status = "idle" | "sending" | "done" | "email" | "unavailable";

function problem(status: Status): string | null {
  switch (status) {
    case "email":
      return "That email doesn't look right. Check it and try again.";
    case "unavailable":
      return "We couldn't save that just now. Try again in a minute.";
    case "idle":
    case "sending":
    case "done":
      return null;
    default: {
      const unreachable: never = status;
      return unreachable;
    }
  }
}

/** The private beta's request form, set as a dialog: an email, an optional answer, and one button. */
export function BetaForm({ className }: { className?: string }) {
  const [status, setStatus] = useState<Status>("idle");
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<BetaRole | null>(null);
  const [website, setWebsite] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setStatus("sending");
    try {
      const response = await fetch(BETA_REQUEST_PATH, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email, role, website }),
      });
      if (response.ok) setStatus("done");
      else setStatus(response.status === 400 ? "email" : "unavailable");
    } catch {
      setStatus("unavailable");
    }
  }

  if (status === "done") {
    return (
      <div role="status" className={cn("grid gap-2 text-base", PIXEL, className)} data-slot="beta-done">
        <p className="font-semibold">Thank you. You&apos;re on the list.</p>
        <p>
          We&apos;ll write to <strong className="font-medium">{email.trim()}</strong> when your place opens.
        </p>
      </div>
    );
  }

  const message = problem(status);

  return (
    <form onSubmit={submit} className={cn("relative grid gap-5 text-base", PIXEL, className)} data-slot="beta-form">
      <p className="grid gap-1.5">
        <label htmlFor="beta-email">
          <span className="underline">E</span>mail address:
        </label>
        <input
          id="beta-email"
          type="email"
          name="email"
          required
          autoComplete="email"
          accessKey="e"
          placeholder="you@example.com"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          aria-invalid={status === "email" || undefined}
          aria-describedby={message ? "beta-problem" : undefined}
          className={FIELD}
        />
      </p>

      <fieldset className={cn(SUNKEN, "grid gap-1.5 px-3 pt-1 pb-3")}>
        <legend className="px-1">What would you use it for? (optional)</legend>
        {BETA_ROLES.map((value) => (
          <label key={value} className="flex w-fit cursor-pointer items-center gap-2">
            <input type="radio" name="role" value={value} checked={role === value} onChange={() => setRole(value)} className="size-3.5 accent-foreground" />
            {ROLE_LABELS[value]}
          </label>
        ))}
      </fieldset>

      <div aria-hidden className="absolute -left-[9999px] h-px w-px overflow-hidden">
        <label htmlFor="beta-website">Website</label>
        <input id="beta-website" type="text" name="website" tabIndex={-1} autoComplete="off" value={website} onChange={(event) => setWebsite(event.target.value)} />
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <button type="submit" disabled={status === "sending"} className={BUTTON}>
          {status === "sending" ? "Sending..." : "Request access"}
        </button>
        <p id="beta-problem" role="status" className={message ? "font-semibold" : "sr-only"}>
          {message ?? (status === "sending" ? "Sending your request" : "")}
        </p>
      </div>
    </form>
  );
}
