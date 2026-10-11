/**
 * The class that drops the slug from the phone thread header (critique C-16,
 * `web/design/critique-2026-10-09.md`: "**C-16. An agent's thread, 390px.** The thread header
 * packs back, owl, name, mode chip, a slug cut to \"btc-acc…\", \"Desk\" and the paper badge into
 * one row. \"Desk\" sits alone with no \"Chat\" beside it, so it reads as a word, not a switch.
 * *The owner.* *Fix:* drop the slug from the phone thread header, since the owl and the name
 * already say which agent it is. Name \"Desk\" in the vocabulary decision plan section D already
 * holds (\"Three nouns\"). *DEC:* no for the slug; the name goes through the D item."). The class
 * hides the slug on the phone alone, scoped below the thread bar's own boundary — `lg`, the width
 * its back link, owl, paper badge and view tabs already switch at — so from 64rem the header keeps
 * today's slug and only the phone's drawing changes; the phone is laid out by CSS with no script
 * media query (DEC-207). The thread bar carries the class on its slug span
 * (`messages-screen.tsx`); the name the span sits under is this change's to keep, not to rename
 * (plan D's "Three nouns" item holds the naming).
 */
export function threadSlugClass(): string {
  return "max-lg:hidden";
}
