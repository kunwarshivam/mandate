"use client";

import type { MouseEvent } from "react";
import Link from "next/link";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { cn } from "@/lib/utils";
import { PLAIN_BUTTON } from "./letter";
import styles from "./letter.module.css";

/**
 * Sign in and Sign up, in words, where the desktop's bar always shows them: the taskbar on Windows,
 * the menu bar on the Mac (DEC-906 item 8). Sign up is the one filled button on the desktop, and opens
 * the guestbook, the private beta's request form, out of itself.
 */
export function AccountButtons({ onSignUp, compact = false }: { onSignUp: (e: MouseEvent<HTMLButtonElement>) => void; compact?: boolean }) {
  const size = compact ? "h-5 px-2 text-[0.875rem]" : "h-8 px-2.5 text-[0.875rem] sm:px-3 sm:text-[0.9375rem]";
  return (
    <div className={cn("flex shrink-0 items-center", compact ? "gap-1.5 px-1.5" : "gap-1")} data-slot="account-buttons">
      <Link href={LOGIN_PATH} className={cn(PLAIN_BUTTON, size, "whitespace-nowrap")}>
        Sign in
      </Link>
      <button type="button" onClick={onSignUp} className={cn(PLAIN_BUTTON, size, "whitespace-nowrap bg-highlight text-highlight-foreground ring-1 ring-foreground", styles.signUp)}>
        Sign up
      </button>
    </div>
  );
}
