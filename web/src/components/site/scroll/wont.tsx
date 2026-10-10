import type { CSSProperties } from "react";
import { cn } from "@/lib/utils";
import { WRAP } from "./parts";
import { perchProps } from "./shot";
import styles from "./scroll.module.css";

/** What no agent does in v1, whatever its rules say (PRD §3, FR-5.12), each with the line that holds it. */
export const WONT: { verb: string; why: string }[] = [
  { verb: "borrow", why: "It trades at one times buying power. No margin loans." },
  { verb: "sell short", why: "It only sells what it holds." },
  { verb: "trade options", why: "US stocks, ETFs and crypto, nothing with an expiry." },
  { verb: "trade overnight", why: "Stock orders go in during the regular session." },
  { verb: "chase a price", why: "It opens every position with a limit order." },
];

/** The won't list, on a field of sun under ink type, each verb struck through as its line comes into view. */
export function Wont() {
  return (
    <section id="wont" aria-labelledby="wont-title" className="scroll-mt-14 bg-highlight text-highlight-foreground" data-slot="wont">
      <div className={cn(WRAP, "grid gap-10 py-24 sm:py-32 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16")}>
        <h2
          id="wont-title"
          className={cn(styles.display, "text-[clamp(3rem,1.6rem+5.5vw,7rem)] leading-[0.95] font-normal tracking-[-0.025em] lg:sticky lg:top-40 lg:self-start")}
          {...perchProps({ at: 0.92, yaw: 0.35 })}
        >
          <i>It won&apos;t</i>
        </h2>
        <ul className="grid">
          {WONT.map((w, i) => (
            <li key={w.verb} style={{ "--i": i } as CSSProperties} data-reveal="strike" className="grid gap-2 border-t border-current/25 py-6 first:border-t-0 first:pt-0 sm:grid-cols-[minmax(0,1fr)_16rem] sm:items-baseline sm:gap-8">
              <span className={cn(styles.display, "text-[clamp(2rem,1.3rem+2.4vw,3.5rem)] leading-none tracking-[-0.02em] whitespace-nowrap")}>
                <s className={cn(styles.struck, "no-underline")}>{w.verb}</s>
              </span>
              <span className="text-[1.0625rem] leading-[1.5] text-pretty">{w.why}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
