"use client";

import { type FormEvent, useState } from "react";
import { WarningDiamond } from "pixelarticons/react/WarningDiamond.js";
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

type Look = "guestbook" | "page";

/**
 * How the form is drawn: as a dialog of the late 1990s in the guestbook's window, or in the long
 * page's own type under the desktop (DEC-907). Both send the same request.
 */
const LOOKS: Record<Look, { face: string; field: string; roles: string; role: string; button: string; problem: string; submit: string; sending: string }> = {
  guestbook: {
    face: PIXEL,
    field: FIELD,
    roles: cn(SUNKEN, "grid gap-1.5 px-3 pt-1 pb-3"),
    role: "flex w-fit cursor-pointer items-center gap-2",
    button: BUTTON,
    problem: "border border-foreground bg-warning-soft",
    submit: "Sign the guestbook",
    sending: "Signing...",
  },
  page: {
    face: "",
    field:
      "h-12 w-full max-w-[26rem] rounded-full bg-card px-5 text-[1.0625rem] text-foreground ring-1 ring-foreground/25 outline-none placeholder:text-muted-foreground focus-visible:ring-3 focus-visible:ring-ring",
    roles: "flex flex-wrap gap-2",
    role: "flex cursor-pointer items-center gap-2 rounded-full px-3.5 py-2 text-[0.9375rem] ring-1 ring-foreground/25 has-checked:bg-foreground has-checked:text-background has-checked:ring-foreground has-focus-visible:ring-3 has-focus-visible:ring-ring",
    button:
      "inline-flex h-12 items-center rounded-full bg-foreground px-6 text-[1rem] font-semibold text-background outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:opacity-60",
    problem: "rounded-xl bg-warning-soft ring-1 ring-foreground/25",
    submit: "Ask for a place",
    sending: "Sending...",
  },
};

/**
 * The private beta's request form, set as a dialog: an email, an optional answer, and one button. `id`
 * prefixes its element ids, so a second copy (the page's no-script fallback) never repeats them.
 */
export function BetaForm({ id = "beta", look = "guestbook", className }: { id?: string; look?: Look; className?: string }) {
  const style = LOOKS[look];
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
      <div role="status" className={cn("grid gap-2 text-base", style.face, className)} data-slot="beta-done">
        <p className="font-semibold">Thank you. You&apos;re on the list.</p>
        <p>
          We&apos;ll write to <strong className="font-medium">{email.trim()}</strong> when your place opens.
        </p>
      </div>
    );
  }

  const message = problem(status);

  return (
    <form onSubmit={submit} className={cn("relative grid gap-5 text-base", style.face, className)} data-slot="beta-form">
      <p className="grid gap-1.5">
        <label htmlFor={`${id}-email`}>
          {look === "guestbook" ? (
            <>
              <span className="underline">E</span>mail address:
            </>
          ) : (
            "Email address"
          )}
        </label>
        <input
          id={`${id}-email`}
          type="email"
          name="email"
          required
          autoComplete="email"
          accessKey="e"
          placeholder="you@example.com"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          aria-invalid={status === "email" || undefined}
          aria-describedby={message ? `${id}-problem` : undefined}
          className={style.field}
        />
      </p>

      <fieldset className={style.roles}>
        <legend className={look === "guestbook" ? "px-1" : "mb-2.5 text-[0.9375rem] text-muted-foreground"}>What would you use it for? (optional)</legend>
        {BETA_ROLES.map((value) => (
          <label key={value} className={style.role}>
            <input type="radio" name="role" value={value} checked={role === value} onChange={() => setRole(value)} className={look === "guestbook" ? "size-3.5 accent-foreground" : "sr-only"} />
            {ROLE_LABELS[value]}
          </label>
        ))}
      </fieldset>

      <div aria-hidden className="absolute -left-[9999px] h-px w-px overflow-hidden">
        <label htmlFor={`${id}-website`}>Website</label>
        <input id={`${id}-website`} type="text" name="website" tabIndex={-1} autoComplete="off" value={website} onChange={(event) => setWebsite(event.target.value)} />
      </div>

      <div className="grid gap-3">
        <p>
          <button type="submit" disabled={status === "sending"} className={style.button}>
            {status === "sending" ? style.sending : style.submit}
          </button>
        </p>
        <p
          id={`${id}-problem`}
          role="status"
          className={message ? cn("flex max-w-[22rem] items-start gap-2 px-3 py-2", style.problem) : "sr-only"}
          data-slot={message ? "beta-problem" : undefined}
        >
          {message && <WarningDiamond aria-hidden className="size-6 shrink-0" />}
          <span>{message ?? (status === "sending" ? "Sending your request" : "")}</span>
        </p>
      </div>
    </form>
  );
}
