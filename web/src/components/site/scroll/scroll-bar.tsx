"use client";

import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { cn } from "@/lib/utils";
import { PAGE_FORM_ID, PARTS, SIGN_UP_ID } from "./parts";
import styles from "./scroll.module.css";

const PART_LINK =
  "rounded-full px-3 py-1.5 text-[0.9375rem] font-medium whitespace-nowrap text-muted-foreground outline-none transition-colors duration-150 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring";

const ACCOUNT = "inline-flex h-9 items-center rounded-full px-4 text-[0.9375rem] font-semibold whitespace-nowrap outline-none focus-visible:ring-3 focus-visible:ring-ring";

/** Brings the request form at the foot of the page into view and puts the cursor in its email field. */
export function toSignUp() {
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  document.getElementById(SIGN_UP_ID)?.scrollIntoView({ behavior: still ? "auto" : "smooth", block: "start" });
  document.getElementById(`${PAGE_FORM_ID}-email`)?.focus({ preventScroll: true });
}

/**
 * The long page's bar, on the top edge of the sheet that rises over the desktop: it rides up with the
 * sheet and then sticks, so Sign in and Sign up stay in view once the desktop's own bar is covered
 * (DEC-906 item 8).
 */
export function ScrollBar() {
  return (
    <div className={cn(styles.bar, "sticky top-0 z-20")} data-slot="page-bar">
      <div className="mx-auto flex h-14 w-full max-w-[80rem] items-center gap-2 px-3 sm:px-6 lg:px-10">
        <a href="#desktop" className="flex shrink-0 items-center gap-2 rounded-md px-1 outline-none focus-visible:ring-3 focus-visible:ring-ring" style={{ color: "var(--logo)" }}>
          <span className="sr-only">Owlhead, back to the desktop</span>
          <BrandOwl className="size-7" />
          <OwlheadWordmark title="" className="h-5 w-auto max-[380px]:hidden" />
        </a>
        <nav aria-label="On this page" className="mx-auto max-lg:hidden">
          <ul className="flex items-center">
            {PARTS.map((p) => (
              <li key={p.id}>
                <a href={`#${p.id}`} className={PART_LINK}>
                  {p.label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className="ms-auto flex items-center gap-1.5 lg:ms-0" data-slot="account-buttons">
          <Link href={LOGIN_PATH} className={cn(ACCOUNT, "text-foreground hover:bg-muted")}>
            Sign in
          </Link>
          <button type="button" onClick={toSignUp} className={cn(ACCOUNT, "bg-highlight text-highlight-foreground ring-1 ring-foreground")}>
            Sign up
          </button>
        </div>
      </div>
    </div>
  );
}
