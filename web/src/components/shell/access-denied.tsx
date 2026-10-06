"use client";

import Link from "next/link";
import { Lock } from "pixelarticons/react/Lock.js";
import { homeFor } from "@/lib/access";
import { ROLES, type Role, can } from "@/lib/roles";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

/** Rendered by the shell in place of a page the role may not open; the page itself never mounts. */
export function AccessDenied({ role }: { role: Role }) {
  const home = homeFor(role);
  const label = ROLES.find((r) => r.id === role)?.label ?? role;
  return (
    <section data-slot="access-denied" aria-labelledby="denied-title" className="reveal grid max-w-2xl gap-4 pt-6 sm:pt-12">
      <Lock className="size-6" aria-hidden />
      <h1 id="denied-title" className="text-h1 sm:text-h1">
        Not available to your role
      </h1>
      <p className="max-w-measure">
        {can(role, "agents.view")
          ? `As ${label.toLowerCase()}, you cannot open this screen. An owner or operator can.`
          : `As ${label.toLowerCase()}, you see the journal and its exports, and nothing that acts.`}
      </p>
      <Link href={home.href} className={cn("w-fit", KEY)}>
        Go to the {home.label}
      </Link>
    </section>
  );
}
