import type { CSSProperties, ReactNode } from "react";
import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";
import { BetaForm } from "../beta-form";
import { Ambient } from "./ambient";
import { LockScreen } from "./lock-screen";
import { DISPLAY, DISPLAY_MD, INTRO_ID, LEAD, LEAD_ON_TIDE, PAGE_FORM_ID, PRIMARY, SECONDARY, SIGN_UP_ID, WRAP } from "./parts";
import { PixelSea } from "./pixel-sea";
import { Reveals } from "./reveals";
import { ScrollBar } from "./scroll-bar";
import { Shot } from "./shot";
import { SPRING_VARS } from "./spring";
import { Wont } from "./wont";
import styles from "./scroll.module.css";
import { SIGN_IN_HREF } from "@/lib/hosts";

/** Limits are never said to cap a loss without this beside them. */
export const GAP_CAVEAT =
  "Limits can't prevent every loss. When a market gaps or halts, prices can move past a limit before an order fills, so a loss can end up larger than the limit.";

/**
 * The brand owl standing still on a picture's top edge, blinking and following the pointer with its
 * eyes as every owl on the site does (DEC-906, DEC-932): the page's one quiet sign of life in each
 * part, never a flight. `side` is where along the edge it stands. Decoration, hidden from assistive
 * technology.
 */
function Watcher({ side, children, className }: { side: "left" | "right"; children: ReactNode; className?: string }) {
  return (
    <div className={cn("relative", className)}>
      <span aria-hidden className={cn("absolute -top-10 z-[1] sm:-top-12", side === "left" ? "left-10" : "right-10")} style={{ color: "var(--logo)" }} data-slot="watcher">
        <BrandOwl className="size-12 sm:size-14" />
      </span>
      {children}
    </div>
  );
}

/** The order a piece settles in among its neighbours, as `scroll.module.css` staggers it. */
const nth = (i: number) => ({ "--i": i }) as CSSProperties;

type Tone = "card" | "background" | "tide";

const TONES: Record<Tone, { section: string; lead: string }> = {
  card: { section: "bg-card", lead: LEAD },
  background: { section: "bg-background", lead: LEAD },
  tide: { section: "bg-tide text-tide-foreground", lead: LEAD_ON_TIDE },
};

/** One part of the page: its words on one side and the app on the other, swapped by `flip`. */
function Feature({
  id,
  title,
  lead,
  after,
  flip = false,
  tone = "card",
  children,
}: {
  id: string;
  title: ReactNode;
  lead: string;
  after?: ReactNode;
  flip?: boolean;
  tone?: Tone;
  children: ReactNode;
}) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className={cn("scroll-mt-14", TONES[tone].section)} data-slot="part" data-tone={tone}>
      <div className={cn(WRAP, "grid items-center gap-x-12 gap-y-24 py-24 sm:py-32 lg:grid-cols-12 lg:gap-16")}>
        <div className={cn("grid content-start gap-6 lg:col-span-5", flip && "lg:order-2")}>
          <h2 id={`${id}-title`} className={cn(styles.display, DISPLAY_MD)} data-reveal="">
            {title}
          </h2>
          <p className={TONES[tone].lead} style={nth(1)} data-reveal="">
            {lead}
          </p>
          {after && (
            <div style={nth(2)} data-reveal="">
              {after}
            </div>
          )}
        </div>
        <div className={cn("lg:col-span-7", flip && "lg:order-1")} data-reveal="tilt">
          {children}
        </div>
      </div>
    </section>
  );
}

/**
 * The page under the desktop (DEC-907, DEC-932): the desktop fills the first screen, and this rises
 * over it as the visitor scrolls. It shows the app itself, in pictures of the example workspace, and
 * sells what the desktop's homepage explains (DEC-908): an opening over the agent's page on a pixel
 * sea, then three parts (limits in dollars, the big calls that come to you, the reason behind every
 * move), what no agent does, and the request for a place. Each piece settles on a spring as it comes
 * into view; tide, the page's third colour beside ink and sun, fills only the part about asking you.
 */
