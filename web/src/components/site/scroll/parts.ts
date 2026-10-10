import styles from "./scroll.module.css";

/** The long page's parts, in order, as its bar links to them. */
export const PARTS = [
  { id: "checks", label: "Checks" },
  { id: "asking", label: "Asking you" },
  { id: "threads", label: "Threads" },
  { id: "limits", label: "Limits" },
  { id: "wont", label: "Won't do" },
] as const;

export const INTRO_ID = "intro";
export const SIGN_UP_ID = "sign-up";
export const PAGE_FORM_ID = "beta-page";

export const WRAP = "mx-auto w-full max-w-[80rem] px-5 sm:px-8 lg:px-12";

const HEADING = "font-normal text-balance tracking-[-0.022em]";
export const DISPLAY = `${HEADING} text-[clamp(2.75rem,1.4rem+5vw,6.5rem)] leading-[0.98]`;
export const DISPLAY_MD = `${HEADING} text-[clamp(2.25rem,1.4rem+2.9vw,4.25rem)] leading-[1.02]`;
const LEAD_TYPE = "max-w-[36rem] text-[1.125rem] leading-[1.6] text-pretty sm:text-[1.25rem]";
export const LEAD = `${LEAD_TYPE} text-muted-foreground`;
/** The lead on the page's one tide section. */
export const LEAD_ON_TIDE = `${LEAD_TYPE} text-tide-muted`;

/** Lifts on hover and squashes when pressed, on a spring, with motion allowed. */
export const PRESS = styles.press;

/** The page's one filled button: ink, in a pill. */
export const PRIMARY = `${PRESS} inline-flex h-12 items-center justify-center gap-2 rounded-full bg-foreground px-6 text-[1rem] font-semibold text-background outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background`;

/** A quiet button beside it: a ring of ink. */
export const SECONDARY = `${PRESS} inline-flex h-12 items-center justify-center gap-2 rounded-full px-6 text-[1rem] font-semibold text-foreground ring-1 ring-foreground/25 outline-none hover:ring-foreground focus-visible:ring-3 focus-visible:ring-ring`;
