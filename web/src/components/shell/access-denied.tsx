"use client";

import Link from "next/link";
import { LockSimple } from "@phosphor-icons/react";
import { homeFor } from "@/lib/access";
import { ROLES, type Role, can } from "@/lib/roles";

/** Rendered by the shell in place of a page the role may not open; the page itself never mounts. */
export function AccessDenied({ role }: { role: Role }) {
  const home = homeFor(role);
  const label = ROLES.find((r) => r.id === role)?.label ?? role;
  return (
    <section data-slot="access-denied" aria-labelledby="denied-title" className="reveal grid max-w-3xl gap-3 border-t-4 border-foreground bg-muted p-4 sm:p-6">
      <LockSimple className="size-6" aria-hidden />
      <h1 id="denied-title" className="text-title sm:text-display">
        Not available to your role
      </h1>
      <p className="max-w-prose">
        {can(role, "agents.view")
          ? `As ${label.toLowerCase()}, you cannot open this screen. An owner or operator can.`
          : `As ${label.toLowerCase()}, you see the journal and its exports, and nothing that acts.`}
      </p>
      <Link href={home.href} className="press inline-flex h-11 w-fit items-center border-2 border-foreground bg-card px-4 font-bold hover:bg-background">
        Go to the {home.label}
      </Link>
    </section>
  );
}
