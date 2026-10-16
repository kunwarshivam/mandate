"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { cn } from "@/lib/utils";
import { PAGE_FORM_ID, SIGN_UP_ID } from "./parts";
import styles from "./scroll.module.css";

const ACCOUNT = "inline-flex h-9 items-center rounded-full px-4 text-[0.9375rem] font-semibold whitespace-nowrap outline-none focus-visible:ring-3 focus-visible:ring-ring";

/** Brings the request form at the foot of the page into view and puts the cursor in its email field. */
export function toSignUp() {
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  document.getElementById(SIGN_UP_ID)?.scrollIntoView({ behavior: still ? "auto" : "smooth", block: "start" });
  document.getElementById(`${PAGE_FORM_ID}-email`)?.focus({ preventScroll: true });
}

/**
 * The long page's one header (DEC-907, DEC-932): the logo back to the desktop, and Sign in and Sign
 * up. It stays out of sight while the desktop's own bar is on screen and is pinned to the top once
 * the page has covered the desktop, so Sign in and Sign up are always in view (DEC-906 item 8) and
 * never twice. It lists none of the page's parts (DEC-908).
 */
export function ScrollBar() {
  const ref = useRef<HTMLElement>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    const sheet = ref.current?.parentElement;
    if (!sheet) return;
    const check = () => setShown(sheet.getBoundingClientRect().top <= 0);
    check();
    window.addEventListener("scroll", check, { passive: true });
    window.addEventListener("resize", check);
    return () => {
      window.removeEventListener("scroll", check);
      window.removeEventListener("resize", check);
    };
  }, []);

  return (
    <header ref={ref} className={cn(styles.bar, "fixed inset-x-0 top-0 z-20")} data-slot="page-bar" data-shown={shown ? "" : undefined} inert={!shown}>
      <div className="mx-auto flex h-14 w-full max-w-[80rem] items-center gap-2 px-3 sm:px-6 lg:px-10">
        <a href="#desktop" className="flex shrink-0 items-center gap-2 rounded-md px-1 outline-none focus-visible:ring-3 focus-visible:ring-ring" style={{ color: "var(--logo)" }}>
          <span className="sr-only">Owlhead, back to the desktop</span>
          <BrandOwl className="size-7" />
          <OwlheadWordmark title="" className="h-5 w-auto max-[380px]:hidden" />
        </a>
        <div className="ms-auto flex items-center gap-1.5" data-slot="account-buttons">
          <Link href={LOGIN_PATH} className={cn(ACCOUNT, "text-foreground hover:bg-muted")}>
            Sign in
          </Link>
          <button type="button" onClick={toSignUp} className={cn(ACCOUNT, "bg-highlight text-highlight-foreground ring-1 ring-foreground")}>
            Sign up
          </button>
        </div>
      </div>
    </header>
  );
}
