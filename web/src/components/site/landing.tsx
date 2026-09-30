import "@fontsource/dotgothic16/400.css";
import "@fontsource/vt323/400.css";
import "@fontsource-variable/pixelify-sans";
import type { ReactNode } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { OWLHEAD_ASCII } from "./ascii";
import { BetaForm } from "./beta-form";
import { Contents } from "./contents";
import { BODY, BOLD, BUTTON, H2, LINK, MONO, PIXEL, PLAIN_BUTTON, RAISED, RULE } from "./letter";
import { RecordTrace } from "./record-trace";
import { Blink, Browser, Desktop, UnderConstruction, Window } from "./retro";
import { SiteFooter } from "./site-footer";

export const HEADLINE = "Owlhead";
export const SUBHEAD = "A trading agent for your own brokerage account. It works inside rules you write, and it writes down every decision it makes.";
export const UPDATED = "30 September 2026";

type Section = { id: string; title: string; body: ReactNode };

const B = BOLD;

export const SECTIONS: Section[] = [
  {
    id: "what",
    title: "What we're building",
    body: (
      <>
        <p>
          Owlhead is a trading agent that runs in your own brokerage account. It reads filings, news and prices, comes up with its own trade ideas, places the orders, and looks after each position until it closes.
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
        <p>Agents are going to handle more and more of people&apos;s money. We think the people whose money it is should stay in charge of them.</p>
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
          <strong className={B}>It places orders.</strong> Every order is sized and checked against your rules before it&apos;s sent. If your rules say to ask, the order waits on your phone until you approve it.
        </li>
        <li>
          <strong className={B}>It looks after the position.</strong> Every trade has an exit plan from the start: a stop, a target and a time limit.
        </li>
      </ol>
    ),
  },
  {
    id: "safety",
    title: "How it stays in check",
    body: (
      <>
        <p>An agent that trades on its own should earn trust a step at a time. Every idea takes the same path to real money:</p>
        <ol className="grid list-decimal gap-3 ps-6">
          <li>
            <strong className={B}>Backtest.</strong> It runs on past prices first. Orders that would break your limits are blocked there too.
          </li>
          <li>
            <strong className={B}>Paper.</strong> Then it trades with simulated money, and every trade is scored. An idea gets three tries, then it&apos;s retired.
          </li>
          <li>
            <strong className={B}>Live.</strong> Real money only when you switch it on, with your passkey.
          </li>
        </ol>
        <p>And while it trades:</p>
        <ul className="grid list-disc gap-3 ps-6">
          <li>As losses reach levels you set, it trades smaller, then only sells, then closes out and pauses.</li>
          <li>Orders above a size you choose wait for your approval, or a second person&apos;s.</li>
          <li>One Stop button halts every agent and cancels their open orders.</li>
          <li>If market data goes stale, or Owlhead&apos;s records and your broker&apos;s disagree, it stops adding risk and waits for you.</li>
        </ul>
        <p>
          Limits can&apos;t prevent every loss. When a market gaps or halts, prices can move past a limit before an order fills, so a loss can end up larger than the limit. Owlhead tells you this, in dollars, before you confirm your
          rules.
        </p>
      </>
    ),
  },
  {
    id: "record",
    title: "The record",
    body: (
      <>
        <p>
          Before any order goes out, Owlhead writes down what the agent read, what it concluded, which checks passed and who approved it. Each line carries a hash of the line before it, so if anyone edits a line, the chain stops
          matching from there on. You can export it for an auditor or your investors.
        </p>
        <p>Here is one decision from start to finish. Try editing a line.</p>
        <RecordTrace />
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
          ["You trade your own account.", "The agent does the watching, and asks you only what you've told it to."],
          ["You manage money for others.", "Require a second sign-off above a size you choose. When an investor asks why a trade happened, send them the record."],
          ["You run a desk.", "Set firm limits that each team can tighten but never loosen. Run it in your own cloud, so strategy and keys stay with you."],
          ["You've built your own agent.", "Connect it through Owlhead's MCP server. Its orders get the same checks, approvals and record, and it can't approve itself."],
        ].map(([who, what]) => (
          <div key={who}>
            <dt className={B}>{who}</dt>
            <dd>{what}</dd>
          </div>
        ))}
      </dl>
      <p className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <a href="#beta" className={BUTTON}>
          Sign the guestbook
        </a>
        <span>and tell us which of these you are.</span>
      </p>
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
          <li>The record</li>
        </ul>
        <UnderConstruction />
        <p className={B}>Coming during the beta:</p>
        <ul className="grid list-disc gap-1.5 ps-6">
          <li>Live trading, once it has legal sign-off</li>
          <li>More brokers, starting with Robinhood and Kraken Derivatives US</li>
          <li>Firm and team limits, and running Owlhead in your own cloud</li>
          <li>Connecting your own agent through the MCP server</li>
        </ul>
      </>
    ),
  },
];

