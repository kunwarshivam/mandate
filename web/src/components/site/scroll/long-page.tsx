import type { CSSProperties, ReactNode } from "react";
import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { cn } from "@/lib/utils";
import { artwork } from "../art";
import { BetaForm } from "../beta-form";
import { LockScreen } from "./lock-screen";
import { OwlFlight } from "./owl-flight";
import { DISPLAY, DISPLAY_MD, INTRO_ID, LEAD, LEAD_ON_TIDE, PAGE_FORM_ID, PRIMARY, SECONDARY, SIGN_UP_ID, WRAP } from "./parts";
import { Plate } from "./plate";
import { Reveals } from "./reveals";
import { ScrollBar } from "./scroll-bar";
import { Shot, perchProps } from "./shot";
import { SPRING_VARS } from "./spring";
import { Wont } from "./wont";
import styles from "./scroll.module.css";

/** Limits are never said to cap a loss without this beside them. */
export const GAP_CAVEAT =
  "Limits can't prevent every loss. When a market gaps or halts, prices can move past a limit before an order fills, so a loss can end up larger than the limit.";

export const BROKERS: { name: string; status: string }[] = [
  { name: "Alpaca", status: "Paper trading" },
  { name: "Robinhood", status: "Coming" },
  { name: "Kraken Derivatives US", status: "Coming" },
];

const HALF = "(min-width: 64rem) 46rem, 100vw";

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
 * The page under the desktop (DEC-907): the desktop fills the first screen, and this rises over it as
 * the visitor scrolls. It shows the app itself, in pictures of the example workspace, and says what
 * the desktop's homepage does not: the checks, the request on a phone, the threads, the limits in
 * dollars, what no agent does, and the request for a place. The brand owl flies down it in three
 * dimensions, and each piece settles on a spring as it comes into view; tide, the page's third
 * colour beside ink and sun, fills only the part about asking you.
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
            It trades in your own brokerage account, with permission to trade and nothing that can move money out of it.
          </p>
          <p className="flex flex-wrap justify-center gap-3" style={nth(3)} data-reveal="">
            <a href={`#${SIGN_UP_ID}`} className={PRIMARY}>
              Ask for a place
            </a>
            <Link href={LOGIN_PATH} className={SECONDARY}>
              Sign in
            </Link>
          </p>
        </div>
        <div className={cn(WRAP, "pt-24 sm:pt-28")} data-reveal="tilt">
          <Shot
            name="agent"
            alt="Agent 1's page in Owlhead: equity of $10,123.45 on paper, its chart, and its limits in dollars beside it."
            sizes="(min-width: 80rem) 74rem, 100vw"
            className="mx-auto max-w-[74rem]"
            perch={{ at: 0.86, yaw: -0.5 }}
          />
        </div>
        <div className={cn(WRAP, "py-16 sm:py-20")}>
          <dl className="flex flex-wrap items-baseline justify-center gap-x-12 gap-y-6" data-slot="brokers">
            {BROKERS.map((b, i) => (
              <div key={b.name} className="grid justify-items-center gap-1" style={nth(i)} data-reveal="">
                <dt className="text-[1.375rem] font-semibold tracking-[-0.02em] sm:text-[1.625rem]">{b.name}</dt>
                <dd className="text-[0.875rem] text-muted-foreground">{b.status}</dd>
              </div>
            ))}
          </dl>
        </div>
      </section>

      <Feature
        id="checks"
        tone="background"
        title="Every order clears the same checks, in the same order."
        lead="Can the account trade? Is the market open? Does the order fit your limits? Each answer is written down, the yeses too, and an order that fails one is never sent."
      >
        <Shot name="gate" alt="A gate decision: buying 0.02 BTC/USD was not allowed, with each check listed in the order it ran." sizes={HALF} perch={{ at: 0.12, yaw: 0.6 }} />
      </Feature>

      <Feature
        id="asking"
        flip
        tone="tide"
        title={
          <>
            Your phone says an agent needs you. <i>Nothing more.</i>
          </>
        }
        lead="The order, the rule that asked and the deadline open in the app. Your approval holds the size and the price, and the checks run again before it goes out."
      >
        <div className="flex items-center justify-center gap-4 sm:gap-8">
          <LockScreen className="max-sm:hidden" />
          <div className={cn(styles.phone, "w-[16.5rem] shrink-0")} {...perchProps({ at: 0.5, yaw: -0.3, coat: "pale" })}>
            <Shot name="request" frame={false} alt="An approval request on a phone: buy 0.015 BTC/USD at a limit of $56,700.00, with Approve and Skip." sizes="17rem" />
          </div>
        </div>
      </Feature>

      <Feature
        id="threads"
        tone="background"
        title="Every agent keeps a thread."
        lead="Its orders, its requests and every change to its mode, in the order they happened. Ask it about any of them, or ask Owlhead about all your agents at once."
      >
        <Shot name="thread" alt="Agent 2's thread: a request waiting, a new version of its rules applied, and an approved order the gate then held back." sizes={HALF} perch={{ at: 0.88, yaw: -0.6 }} />
      </Feature>

      <Feature
        id="limits"
        flip
        title="Limits in dollars, with the headroom beside each one."
        lead="Raising a limit asks for your passkey. Lowering one takes effect at the agent's next safe point."
        after={<p className="max-w-[36rem] text-[0.9375rem] leading-[1.55] text-pretty text-muted-foreground">{GAP_CAVEAT}</p>}
      >
        <Shot name="mandate" alt="Agent 1's limits: each level in dollars, what the agent does there, and the headroom left on each limit." sizes={HALF} perch={{ at: 0.1, yaw: 0.6 }} />
      </Feature>

      <Wont />

      <Plate id={SIGN_UP_ID} art={artwork("kanasawa-full-moon")} className={cn(WRAP, "grid justify-items-center py-28 sm:py-40")}>
        <div className="grid w-full max-w-[38rem] gap-6 rounded-3xl bg-card p-7 sm:p-10" data-reveal="" {...perchProps({ at: 0.86, yaw: -0.4 })}>
          <h2 id={`${SIGN_UP_ID}-title`} className={cn(styles.display, DISPLAY_MD)}>
            Ask for a place in the beta.
          </h2>
          <p className={LEAD}>We let people in a few at a time. It&apos;s free while the beta runs.</p>
          <BetaForm id={PAGE_FORM_ID} look="page" />
        </div>
      </Plate>

      <footer className="bg-card" data-slot="long-page-footer" {...perchProps({ at: 0.1, yaw: 0.4 })}>
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
              <Link href={LOGIN_PATH} className="underline decoration-1 underline-offset-2 outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring">
                Sign in
              </Link>
              <a href="#desktop" className="underline decoration-1 underline-offset-2 outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring">
                Back to the desktop
              </a>
            </p>
          </div>
        </div>
      </footer>

      <OwlFlight />
      <Reveals />
    </div>
  );
}
