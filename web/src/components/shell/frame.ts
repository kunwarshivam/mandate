/** A page's frame: centred, at most the content width, inside the page's gutters. */
export const PAGE_FRAME = "mx-auto w-full max-w-(--content-max) px-(--page-x) pt-(--page-top) pb-10";

/**
 * A thread fills the window edge to edge (DEC-481, DEC-2293): an agent's and a new agent's setup,
 * with no gutter and no content width, scrolling on its own between the header and the dock or the
 * tab bar, as a chat app does. An agent's record screens (its kill switch) keep the page frame.
 */
export function fullBleed(pathname: string): boolean {
  return /^\/(new|agents\/[^/]+)\/?$/.test(pathname) && pathname !== "/agents/new";
}
