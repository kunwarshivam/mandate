/** The hero rows that critique C-2 orders by name: the subhead, the actions, the beta line. */
export type HeroRow = "subhead" | "actions" | "beta-line";

/**
 * The class that stands the hero's row in the order a phone draws it: the actions directly under
 * the subhead, the beta line after them (critique C-2,
 * `web/design/critique-2026-10-09.md`: "*Fix:* on phones, put the three buttons directly under
 * the subhead and the beta line after them. *DEC:* no. DEC-467 chose the letters, not the
 * order."). The class carries a Tailwind order for the header's grid and is scoped below the
 * landing page's own phone boundary (`sm:`, the 40rem width its padding and its wide toolbar
 * already switch at), so from 40rem the header keeps today's order — the beta line between the
 * subhead and the actions — and the phone is laid out by CSS with no script media query
 * (DEC-207). The landing page's header carries each row's class on the row (`landing.tsx`).
 */
export function heroPhoneOrderClass(row: HeroRow): string {
  void row;
  throw new Error("Unimplemented: E11-9");
}
