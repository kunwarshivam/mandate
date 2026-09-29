/** The text assistive technology reads from an element: everything outside `aria-hidden` subtrees. */
export function spoken(el: Element): string {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let text = "";
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.parentElement?.closest("[aria-hidden=true]")) text += node.textContent;
  }
  return text;
}

/** The text drawn for sighted readers: everything outside screen-reader-only copies. */
export function drawn(el: Element): string {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let text = "";
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.parentElement?.closest(".sr-only")) text += node.textContent;
  }
  return text;
}