export function LongPage() {
  return (
    <div className={cn(styles.page, "relative z-10 bg-card text-foreground")} style={SPRING_VARS as CSSProperties} data-slot="long-page">
      <ScrollBar />

      <section id={INTRO_ID} aria-labelledby="intro-title" className="scroll-mt-14 bg-card" data-slot="intro">
        <div className={cn(WRAP, "grid justify-items-center gap-7 pt-24 text-center sm:pt-32")}>
          <h2 id="intro-title" className={cn(styles.display, DISPLAY)}>
            <span className="block" data-reveal="">
              Hire an agent.
            </span>
            <i className="block" style={nth(1)} data-reveal="">
              Keep the keys.
            </i>
          </h2>
          <p className={cn(LEAD, "mx-auto")} style={nth(2)} data-reveal="">
            It trades in your own brokerage account, inside limits you set. Your money never leaves it.
          </p>
          <p className="flex flex-wrap justify-center gap-3" style={nth(3)} data-reveal="">
            <a href={`#${SIGN_UP_ID}`} className={PRIMARY}>
              Join the beta
            </a>
            <Link href={SIGN_IN_HREF} className={SECONDARY}>
              Sign in
            </Link>
          </p>
          <p className="-mt-2 text-[0.9375rem] text-muted-foreground" style={nth(4)} data-reveal="" data-slot="reassure">
            Free during the beta. Every agent starts on paper money.
          </p>
        </div>
        <div className="relative">
          <PixelSea className="absolute inset-x-0 top-14 bottom-0 sm:top-16" />
          <div className={cn(WRAP, "pt-24 pb-16 sm:pt-28 sm:pb-24")} data-reveal="tilt">
            <Shot
              name="agent"
              alt="Agent 1's page in Owlhead: equity of $10,123.45 on paper, its chart, and its limits in dollars beside it."
              sizes="(min-width: 80rem) 74rem, 100vw"
              className="mx-auto max-w-[74rem]"
            />
          </div>
        </div>
      </section>

      <Feature
        id="limits"
        tone="background"
        title={
          <>
            Set the limits. <i>In dollars.</i>
          </>
        }
        lead="Decide what it can spend and where it steps back. Every order is checked against your limits before it's sent, and if it doesn't fit, it doesn't go. You can tighten them any time; only you can loosen them."
        after={<p className="max-w-[36rem] text-[0.9375rem] leading-[1.55] text-pretty text-muted-foreground">{GAP_CAVEAT}</p>}
      >
        <Watcher side="left" className="mx-auto w-full max-w-[596px]">
          <Shot name="ladder" alt="Agent 1's limits as a ladder in dollars, from $10,150.00 down to a floor of $9,000.00, with what the agent does at each." />
        </Watcher>
      </Feature>

      <Feature
        id="asking"
        flip
        tone="tide"
        title={
          <>
            Big calls <i>wait for you.</i>
          </>
        }
        lead="Above an amount you choose, it asks first. Approve it from your phone in a tap. Ignore it, and nothing happens."
      >
        <div className="flex items-center justify-center gap-4 sm:gap-8">
          <LockScreen className="max-sm:hidden" />
          <div className={cn(styles.phone, "w-[16.5rem] shrink-0")}>
            <Shot name="request" frame={false} alt="An approval request on a phone: buy 0.015 BTC/USD at a limit of $56,700.00, with Approve and Skip." sizes="17rem" />
          </div>
        </div>
      </Feature>

      <Feature
        id="threads"
        tone="background"
        title={
          <>
            Every move, <i>explained.</i>
          </>
        }
        lead="Each decision is written down with its reason the moment it's made. Ask your agent about any of them in plain words."
      >
        <Watcher side="right" className="mx-auto w-full max-w-[607px]">
          <Shot name="ask" alt="Agent 2 asks to buy 2 XYZ at a limit of $141.30, with why it asked, and questions to ask it next, such as Why did it ask?" />
        </Watcher>
      </Feature>

      <Wont />

      <section id={SIGN_UP_ID} aria-labelledby={`${SIGN_UP_ID}-title`} className="scroll-mt-14 bg-highlight text-highlight-foreground" data-slot="sign-up">
        <div className={cn(WRAP, "grid items-center gap-16 py-24 sm:py-32 lg:grid-cols-12")}>
          <div className="grid content-start gap-6 lg:col-span-6">
            <h2 id={`${SIGN_UP_ID}-title`} className={cn(styles.display, DISPLAY)} data-reveal="">
              Put an agent <i>to work.</i>
            </h2>
            <p className="max-w-[30rem] text-[1.125rem] leading-[1.6] text-pretty sm:text-[1.25rem]" style={nth(1)} data-reveal="">
              We&apos;re letting people in a few at a time. It&apos;s free during the beta, and every agent starts on paper money.
            </p>
          </div>
          <div className="lg:col-span-6" style={nth(2)} data-reveal="">
            <Watcher side="right">
              <div className="grid gap-6 rounded-3xl bg-card p-7 text-foreground ring-1 ring-foreground sm:p-10">
                <BetaForm id={PAGE_FORM_ID} look="page" />
              </div>
            </Watcher>
          </div>
        </div>
      </section>

      <footer className="bg-card" data-slot="long-page-footer">
        <div className={cn(WRAP, "grid gap-8 py-14 sm:grid-cols-[auto_minmax(0,1fr)] sm:items-start sm:gap-16")}>
          <a href="#desktop" className="flex items-center gap-2 rounded-md outline-none focus-visible:ring-3 focus-visible:ring-ring" style={{ color: "var(--logo)" }}>
            <span className="sr-only">Owlhead, back to the desktop</span>
            <BrandOwl className="size-8" />
            <OwlheadWordmark title="" className="h-6 w-auto" />
          </a>
          <div className="grid gap-3 text-[0.9375rem] leading-[1.55] text-muted-foreground sm:justify-items-end sm:text-end">
            <p className="max-w-[36rem] text-pretty">Owlhead is in private beta. It is software, not investment advice. Trading involves risk, and you can lose money.</p>
            <p className="flex flex-wrap gap-x-4 gap-y-1">
              <span>© 2026 Owlhead</span>
              <Link href={SIGN_IN_HREF} className="underline decoration-1 underline-offset-2 outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring">
                Sign in
              </Link>
              <a href="#desktop" className="underline decoration-1 underline-offset-2 outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring">
                Back to the desktop
              </a>
            </p>
          </div>
        </div>
      </footer>

      <Reveals />
      <Ambient />
    </div>
  );
}
