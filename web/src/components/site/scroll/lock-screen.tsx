import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import { PixelNight } from "./pixel-night";
import styles from "./scroll.module.css";

/** The one sentence a request's push carries (`public/push-sw.js`, approval_needed). */
export const PUSH_TEXT = "An agent in your workspace needs your approval";

/**
 * A phone's lock screen with Owlhead's push on it: the brand and the fixed sentence, nothing about the
 * trade, over a night in pixels. It is night in both themes, so it takes only colours that stay put.
 */
export function LockScreen({ className }: { className?: string }) {
  return (
    <div className={cn(styles.phone, "w-[16.5rem] shrink-0", className)} data-slot="lock-screen">
      <div className="relative isolate flex aspect-[390/844] flex-col items-center px-3 pt-14 text-tide-foreground">
        <PixelNight className="absolute inset-0 -z-10 size-full" />
        <span aria-hidden className="absolute top-2.5 left-1/2 h-6 w-24 -translate-x-1/2 rounded-full bg-highlight-foreground" />
        <p className="text-[0.875rem] font-medium">Friday, September 25</p>
        <p className="text-[4.25rem] leading-none font-semibold tracking-[-0.03em] tabular-nums">2:04</p>
        <div className="mt-8 flex w-full items-start gap-2.5 rounded-2xl bg-tide-foreground px-3 py-2.5 text-start text-highlight-foreground" data-reveal="drop">
          <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-background ring-1 ring-highlight-foreground/15" style={{ color: "var(--logo)" }}>
            <BrandOwl still className="size-6" />
          </span>
          <span className="grid min-w-0 gap-0.5">
            <span className="flex items-baseline justify-between gap-2 text-[0.8125rem]">
              <span className="font-semibold">Owlhead</span>
              <span>now</span>
            </span>
            <span className="text-[0.875rem] leading-snug">{PUSH_TEXT}</span>
          </span>
        </div>
      </div>
    </div>
  );
}
