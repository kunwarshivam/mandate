import { BOLD, BUTTON, FIELD, LINK, PLAIN_BUTTON, SUNKEN } from "@/components/site/letter";
import { cn } from "@/lib/utils";

/** The sign-in pages' controls, from the landing page's letter (`site/letter.ts`), so signing in looks like the page it came from. */
export const LOGON_HEADING = `${BOLD} text-[1.625rem]`;

/** The dialog's default button: grey, ringed in ink, full width, and 44px so a thumb finds it. */
export const LOGON_PRIMARY = cn(BUTTON, "h-11 w-full gap-2.5");

export const LOGON_SECONDARY = cn(PLAIN_BUTTON, "h-11 w-full gap-2.5");

export const LOGON_FIELD = cn(FIELD, "h-11 max-w-none");

export const LOGON_NOTICE = `${SUNKEN} bg-card px-3 py-2 text-[1rem] leading-snug`;

export const LOGON_LINK = `${LINK} w-fit`;
