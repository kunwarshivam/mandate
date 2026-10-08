# Design direction, 7 October 2026

Notes from a conversation with the founder after the UI review
([review-2026-10-07.md](review-2026-10-07.md)). The founder's ask: a product and a website among the
best designed anywhere; unforgettable and quirky, never cringey. Nothing here is decided; each item
becomes a story and, where it changes a rule, a DEC. The rules in [DESIGN.md](../DESIGN.md) bind
every item.

## The test for quirk

Quirk is a consistent world; cringe is a joke the product tells about itself. Minesweeper was cut
because it was the second kind. The Great Wave desktop and the record window are the first. For
every idea: would it still work if the owner were having a bad day with real money? If yes, it is
quirk. If it only works when they are in on the joke, it is cringe.

The premise the product already has: a pixel owl that watches your account from a 1996 desktop and
writes everything down. Most products have a style; this one has a premise. Unforgettable comes
from committing to the premise everywhere, not from adding gags.

## On par, then ahead

On par is table stakes and mostly done: one face, tokens, dark mode, reduced motion, a keyboard
palette, phone layout, degraded states. What is left is texture: instant navigation, every state
designed, one voice, and the cuts from the review so the screens breathe.

Ahead cannot come from polish, because others have more polishers. It comes from making visible
three things no brokerage or trading bot has:

1. **The record.** Every other product shows what it bought; Owlhead shows why, from the filing
   it read to the fill, in eight lines. One click from every order, position and alert, with a
   "Try to change it" that fails the hash chain in front of the owner.
2. **The mandate as a living object.** Scrub the chart and the headroom bars move; type a change
   and see which rung it moves and whether the gate would allow it; on an approval, the bars as
   they would be after the fill.
3. **Trust you can test.** Stop on every screen, deadlines that never tick, silence never buys, a
   kill switch that does not depend on model state. Each becomes something the owner can do: "What
   if I do nothing?" on every request; a Stop sheet that says what will rest at the broker
   afterwards; a Verify chain button that runs in front of them.

## Ideas that fit the premise

1. **The owls are the agents, fully.** One pose per state, one frame change per state, never a
   loop: awake and trading; head turned, watching a stale feed; asleep with the "z" when paused;
   eyes shut and greyed when stopped. The owl's face never changes with P&L.
2. **The deadline is a sand clock, not a countdown.** The time it dies at and a pixel hourglass
   that is simply there, the same at nine minutes and one. It says "this has a deadline" without
   pressure.
3. **The agent's diary.** An agent's activity in the first person, from the journal, deterministic
   text, no model: "09:41 Read XYZ's filing. 09:41 Decided to buy. 09:43 You said yes. 09:43
   Filled." The most shareable screen and the plainest compliance story.
4. **"Try to change it."** The landing page's tamper demo on every real record, with the owner's
   own data.
5. **The Stop sheet as a physical object.** A plain bevelled switch under a cover: Pause is the
   cover coming off, Stop is the switch. The same hierarchy as today, drawn like a machine's control
   rather than a modal.
6. **Silence as a feature.** The all-clear on Home is the owl asleep on a branch and one line,
   "Nothing needs you." No chart of the day, no streak, nothing else.
7. **Dark mode is night.** The moonlight prints (`public/art`) behind the logon window, the owls'
   eye highlight brighter, the same tokens, one world.
8. **Sound, off by default, physical.** One click for a confirmed approval, one soft thud for Stop,
   nothing for gains or losses.

## What to refuse

- Mascot speech. The owl never talks in bubbles, never has a personality in copy. It watches and
  writes. The copilot is Owlhead, not an owl's voice.
- Jokes in copy, exclamation marks, "oops", "yay", "uh-oh".
- Anything that reacts to money going up or down.
- Easter eggs that do things. A Konami code that opens Winamp is fine; one that changes a setting
  is not.
