import type { Metadata } from "next";
import Link from "next/link";
import { OwlheadLockup } from "@/components/brand/Logo";
import { PRIMARY_PILL } from "@/components/auth/buttons";
import { cn } from "@/lib/utils";

export const metadata: Metadata = { title: { absolute: "Owlhead" } };

/** A placeholder until the landing page (`src/components/site/landing.tsx`) is wired in here. */
export default function WelcomePage() {
  return (
    <section aria-labelledby="welcome-title" className="mx-auto grid w-full max-w-md justify-items-start gap-6 pt-10 sm:pt-24" data-slot="welcome">
      <h1 id="welcome-title" className="sr-only">
        Owlhead
      </h1>
      <span style={{ color: "var(--logo)" }}>
        <OwlheadLockup title="" className="h-12 w-auto" />
      </span>
      <p className="text-muted-foreground">Sign in to see your agents, the limits you set for them, and what needs you.</p>
      <Link href="/login" className={cn(PRIMARY_PILL, "w-auto")}>
        Sign in
      </Link>
    </section>
  );
}
