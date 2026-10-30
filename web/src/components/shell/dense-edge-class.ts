/**
 * The class that keeps the dense column's left edge on the calm page's, widening only to the
 * right (critique C-17, `web/design/critique-2026-10-09.md`: "**C-17. Audit, 1440px.** The dense
 * screens start their content at 40px, while the page's own \"Fixture data\" tag, and every
 * calm screen, start at 120px. Moving from More to Audit makes the left edge jump, and the
 * tag floats away from the column it labels. *The record*: the screens that hold the record
 * look like a different product. *Fix:* the dense column keeps the calm page's left edge and
 * only widens to the right, or the tag follows the dense column. Pick whichever keeps one
 * left edge across both densities. *DEC:* no."). Of the two fixes the row hands the lane, the
 * pick is the first, the one the row's own test — "whichever keeps one left edge across both
 * densities" — selects: only it keeps the same edge as every calm screen; the second would
 * carry the page's own tag to the dense column's 40px edge and still leave the edge jumping
 * from every calm screen's 120px into Audit, and it would reach into the frame's shared foot,
 * while the first moves the dense column alone and brings the tag back over the column it
 * labels without moving the tag.
 *
 * The shape it returns: `w-auto ml-[max(0px,calc((100%-80rem)/2))] mr-0`, carried by the shell's
 * main column wherever `densityFor` reads dense (`app-shell.tsx`), beside the frame every
 * calm screen draws. The left inset is the calm page's own: `max(0px, calc((100%-80rem)/2))`
 * is the inset the calm column's centred 80rem bound (`--content-max` at `:root`, the width
 * behind the row's 120px) leaves at every width, given the shell already puts the same
 * `px-(--page-x)` gutters inside it, so both densities share one left edge at every width —
 * below 80rem the inset is 0 and the page's gutters hold on both, above it the inset mirrors
 * the calm column's.
 * `w-auto` and `mr-0`, replacing `w-full` and `mx-auto` on the column, drop the centred
 * widening so the column extends to the right only, never past its own density's bound
 * (`--content-max` 90rem under `data-density="dense"`, unchanged); the merge helper keeps
 * `mx-auto` beside a later `ml-`, so the dense column's frame is the frame's own pieces with
 * the centring gone, not `PAGE_FRAME` with the class appended. The phone is laid out by CSS
 * with no script media query (DEC-207): one class needs no media boundary. The page's foot —
 * the row's "Fixture data" tag — keeps its today's frame; still on the page's own edge, it
 * sits back over the column it labels.
 */
export function denseEdgeClass(): string {
  return "w-auto ml-[max(0px,calc((100%-80rem)/2))] mr-0";
}
