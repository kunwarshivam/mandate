import "./faces";
import type { ReactNode } from "react";
import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";
import { BetaForm } from "./beta-form";
import { Contents } from "./contents";
import { Desktop } from "./desktop";
import type { AppId } from "./windows";
import { BODY, BOLD, BUTTON, H2, LINK, PIXEL, PLAIN_BUTTON, RAISED, RULE } from "./letter";
import { OpenApp } from "./open-app";
import { ModeChart, Perch, TitleOwl } from "./owls";
import { Blink, Browser, UnderConstruction, Window } from "./retro";
import { SiteFooter } from "./site-footer";

export const HEADLINE = "Owlhead";
export const SUBHEAD = "A trading agent for your own brokerage account. It does the reading and the watching, trades only inside rules you write, and writes down why it placed every order.";

type Section = { id: string; title: string; body: ReactNode };

const B = BOLD;

export const SECTIONS: Section[] = [
  {
    id: "what",
    title: "What we're building",
    body: (
      <>
        <p>
          Owlhead is a trading agent that works in your own brokerage account. It reads filings, news and prices, comes up with its own trade ideas, places the orders, and looks after each position until it closes.
        </p>
        <p>It works inside rules you write in plain English, which we call your mandate. It can&apos;t change those rules, and it can&apos;t take money out of your account.</p>
      </>
    ),
  },
  {
    id: "why",
    title: "Why",
    body: (
      <>
        <p>
          Most people who trade can&apos;t watch the market all day. The software that can is usually one of two things: a fund you hand your money to and can&apos;t see inside, or a bot you program yourself and hope you got
          right.
        </p>
        <p>
          We want a third option. The agent does the reading and the watching. You decide how much it can spend, what it can buy and when it has to ask you. Every decision is written down before it acts, so you can always see
          why an order went out and who said yes.
        </p>
        <p>Agents are going to handle more and more of people&apos;s money. We think the people whose money it is should stay in charge of the agents.</p>
      </>
    ),
  },
  {
    id: "how",
    title: "How it works",
    body: (
      <ol className="grid list-decimal gap-3 ps-6">
        <li>
          <strong className={B}>You write its rules.</strong> For example: &ldquo;US stocks only. No more than $2,000 on any one order. Ask me before buying anything you haven&apos;t held.&rdquo; Owlhead shows you what it
          understood, in dollars, before you confirm.
        </li>
        <li>
          <strong className={B}>It looks for ideas.</strong> It writes each one down with its reason and what would prove it wrong.
        </li>
        <li>
          <strong className={B}>It places orders.</strong> Every order is sized and checked against your rules before it&apos;s sent. If your rules say to ask, the order waits on your phone for you. If you don&apos;t
          answer in time, it&apos;s skipped.
        </li>
        <li>
          <strong className={B}>It looks after the position.</strong> Each idea comes with a time limit and the signs that would prove it wrong. Unless you turn protection off, each position also gets a stop that rests at your
          broker.
        </li>
      </ol>
    ),
  },
  {
    id: "safety",
    title: "How it stays in check",
    body: (
      <>
        <p>An agent that trades on its own should earn trust a step at a time. Every agent takes the same path to real money:</p>
        <ol className="grid list-decimal gap-3 ps-6">
          <li>
            <strong className={B}>Backtest.</strong> Your rules run on past prices first, to check that orders are sized right and that anything breaking your limits is blocked.
          </li>
          <li>
            <strong className={B}>Paper.</strong> Then it trades with simulated money, and each idea is scored when its time is up. An idea that keeps failing is retired.
          </li>
          <li>
            <strong className={B}>Live.</strong> Real money only after both, and only when you switch it on, with your passkey.
          </li>
        </ol>
        <p>And while it trades:</p>
        <ul className="grid list-disc gap-3 ps-6">
          <li>As losses reach levels you set, it trades smaller, then only sells, then closes out and pauses.</li>
          <li>Orders above a size you choose wait for your approval, or a second person&apos;s.</li>
          <li>Stop is on every screen. Pause one agent, or stop them all: each one cancels its orders, sells what it holds and ends.</li>
          <li>If market data goes stale, or Owlhead&apos;s records and your broker&apos;s disagree, it stops adding risk and waits for you.</li>
        </ul>
        <ModeChart />
        <p>
          Limits can&apos;t prevent every loss. When a market gaps or halts, prices can move past a limit before an order fills, so a loss can end up larger than the limit. Owlhead tells you this, in dollars, before you confirm your
          rules.
        </p>
      </>
    ),
  },
  {
    id: "who",
    title: "Who it's for",
    body: (
      <>
      <dl className="grid gap-4">
        {[
          ["You trade your own account.", "The agent does the watching, and asks you only when your rules say to."],
          ["You manage money for others.", "Require a second sign-off above a size you choose. When an investor asks why a trade happened, send them the record."],
          ["You run a desk.", "Set firm limits that each team can tighten but never loosen, and run Owlhead in your own cloud, so strategy and keys stay with you. Both are coming during the beta."],
          ["You've built your own agent.", "Connect it through Owlhead's MCP server, coming during the beta. Its orders get the same checks, approvals and record, and it can't approve itself."],
        ].map(([who, what]) => (
          <div key={who}>
            <dt className={B}>{who}</dt>
            <dd>{what}</dd>
          </div>
        ))}
      </dl>
      <p className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <OpenApp app="guestbook" className={BUTTON}>
          Sign the guestbook
        </OpenApp>
        <span>and tell us which of these you are.</span>
      </p>
      {/* The guestbook opens in its own window, which needs a script; without one, the form is here. */}
      <noscript>
        <Window title="guestbook.cgi" icon={<TitleOwl />} className="max-w-[30rem]">
          <BetaForm id="beta-noscript" />
        </Window>
      </noscript>
      </>
    ),
  },
  {
    id: "status",
    title: "Where we are",
    body: (
      <>
        <p>Owlhead is in private beta. Here is what works today and what we&apos;re building while the beta opens up.</p>
        <p className={B}>Working now, on paper:</p>
        <ul className="grid list-disc gap-1.5 ps-6">
          <li>Writing your rules, and checking every order against them</li>
          <li>Paper trading on Alpaca, with simulated money</li>
          <li>Approvals on your phone, and the Stop button</li>
          <li>The record of every decision</li>
        </ul>
        <UnderConstruction />
        <p className={B}>Coming during the beta:</p>
        <ul className="grid list-disc gap-1.5 ps-6">
          <li>Agents that bring their own trade ideas. We&apos;re scoring the ideas on our own paper account first</li>
          <li>Live trading, once it has legal sign-off</li>
          <li>More brokers, starting with Robinhood and Kraken Derivatives US</li>
          <li>Firm and team limits, and running Owlhead in your own cloud</li>
          <li>Connecting your own agent through the MCP server</li>
        </ul>
      </>
    ),
  },
];

