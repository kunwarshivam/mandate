/**
 * The app's one button (DEC-469), the face Approve and Skip took first (DEC-467): a solid key on the
 * card, a crisp ink edge with a heavier foot that sinks while pressed, and a semibold Public Sans
 * label. It goes on Kumo's `Button` and `LinkButton` through Kumo's own class merge, taking the place
 * of Kumo's ring, shadow and pill, and on the app's own buttons and button links as it is. Flat colour
 * only (DEC-200). Stop, the kill-switch choices, the dock, the tab bar, menus and list rows keep their
 * own shapes.
 */
const FACE =
  "press inline-flex items-center justify-center gap-2 rounded-lg border border-foreground/45 border-b-[3px] border-b-foreground/80 bg-card font-semibold text-foreground shadow-none ring-0 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 active:not-disabled:border-b active:not-disabled:pt-0.5 disabled:cursor-not-allowed disabled:opacity-55 forced-colors:border-[ButtonText]";

/** The button, 44px tall. */
export const KEY = `${FACE} h-11 px-5 text-base`;

/** A row's own actions, as on a passkey's line in Settings: still 44px for a finger, narrower, in a smaller label. */
export const KEY_SM = `${FACE} h-11 shrink-0 px-3.5 text-sm`;

/** A decision's pair of answers, Approve and Skip: 48px and the full width, the two alike so neither leads. */
export const DECISION_KEY = `${FACE} h-12 w-full text-base`;
