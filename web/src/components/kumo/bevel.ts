/**
 * The landing page's window chrome on a few of the app's Kumo buttons (DEC-452), so the two read as
 * one product: a raised 2px edge, lit on the top and left and shaded on the bottom and right, on the
 * muted fill, that sinks in while pressed, with the label in the chrome's pixel face. It takes the
 * place of Kumo's hairline ring and shadow, through Kumo's own class merge; the focus ring stays.
 * Flat colour only (DEC-200). Stop, the kill switch, the dock, the tab bar and menus keep their own
 * shapes.
 */
/**
 * A decision's pair of answers, Approve and Skip (DEC-467): a solid lit key on the card, a crisp ink
 * edge with a heavier foot that sinks while pressed, and an ink label in Public Sans. Both answers
 * wear it alike, so neither leads. The bevel's muted fill and pixel face read as disabled here, so
 * the bevel stays on secondary actions.
 */
export const DECISION_KEY =
  "press h-12 w-full justify-center rounded-lg border border-foreground/45 border-b-[3px] border-b-foreground/80 bg-card text-base font-semibold text-foreground shadow-none ring-0 hover:bg-background active:not-disabled:border-b active:not-disabled:pt-0.5 forced-colors:border-[ButtonText]";

export const BEVEL =
  "pixel-face rounded-sm border-2 border-t-card border-l-card border-r-foreground/60 border-b-foreground/60 bg-muted shadow-none ring-0 active:not-disabled:border-t-foreground/60 active:not-disabled:border-l-foreground/60 active:not-disabled:border-r-card active:not-disabled:border-b-card forced-colors:border-[ButtonText]";