- Pixel art for the data. Charts, figures and tables stay in Public Sans and clean lines. The
  pixel world is the frame and the characters, not the numbers.

## Craft: the standard that makes "best" credible

1. **Instant.** Every navigation under 100ms with no spinner; prefetch on hover and on the dock;
   the hero number in the HTML on first paint, never a skeleton. A Playwright budget per route,
   failed in CI.
2. **Type set by a typographer.** Optical margin alignment on the hero (the `$` hangs so the digits
   align with the text below); hanging punctuation in the diary; a thin space before `%`; the care
   given to the cents extended to every figure; prose never over 65 characters a line.
3. **An 8px rhythm with no exceptions.** Every gap, row height and icon box on the grid, audited by
   screenshot overlay.
4. **Transitions that explain.** Opening an agent from Home, the owl and the name travel to the
   page header (a shared-element transition, 200ms); an approved request slides down into
   Resolved rather than vanishing. Motion that shows where things went. Off under reduced motion.
5. **Every state designed.** Loading, empty, error, stale, unreachable, no permission, first run,
   one agent, twenty agents, a $12 account, a $4M account, a nine-character ticker, a name in
   Vietnamese.
6. **One voice, one person.** Every string read aloud in one sitting; the landing page's voice is
   the one to keep.
7. **Keyboard as a first-class surface.** `j`/`k` through requests, `a`/`s` for approve and skip
   (equal weight kept), `g h` for Home, `?` for the map.
8. **Dark mode as a second design.** Hairlines slightly lighter, the chart line thinner, the volt a
   touch less neon, shadows gone.

## Moments: the three things people will remember

9. **The diary** (above).
10. **The mandate you can feel.** Scrub the chart and the headroom bars move with the cursor; drag a
    limit line on the mandate page and a sentence says what would have happened last week under
    that rule, from the journal, never a forecast.
11. **The hatch.** Creating an agent ends with a passkey and then the owl hatching from an egg on
    the 16px grid, once, 600ms, then sitting on its branch with its name under it. Answers the
    owner's action, so it is allowed.

## The website

12. **The desktop is a place, not a page.** Windows open beside each other rather than on top; a
    double-click on a title bar maximises; the taskbar switches; Alt+Tab works. Then only what a
    1996 desktop would have had: a clock that keeps real time; Display Properties that changes the
    wallpaper among the six prints; a screensaver after two minutes idle (the owls flying; off
    under reduced motion).
13. **One live thing.** A real window showing a demo paper agent's last five decisions with real
    timestamps, read-only. The real product doing its real job in the open.
14. **A print page.** `⌘P` on any record gives a clean hairline document in Public Sans with the
    hash at the foot.

## Process

15. **A critique rhythm.** Once a week, screenshots of every screen; one question: what is the
    worst thing on this screen? Fix only that.
16. **Reference, not inspiration.** Five products and what is taken from each: Linear (speed and
    keyboard), Bloomberg (density without clutter), Things (calm states), Arc (character without
    jokes), the Macintosh HIG of 1992 (consistency). Nothing else on the list.
17. **Five people, watched.** No telemetry by design, so five owners, twenty minutes each, say
    nothing, write down where they hesitate.
18. **A design engineer's checklist in CI.** Not look-pinning tests (DEC-511) but the craft rules:
    the 8px grid, the type scale only, contrast, targets, no orphan words in headings, no string
    outside the voice file.
19. **A golden path.** One ten-minute journey (sign in, set up an agent, see it ask, approve, read
    the record, stop it) that must be perfect on every release, reviewed from screenshots.

## Where to start

The three that move furthest: craft 1 (instant), craft 4 (transitions that explain), and moment
10 (the mandate you can feel). The first two are the difference between "nice" and "fast and
coherent"; the third is the thing nobody else can build. Of the premise ideas, the owl states (1)
and the diary (3) first: both deterministic, both inside the rules, and together the thing people
remember.