const CONTENTS = SECTIONS.map(({ id, title }) => ({ id, title }));

/** The parts of the site that open in their own window on the desktop, as their icons do. */
export const WINDOWS: { app: AppId; title: string }[] = [
  { app: "record", title: "The record" },
  { app: "questions", title: "Questions" },
  { app: "guestbook", title: "Guestbook" },
];

function Section({ id, n, title, children }: { id: string; n: number; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={id}>
      {n > 1 && <hr className={RULE} />}
      <div className="flex items-baseline justify-between gap-4 pb-4">
        <h2 id={id} className={H2}>
          {n}. {title}
        </h2>
        <a href="#top" className={cn(LINK, "shrink-0 text-sm")}>
          [top]
        </a>
      </div>
      <div className="grid max-w-[42rem] gap-4 text-pretty">{children}</div>
    </section>
  );
}

/**
 * The landing page at owlhead.ai (DEC-212), for signed-out visitors: one homepage, set as the web
 * looked in the late 1990s and open in a browser window on a desktop of the time, that says what
 * Owlhead is, why and how. The record, the questions and the guestbook that asks for an email each
 * open in their own window on the desktop. It brings its own `<main>` and footer.
 */
export function Landing() {
  return (
    <Desktop
      home={
        <Browser address="http://www.owlhead.ai/" bookmarks={CONTENTS}>
          <div id="top" className={cn("text-[1.125rem] leading-[1.65]", BODY)} data-slot="landing-page">
          <header className="grid justify-items-center gap-4 px-4 pt-8 pb-2 text-center sm:px-8 sm:pt-12">
            <h1>
              <span className="sr-only">{HEADLINE}</span>
              <span aria-hidden className="flex items-center gap-3 sm:gap-4" style={{ color: "var(--logo)" }} data-slot="wordmark">
                <BrandOwl still className="size-14 sm:size-16" />
                <OwlheadWordmark title="" className="h-10 w-auto sm:h-12" />
              </span>
            </h1>
            <Perch />
            <p className="max-w-[34rem] pt-2 text-[1.3125rem] leading-snug text-balance">{SUBHEAD}</p>
            <p className="flex flex-wrap items-center justify-center gap-x-2 gap-y-1">
              <span className={cn(RAISED, "bg-highlight px-1.5 text-sm tracking-wide text-highlight-foreground uppercase", PIXEL)}>
                <Blink>New</Blink>
              </span>
              <span>Private beta, opening a few people at a time. Sign the guestbook to ask for a place.</span>
            </p>
            <p className="flex flex-wrap items-center justify-center gap-2" data-slot="hero-actions">
              <OpenApp app="guestbook" className={BUTTON}>
                Sign the guestbook
              </OpenApp>
              <OpenApp app="record" className={BUTTON}>
                See why it traded
              </OpenApp>
              <Link href="/login" className={cn(PLAIN_BUTTON, "h-9")}>
                Sign in
              </Link>
            </p>
          </header>

          <hr className={cn(RULE, "mx-4 sm:mx-8")} />

          <div className="grid gap-8 px-4 pb-10 sm:px-8 lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12">
            <aside>
              <Contents items={CONTENTS} windows={WINDOWS} />
            </aside>

            <main id="main" tabIndex={-1} data-slot="landing" className="min-w-0 outline-none">
              {SECTIONS.map((s, i) => (
                <Section key={s.id} id={s.id} n={i + 1} title={s.title}>
                  {s.body}
                </Section>
              ))}
            </main>
          </div>

          <SiteFooter />
          </div>
        </Browser>
      }
    />
  );
}
