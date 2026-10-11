import "./faces";
import type { ReactNode } from "react";
import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import { BODY, WINDOW_FRAME } from "./letter";
import { TitleBar } from "./title-bar";
import { Wallpaper } from "./wallpaper";

/**
 * The sign-in pages as the logon dialog of the landing page's desktop: the same wallpaper, one window
 * in the middle, in the landing page's faces and bevels. Its close box goes back to the home page.
 */
export function Logon({ title, children }: { title: string; children: ReactNode }) {
  return (
    <main id="main" tabIndex={-1} className={cn("relative isolate grid min-h-dvh flex-1 place-items-center px-3 py-6 outline-none sm:p-8", BODY)} data-slot="logon">
      <Wallpaper />
      <section aria-label={title} className={cn(WINDOW_FRAME, "w-full max-w-[31rem]")} data-slot="logon-window">
        <TitleBar title={title} icon={<BrandOwl className="size-4" />} controls={{ close: { label: "Close, back to the Owlhead home page", href: "/" } }} />
        <div className="min-w-0 px-5 pt-5 pb-6 sm:px-7 sm:pt-6 sm:pb-7">{children}</div>
      </section>
    </main>
  );
}
