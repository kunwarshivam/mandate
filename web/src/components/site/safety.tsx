import type { ReactNode } from "react";
import { NotYet, SiteSection } from "./parts";

export const AGENT_CANNOT = [
  "Change its own limits.",
  "Send an order your mandate doesn't allow.",
  "Reach your broker except through the gate.",
  "Move money in or out of your account.",
];

export const YOU_CAN_ALWAYS = [
  "Pause one agent or every agent.",
  "Use the kill switch, even when the model is down.",
  "Approve or skip what an agent asks.",
  "See why each order was allowed or blocked.",
];

type Fact = { term: string; detail: ReactNode };

export const FACTS: Fact[] = [
  {
    term: "Plain code checks every order",
    detail:
      "The language models behind an agent suggest trades. They never place orders. Deterministic code sizes each order, and the gate checks it against your mandate and US account rules before it's sent.",
  },
  {
    term: "A journal that shows any change",
    detail: (
      <>
        <span className="block">
          Every order is written to an append-only, hash-chained journal before it&apos;s sent, so a changed or missing entry shows. You can read every gate decision today.
        </span>
        <span className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
          <span>Export and a chain check aren&apos;t built yet.</span>
          <NotYet />
        </span>
      </>
    ),
  },
  {
    term: "Paper first",
    detail: "Owlhead runs on broker paper accounts with simulated funds. Live trading isn't available, and there's no date for it.",
  },
  {
    term: "Your broker holds your money",
    detail: "Owlhead never holds your funds and can't move them. It connects with permission to trade only, and turns down keys that could withdraw or transfer.",
  },
  {
    term: "Nothing shown before it's true",
    detail: "An order reads as submitted only once it's in the journal. An order whose state isn't clear reads as unknown, and old data shows its age.",
  },
];

function Ledger({ id, title, items }: { id: string; title: string; items: string[] }) {
  return (
    <div className="grid content-start gap-2">
      <h3 id={id} className="field-label">
        {title}
      </h3>
      <ul aria-labelledby={id} className="grid">
        {items.map((item) => (
          <li key={item} className="border-b border-border/70 py-3.5 text-lg text-pretty">
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function Safety() {
  return (
    <SiteSection id="safety" title="Why it's safe" lead="No software can promise you won't lose money. These are the checks that keep an agent inside your limits.">
      <div className="grid gap-10 sm:grid-cols-2 sm:gap-14">
        <Ledger id="agent-cannot" title="The agent can't" items={AGENT_CANNOT} />
        <Ledger id="you-can" title="You can always" items={YOU_CAN_ALWAYS} />
      </div>
      <dl className="grid">
        {FACTS.map((fact) => (
          <div key={fact.term} className="grid gap-1.5 border-t border-border/70 py-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-14">
            <dt className="text-h3">{fact.term}</dt>
            <dd className="max-w-measure text-pretty text-muted-foreground">{fact.detail}</dd>
          </div>
        ))}
      </dl>
    </SiteSection>
  );
}
