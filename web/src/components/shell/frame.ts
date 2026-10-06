/** A page's frame: centred, at most the content width, inside the page's gutters. */
export const PAGE_FRAME = "mx-auto w-full max-w-(--content-max) px-(--page-x) pt-(--page-top) pb-10";

/**
 * Messages fills the window edge to edge (DEC-478): no gutter and no content width, its panes each
 * scrolling on their own between the header and the dock or the tab bar, as a chat app does.
 */
export function fullBleed(pathname: string): boolean {
  return /^\/messages(\/|$)/.test(pathname);
}
