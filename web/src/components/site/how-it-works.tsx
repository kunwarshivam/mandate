import type { ReactNode } from "react";
import { PhoneFrame } from "./hero";
import { SiteSection } from "./parts";
import { Screenshot } from "./screenshot";

type Step = { title: string; body: ReactNode; screens: ReactNode };

const FRAME = "overflow-hidden rounded-xl border border-border/70";

export const STEPS: Step[] = [
  {
    title: "Set the mandate",
    body: (
      <>
        <p>Choose what the agent may trade. Set its largest order, its largest position, and a daily loss limit, all in dollars.</p>
        <p>Nothing runs until you confirm it.</p>
      </>
    ),
    screens: (
      <div className="w-[min(20rem,100%)]">
        <Screenshot name="step-mandate" sizes="20rem" className="rounded-2xl" />
      </div>
    ),
  },
  {
    title: "The agent trades inside it",
    body: (
      <>
        <p>Every order goes through the gate before it reaches your broker. The gate is plain code that runs apart from the agent.</p>
        <p>An order inside your mandate goes through. One outside it is blocked. One your rules flag waits for you.</p>
      </>
    ),
    screens: (
      <div className={`${FRAME} w-[min(36rem,100%)] p-4 sm:p-6`}>
        <div className="sm:hidden">
          <Screenshot name="step-gate-phone" sizes="(min-width: 40rem) 1px, 90vw" />
        </div>
        <div className="hidden sm:block">
          <Screenshot name="step-gate" sizes="(min-width: 40rem) 33rem, 1px" />
        </div>
      </div>
    ),
  },
  {
    title: "You stay in charge",
    body: (
      <>
        <p>Approve or skip what the agent asks. If you don&apos;t answer in time, the order is skipped.</p>
        <p>Stop is on every screen. Two taps pause one agent or every agent. The kill switch cancels an agent&apos;s orders, sells its positions, and ends it.</p>
      </>
    ),
    screens: (
      <div className="flex items-start gap-5">
        <PhoneFrame className="w-[min(16rem,100%)] shrink-0">
          <Screenshot name="step-approve" sizes="16rem" />
        </PhoneFrame>
        <div className={`${FRAME} hidden w-[17rem] shrink-0 sm:block`}>
          <Screenshot name="step-stop" sizes="17rem" />
        </div>
      </div>
    ),
  },
];

export function HowItWorks() {
  return (
    <SiteSection id="how-it-works" title="How it works" lead="You write the limits and the agent works inside them. You can step in at any point.">
      <ol className="grid">
        {STEPS.map((step, i) => (
          <li
            key={step.title}
            className="grid gap-8 border-t border-border/70 py-12 first:border-t-0 first:pt-0 last:pb-0 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-14 lg:py-16"
          >
            <div className="grid max-w-measure content-start gap-3">
              <h3 className="flex items-baseline gap-3 text-h2">
                <span aria-hidden className="tabular text-mandate-strong">
                  {i + 1}
                </span>
                {step.title}
              </h3>
              <div className="grid gap-3 text-pretty text-muted-foreground">{step.body}</div>
            </div>
            <div className="min-w-0">{step.screens}</div>
          </li>
        ))}
      </ol>
    </SiteSection>
  );
}
