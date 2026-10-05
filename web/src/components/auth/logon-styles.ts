import { BOLD, BUTTON, FIELD, LINK, PLAIN_BUTTON, SUNKEN } from "@/components/site/letter";

/** The sign-in pages' controls, from the landing page's letter (`site/letter.ts`), so signing in looks like the page it came from. */
export const LOGON_HEADING = `${BOLD} text-[1.625rem]`;

/** The dialog's default button: sun, ringed in ink, full width. */
export const LOGON_PRIMARY = `${BUTTON} h-10 w-full gap-2.5`;

export const LOGON_SECONDARY = `${PLAIN_BUTTON} h-10 w-full gap-2.5`;

export const LOGON_FIELD = `${FIELD} h-10 max-w-none`;

export const LOGON_NOTICE = `${SUNKEN} bg-card px-3 py-2 text-[1rem] leading-snug`;

export const LOGON_LINK = `${LINK} w-fit`;
