import type { ReactNode } from "react";
import Link from "next/link";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { cn } from "@/lib/utils";
import styles from "./landing.module.css";
import { COLUMN, PRIMARY_LINK, SECONDARY_LINK } from "./parts";
import { Screenshot } from "./screenshot";

export const HEADLINE = "Let an AI agent trade inside limits you set.";
export const SUBHEAD = "Owlhead checks every order against your mandate before it reaches your broker, and you can stop the agent at any time.";

/** A phone in a hairline frame: the only device frame on the page. */
export function PhoneFrame({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div data-slot="phone-frame" className={className}>
      <div className="rounded-[2.25rem] border border-border bg-card p-1.5">
        <div className="overflow-hidden rounded-[1.875rem] border border-border/70">{children}</div>
      </div>
    </div>
  );
}

export function Hero() {
  return (
    <section aria-labelledby="hero-title" className={COLUMN}>
      <div className="grid gap-7 pt-12 sm:pt-16 lg:pt-20">
        <h1 id="hero-title" className="max-w-[16ch] text-hero text-balance">
          {HEADLINE}
        </h1>
        <p className="max-w-measure text-lg text-pretty text-muted-foreground">{SUBHEAD}</p>
        <div className="flex flex-wrap items-center gap-3">
          <Link href="/login" className={PRIMARY_LINK}>
            Get started
          </Link>
          <a href="#how-it-works" className={SECONDARY_LINK}>
            How it works
          </a>
        </div>
        <p className="flex flex-wrap items-center gap-x-3 gap-y-2 text-sm text-muted-foreground">
          <EnvironmentBadge environment="paper" />
          <span>Paper trading only. Live trading isn&apos;t available.</span>
        </p>
      </div>

      <figure className="mt-12 pb-20 sm:mt-14 lg:mt-16 lg:pb-24">
        <div className="grid items-end gap-6 lg:grid-cols-[minmax(0,1fr)_15rem]">
          <div className="hidden overflow-hidden rounded-xl border border-border/70 sm:block">
            <Screenshot name="hero-desktop" eager sizes="(min-width: 68rem) 47rem, (min-width: 64rem) 70vw, (min-width: 40rem) 92vw, 1px" />
          </div>
          <PhoneFrame className={cn("mx-auto w-[min(18rem,100%)] sm:hidden lg:mx-0 lg:block lg:w-auto", styles.rise)}>
            <Screenshot name="hero-phone" eager sizes="(min-width: 64rem) 15rem, 18rem" />
          </PhoneFrame>
        </div>
        <figcaption className="mt-5 text-caption text-muted-foreground">Screens from Owlhead, with example data on a paper account.</figcaption>
      </figure>
    </section>
  );
}
