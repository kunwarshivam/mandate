/** The query a component would read to lay out for a phone or tablet in JS; the shell must not need it. */
const BELOW_LG = "(max-width: 1023px)";

/** Report a phone or tablet to `matchMedia` until the returned function restores it. */
export function asPhone(): () => void {
  const original = window.matchMedia;
  window.matchMedia = (query: string) => ({ ...original(query), matches: query === BELOW_LG, media: query }) as MediaQueryList;
  return () => {
    window.matchMedia = original;
  };
}

/** Classes that hide an element on a phone (under 40rem), whatever it shows from a wider breakpoint. */
const HIDES_ON_PHONE = /(^|\s)(hidden|sr-only|max-sm:hidden|max-md:hidden|max-lg:hidden|max-xl:hidden|max-\[\d+rem\]:hidden)(\s|$)/;

/** Classes that hide an element from `lg` up, or hide it with nothing that shows it again at `lg`. */
const HIDES_ON_DESKTOP = /(^|\s)(lg:hidden|xl:hidden)(\s|$)/;
const SHOWS_AT_LG = /(^|\s)lg:(block|flex|grid|inline|inline-flex|inline-block|table|contents)(\s|$)/;

export function shownOnDesktop(el: Element): boolean {
  for (let node: Element | null = el; node && node !== document.body; node = node.parentElement) {
    const cls = node.getAttribute("class") ?? "";
    if (node.hasAttribute("hidden") || HIDES_ON_DESKTOP.test(cls)) return false;
    if (/(^|\s)(hidden|sr-only)(\s|$)/.test(cls) && !SHOWS_AT_LG.test(cls)) return false;
  }
  return true;
}

/**
 * Whether an element shows on a phone, read from its and its ancestors' classes: jsdom applies no
 * media queries, so the shell's breakpoint classes are the only evidence. The e2e suite checks the
 * real layout.
 */
export function shownOnPhone(el: Element): boolean {
  for (let node: Element | null = el; node && node !== document.body; node = node.parentElement) {
    if (node.hasAttribute("hidden") || HIDES_ON_PHONE.test(node.getAttribute("class") ?? "")) return false;
  }
  return true;
}
