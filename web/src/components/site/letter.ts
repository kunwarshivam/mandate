/**
 * The landing page is one homepage from the late 1990s, open in a browser of the time: a dot matrix
 * face on a dithered grey desktop, bevelled chrome, a contents frame, grooved rules, underlined
 * links, and form controls with raised and sunken edges. Every colour is a token, so dark mode is the
 * same page at night.
 */
import styles from "./letter.module.css";

export const BODY = styles.body;

export const MONO = styles.mono;

export const PIXEL = styles.pixel;

export const FOCUS = "outline-none focus-visible:bg-highlight focus-visible:text-highlight-foreground focus-visible:outline-2 focus-visible:outline-dotted focus-visible:outline-offset-2 focus-visible:outline-foreground";

export const LINK = `text-mandate-strong underline decoration-1 underline-offset-[0.15em] hover:bg-highlight hover:text-highlight-foreground ${FOCUS}`;

/** Light on the top and left, dark on the bottom and right: a surface that stands out. On the Mac desktop, a one-pixel ink box. */
export const RAISED = `border-2 border-t-card border-l-card border-r-foreground/60 border-b-foreground/60 ${styles.raised}`;

/** The reverse: a well that sinks in. */
export const SUNKEN = `border-2 border-t-foreground/60 border-l-foreground/60 border-r-card border-b-card ${styles.sunken}`;

/** A window's frame: grey and bevelled on Windows, white with a hard ink shadow on the Mac. */
export const WINDOW_FRAME = `${RAISED} bg-muted p-0.5 ring-1 ring-foreground/70 ${styles.osWindow}`;

/** A dropped-down menu's panel, framed as a window is. */
export const MENU_PANEL = `${RAISED} bg-muted py-1 ring-1 ring-foreground/70 ${styles.menu}`;

export const RULE = "my-10 border-0 border-t border-b border-t-foreground/45 border-b-card sm:my-12";

export const H2 = "scroll-mt-6 text-[1.875rem] leading-tight font-normal text-balance";

/** Bold, in the one face here that has it. */
export const BOLD = `${styles.pixel} text-[1.1em] leading-none font-semibold`;

const BEVEL = `press inline-flex cursor-pointer items-center justify-center gap-1.5 ${RAISED} ${styles.pixel} active:border-t-foreground/60 active:border-l-foreground/60 active:border-r-card active:border-b-card disabled:cursor-wait disabled:opacity-70 outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground`;

/** The default button: the same grey as every other, set apart by the extra ink ring a window's default button had. */
export const BUTTON = `${BEVEL} h-9 min-w-32 bg-muted px-4 text-[0.9375rem] text-foreground ring-1 ring-foreground ${styles.button}`;

export const PLAIN_BUTTON = `${BEVEL} h-8 bg-muted px-3 text-[0.9375rem] text-foreground ${styles.plainButton}`;

/** Typed text in the terminal face, as the address is, at 20px so a phone never zooms into the field. */
export const FIELD = `h-9 w-full max-w-[22rem] ${SUNKEN} bg-card px-2 ${styles.mono} text-[1.25rem] leading-none text-foreground placeholder:text-muted-foreground outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-foreground`;

/** A line in a menu of the time: ink on grey, lit in ink while pointed at or focused. */
export const MENU_ITEM = `flex w-full cursor-pointer items-center gap-2.5 px-2 py-1 text-start text-[0.9375rem] outline-none hover:bg-foreground hover:text-card focus-visible:bg-foreground focus-visible:text-card ${styles.pixel}`;

/** Greyed out as the period drew it: faint ink, etched by a light edge below and to the right. */
export const ETCHED = "cursor-default text-foreground/40 [text-shadow:1px_1px_0_var(--card)] [&_svg]:drop-shadow-[1px_1px_0_var(--card)]";
