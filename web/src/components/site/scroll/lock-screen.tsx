import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import styles from "./scroll.module.css";

/** The one sentence a request's push carries (`public/push-sw.js`, approval_needed). */
export const PUSH_TEXT = "An agent in your workspace needs your approval";

/** A phone's lock screen with Owlhead's push on it: the brand and the fixed sentence, nothing about the trade. */
export function LockScreen({ className }: { className?: string }) {
  return (
    <div className={cn(styles.phone, "w-[16.5rem] shrink-0", className)} data-slot="lock-screen">
      <div className="relative flex aspect-[390/844] flex-col items-center bg-lapis px-3 pt-14 text-card">
        <span aria-hidden className="absolute top-2.5 left-1/2 h-6 w-24 -translate-x-1/2 rounded-full bg-foreground" />
        <p className="text-[0.875rem] font-medium opacity-80">Friday, September 25</p>
        <p className="text-[4.25rem] leading-none font-semibold tracking-[-0.03em] tabular-nums">2:04</p>
        <div className="mt-8 flex w-full items-start gap-2.5 rounded-2xl bg-card px-3 py-2.5 text-start text-foreground">
          <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-background" style={{ color: "var(--logo)" }}>
            <BrandOwl still className="size-6" />
          </span>
          <span className="grid min-w-0 gap-0.5">
            <span className="flex items-baseline justify-between gap-2 text-[0.8125rem]">
              <span className="font-semibold">Owlhead</span>
              <span className="text-muted-foreground">now</span>
            </span>
            <span className="text-[0.875rem] leading-snug">{PUSH_TEXT}</span>
          </span>
        </div>
        <span aria-hidden className="mt-auto mb-7 flex w-full justify-between px-5">
          <span className="size-11 rounded-full bg-card/15" />
          <span className="size-11 rounded-full bg-card/15" />
        </span>
      </div>
    </div>
  );
}
