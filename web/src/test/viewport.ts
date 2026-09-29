/** Kumo's Sidebar reads this query to choose the phone sheet over the desktop layout (the dock). */
const BELOW_LG = "(max-width: 1023px)";

/** Report a phone or tablet to `matchMedia` until the returned function restores it. */
export function asPhone(): () => void {
  const original = window.matchMedia;
  window.matchMedia = (query: string) => ({ ...original(query), matches: query === BELOW_LG, media: query }) as MediaQueryList;
  return () => {
    window.matchMedia = original;
  };
}
