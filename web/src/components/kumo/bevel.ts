/**
 * The landing page's window chrome on a few of the app's Kumo buttons (DEC-452), so the two read as
 * one product: a raised 2px edge, lit on the top and left and shaded on the bottom and right, on the
 * muted fill, that sinks in while pressed, with the label in the chrome's pixel face. It takes the
 * place of Kumo's hairline ring and shadow, through Kumo's own class merge; the focus ring stays.
 * Flat colour only (DEC-200). Stop, the kill switch, the dock, the tab bar and menus keep their own
 * shapes.
 */
export const BEVEL =
  "pixel-face rounded-sm border-2 border-t-card border-l-card border-r-foreground/60 border-b-foreground/60 bg-muted shadow-none ring-0 active:not-disabled:border-t-foreground/60 active:not-disabled:border-l-foreground/60 active:not-disabled:border-r-card active:not-disabled:border-b-card forced-colors:border-[ButtonText]";
