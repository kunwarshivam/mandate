import type { CSSProperties } from "react";
import { cn } from "@/lib/utils";
import { DISPLAY_MD, WRAP } from "./parts";
import styles from "./scroll.module.css";

/** What no agent does in v1, whatever its rules say (PRD §3, FR-5.12), each as a promise to the owner. */
export const WONT: { verb: string; why: string }[] = [
  { verb: "Borrow", why: "It only uses money already in your account." },
  { verb: "Sell short", why: "It only sells what you own." },
  { verb: "Trade options", why: "Stocks, ETFs and crypto, nothing else." },
  { verb: "Chase a price", why: "Every order it places has a limit price." },
];

/** The won't list (DEC-932): a plain list of four lines, its heading beside it. */
export function Wont() {
  return (
    <section id="wont" aria-labelledby="wont-title" className="scroll-mt-14 bg-card" data-slot="wont">
      <div className={cn(WRAP, "grid gap-10 py-24 sm:py-32 lg:grid-cols-12 lg:gap-16")}>
        <h2 id="wont-title" className={cn(styles.display, DISPLAY_MD, "lg:col-span-5")} data-reveal="">
          What it <i>won&apos;t</i> do.
        </h2>
        <ul className="grid lg:col-span-7">
          {WONT.map((w, i) => (
            <li
              key={w.verb}
              style={{ "--i": i } as CSSProperties}
              data-reveal=""
              className="grid gap-1 border-t border-foreground/12 py-5 first:border-t-0 first:pt-0 sm:grid-cols-[12rem_minmax(0,1fr)] sm:items-baseline sm:gap-8"
            >
              <span className="text-[1.125rem] font-semibold tracking-[-0.01em]">{w.verb}</span>
              <span className="text-[1.0625rem] leading-[1.5] text-pretty text-muted-foreground">{w.why}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
