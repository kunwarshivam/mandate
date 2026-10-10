/** The long page's parts, in order, as its bar links to them. */
export const PARTS = [
  { id: "checks", label: "Checks" },
  { id: "asking", label: "Asking you" },
  { id: "threads", label: "Threads" },
  { id: "limits", label: "Limits" },
  { id: "wont", label: "Won't do" },
] as const;

export const SIGN_UP_ID = "sign-up";
export const PAGE_FORM_ID = "beta-page";

export const WRAP = "mx-auto w-full max-w-[80rem] px-5 sm:px-8 lg:px-12";

const HEADING = "font-normal text-balance tracking-[-0.022em]";
export const DISPLAY = `${HEADING} text-[clamp(2.75rem,1.4rem+5vw,6.5rem)] leading-[0.98]`;
export const DISPLAY_MD = `${HEADING} text-[clamp(2.25rem,1.4rem+2.9vw,4.25rem)] leading-[1.02]`;
export const LEAD = "max-w-[36rem] text-[1.125rem] leading-[1.6] text-pretty text-muted-foreground sm:text-[1.25rem]";

/** The page's one filled button: ink, in a pill. */
export const PRIMARY =
  "inline-flex h-12 items-center justify-center gap-2 rounded-full bg-foreground px-6 text-[1rem] font-semibold text-background outline-none transition-transform duration-150 ease-out hover:-translate-y-px focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background active:translate-y-0 motion-reduce:transition-none motion-reduce:hover:translate-y-0";

/** A quiet button beside it: a ring of ink. */
export const SECONDARY =
  "inline-flex h-12 items-center justify-center gap-2 rounded-full px-6 text-[1rem] font-semibold text-foreground ring-1 ring-foreground/25 outline-none transition-colors duration-150 hover:ring-foreground focus-visible:ring-3 focus-visible:ring-ring";
