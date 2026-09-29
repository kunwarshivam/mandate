import type { ReactNode } from "react";
import { Placeholder } from "@/components/domain/placeholders";
import { SiteSection } from "./parts";

type Question = { q: string; a: ReactNode };

export const QUESTIONS: Question[] = [
  {
    q: "Is this real money?",
    a: (
      <p>
        No. Owlhead runs on broker paper accounts with simulated funds. Live trading isn&apos;t available. It needs legal sign-off first, and there&apos;s no date for it.
      </p>
    ),
  },
  {
    q: "Which brokers does it work with?",
    a: (
      <p>
        Alpaca, on paper, today. Robinhood and Kraken Derivatives US are planned and not available yet. Robinhood starts as a simulated broker that follows Robinhood&apos;s rules.
      </p>
    ),
  },
  {
    q: "Can the agent go past my limits?",
    a: (
      <>
        <p>It can&apos;t send an order your mandate doesn&apos;t allow. The gate blocks it, or holds it for your approval when your rules say so.</p>
        <p>Prices can still move past a limit before an order fills, when a market gaps, halts, or goes down. So a loss can end up larger than the limit. The mandate says this, in dollars, before you confirm it.</p>
      </>
    ),
  },
  {
    q: "What happens if something breaks?",
    a: (
      <>
        <p>
          Owlhead stops adding risk instead of guessing. If market data goes stale, the agent opens nothing new in that instrument. If Owlhead&apos;s records and the broker&apos;s disagree, the agent pauses until you look.
        </p>
        <p>
          After a restart, Owlhead replays the journal and checks the broker before it trades again. Protective stops rest at the broker, so they don&apos;t depend on Owlhead staying up. If Stop can&apos;t reach an agent, it says so and tells
          you how to reach your broker directly.
        </p>
      </>
    ),
  },
  {
    q: "Is this investment advice?",
    a: (
      <p>
        <Placeholder name="notAdvice" />
      </p>
    ),
  },
];

export function Faq() {
  return (
    <SiteSection id="faq" title="Questions">
      <dl className="grid">
        {QUESTIONS.map(({ q, a }) => (
          <div key={q} className="grid gap-3 border-t border-border/70 py-8 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-14">
            <dt className="text-h3">{q}</dt>
            <dd className="grid max-w-measure gap-3 text-pretty text-muted-foreground">{a}</dd>
          </div>
        ))}
      </dl>
    </SiteSection>
  );
}