export const QUESTIONS: { q: string; a: string }[] = [
  { q: "Can it take money out of my account?", a: "No. Owlhead only asks your broker for permission to trade. It can't withdraw or transfer money, and it can't change its own rules." },
  {
    q: "Is it trading real money?",
    a: "Not yet. In the beta, agents trade on paper with simulated money. Live trading comes later, once it has legal sign-off, and only when you switch it on with your passkey.",
  },
  { q: "Which brokers does it work with?", a: "Alpaca first. Robinhood and Kraken Derivatives US are planned." },
  { q: "How do I stop it?", a: "Press Stop. It halts every agent and cancels their open orders. Protective stops rest at your broker, so they hold even if Owlhead goes down." },
  { q: "What does it cost?", a: "Nothing during the private beta. We'll tell you the price well before we charge anything." },
  {
    q: "Is this investment advice?",
    a: "No. Owlhead is software that carries out rules you write. It doesn't know your finances and doesn't recommend what to buy or sell. Whether trading suits you is your decision, ideally with an adviser.",
  },
  { q: "Who gets in?", a: "We let people in a few at a time, roughly in the order they ask, with room for each kind of user so we hear from all of them." },
];

const CONTENTS = [...SECTIONS.map(({ id, title }) => ({ id, title })), { id: "questions", title: "Questions" }, { id: "beta", title: "Ask for a place" }];

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
 * looked in the late 1990s, that says what Owlhead is, why and how, and asks for an email. It brings
 * its own `<main>` and footer.
 */
export function Landing() {
  return (
    <Desktop className="flex-1">
      <div id="top" className="mx-auto w-full max-w-[68rem] px-1.5 py-3 sm:px-6 sm:py-8" data-slot="landing-page">
        <Browser address="http://www.owlhead.ai/">
          <div className={cn("text-[1.125rem] leading-[1.65]", BODY)}>
          <header className="grid justify-items-center gap-4 px-4 pt-8 pb-2 text-center sm:px-8 sm:pt-12">
            <h1>
              <span className="sr-only">{HEADLINE}</span>
              <span aria-hidden className={cn(MONO, "block text-start text-[clamp(10px,4vw,28px)] leading-[0.95] whitespace-pre text-foreground")}>
                {OWLHEAD_ASCII}
              </span>
            </h1>
            <p className="max-w-[34rem] pt-2 text-[1.3125rem] leading-snug text-balance">{SUBHEAD}</p>
            <p className="flex flex-wrap items-center justify-center gap-x-2 gap-y-1">
              <span className={cn(RAISED, "bg-highlight px-1.5 text-sm tracking-wide text-highlight-foreground uppercase", PIXEL)}>
                <Blink>New</Blink>
              </span>
              <span>Private beta, opening a few people at a time.</span>
            </p>
            <p className="flex flex-wrap items-center justify-center gap-2" data-slot="hero-actions">
              <a href="#beta" className={BUTTON}>
                Sign the guestbook
              </a>
              <Link href="/login" className={cn(PLAIN_BUTTON, "h-9")}>
                Sign in
              </Link>
            </p>
            <p className="text-[0.9375rem] text-muted-foreground">Last updated {UPDATED}.</p>
          </header>

          <hr className={cn(RULE, "mx-4 sm:mx-8")} />

          <div className="grid gap-8 px-4 pb-10 sm:px-8 lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12">
            <aside>
              <Contents items={CONTENTS} />
            </aside>

            <main id="main" tabIndex={-1} data-slot="landing" className="min-w-0 outline-none">
              {SECTIONS.map((s, i) => (
                <Section key={s.id} id={s.id} n={i + 1} title={s.title}>
                  {s.body}
                </Section>
              ))}

              <Section id="questions" n={SECTIONS.length + 1} title="Questions">
                <dl className="grid gap-5">
                  {QUESTIONS.map(({ q, a }) => (
                    <div key={q}>
                      <dt className={B}>{q}</dt>
                      <dd>{a}</dd>
                    </div>
                  ))}
                </dl>
              </Section>

              <Section id="beta" n={SECTIONS.length + 2} title="Ask for a place">
                <p>Sign the guestbook to ask for a place. Leave your email and we&apos;ll write once, when your place opens.</p>
                <Window title="guestbook.cgi" className="max-w-[30rem]">
                  <BetaForm />
                </Window>
              </Section>
            </main>
          </div>

          <SiteFooter />
          </div>
        </Browser>
      </div>
    </Desktop>
  );
}
