# Web design plan

The action items from the design conversation of 2026-10-07, in one list. Each is a story; the
ones marked DEC change a rule in [DESIGN.md](../DESIGN.md) and take a decision file first. The
evidence is in [review-2026-10-07.md](review-2026-10-07.md) (findings from screenshots, cited as
R-n) and the reasoning in [direction-2026-10-07.md](direction-2026-10-07.md). Tick an item when it
lands, with its PR.

The one idea behind every item: the product has a premise (an owl that trades inside rules you
wrote and shows you its record), and each item removes something standing between the owner and
the owl, the rules and the record.

## A. Process (done or in flight)

- [x] DESIGN.md holds principles; the rendering and the history move to `web/design/` (DEC-511,
      #669).
- [x] **Test sort** (DEC-511 item 5). Loosen look-pinning tests to the invariant they protect,
      remove the ones that only notice the look changed, keep every safety and compliance test
      exactly as it is. Its own PR. (part 1, #882; part 2, #1005)
- [ ] **A weekly critique.** Screenshots of every screen; one question: what is the worst thing on
      this screen; fix only that. (first run: #1017, [critique-2026-10-09.md](critique-2026-10-09.md),
      fixes in section J)
- [x] **A golden path.** Sign in, set up an agent, see it ask, approve, read the record, stop it:
      ten minutes that must be perfect on every release. (spec: #1035,
      `e2e/golden-path.spec.ts`, at 390 and 1440px in both themes; left as `fixme`: the request
      appearing once on Home, which waits on section J's "A request once on Home", C-5)
- [ ] **Five people, watched.** Twenty minutes each, say nothing, write down where they hesitate.
- [ ] **Reference, not inspiration.** Five products and what is taken from each, written down:
      Linear (speed and keyboard), Bloomberg (density without clutter), Things (calm states), Arc
      (character without jokes), the Macintosh HIG of 1992 (consistency). Nothing else on the
      list; a proposal that cites none of them is suspect.
- [ ] **A craft checklist in CI.** Not look-pinning tests (DEC-511) but the mechanical rules that
      make "best": the 8px grid, the type scale only, contrast, 44px targets, no orphan word in a
      heading, no string outside the voice file.

## B. Quick fixes (small, no DEC)

- [x] **The drawdown scenario's chart cliff.** The scenario kept the normal scenario's buy resting
      at $55,900 until 14:01 while the mark at 14:05 was $51,843, so the bars were held above
      $55,900 and fell 7% in four minutes. It now rests at $51,700, below the price's path, and
      `market.test.ts` fails if any curve in any scenario moves more than 2% in one minute. (R-9)
- [x] **The dead prototype route.** Not dead: DEC-472 moved the prototype to `/agents/new` and
      removed the route; the mention the review found is in the history. No change. (R-29)
- [x] **Positions table wrap.** The Mark cell no longer wraps, and the screen's own "Fixture
      data" tag is gone (the shell draws one). (R-16)
- [x] **Keyboard focus.** Verified: the ring paints (a 3px volt ring) on every header stop; the
      review's screenshot was too small to show it. No change. (R-19)
- [x] **Approvals countdown.** By the brief: whole minutes, quantized to 15 seconds, in a slot
      that holds the width of "(59 min left)", never seconds, never colour. No change. (R-14)
- [x] **Breadcrumb ellipsis** on top-level screens: the fold shows only past the top level. (R-8)
- [x] **Landing windows.** Each window now cascades 28px down and right of the one in front,
      so a new one shows the last rather than covering it. "Try it." already had its button on
      the next line, and the windows scroll; the review's capture used overlay scrollbars. (R-30)
- [x] **The account chart's dark-mode paint.** The first `npm run shots` run found the account
      chart blank in dark mode on Home at 390 and 1440px (captured 900ms after load) while the
      agent's chart drew and the same screens in light drew; earlier captures at 1500ms showed it.
      Find whether the dark-mode redraw (`setChartMode`) races the draw-in, and pin it with a test
      that reads the canvas after the mode switch. (#1032) Cause: hydration read the chart mode
      from the server's default, so a dark Home drew a light chart, then tore it down for a dark
      one, later than light; the blank captures themselves (C-22, both themes) were taken before
      the dev server hydrated, with no canvas yet, not a sizing or draw-in race. Fix: charts read
      `<html>` while hydrating, one chart per load; `e2e/chart-paint.spec.ts` reads its pixels.
- [x] **Set-up replies on a miss.** A second miss in a row says what shape of answer would be
      read, never a value; each model setting's question names its range, so nobody guesses.
      Defaulting a setting would amend DEC-472 and brief A3 ("each setting empty"), so it waits
      for a DEC. (R-26, R-27)

## C. The cutting pass (one PR, no new rules)

- [x] **One status badge system.** One chip, one width, first in its row, and "Asked you" distinct
      from "Allowed", so "Allowed" only ever means the gate passed it. (R-3, DEC-512)
- [x] **One provenance tag style.** Five short words ("You said", "You entered", "Template",
      "Proposed", "Default") in two shapes, the same on the mandate page and in the set-up summary.
      (R-22, DEC-513)
- [x] **Less fine print.** One "Fixture data" tag per screen: the chart footer, the Positions
      screen and the More sheet dropped theirs. The as-of line and the simulated notes stay as
      disclosures. (R-4, DEC-513)
- [x] **Remove the unbuilt.** No dock, menu, sheet or palette lists a screen that is not built; the
      index pages name what is coming in one line. (R-17, DEC-513)
- [x] **Less chrome.** The breadcrumb folds nothing on top-level screens; the paper badge sits in
      the header and on record screens only; Audit folded into More and Connections off the dock.
      (R-8, DEC-513)
- [ ] **The set-up summary, short first.** The six sentences and the three dollar losses, then
      Create agent; the thirty rows folded under "Everything it will hold to"; the hash in a
      footnote. (R-28) Waits on counsel: the rows are what the passkey confirms, and DEC-477
      flags accepting proposed limits in bulk as compliance question 23.
- [x] **Messages columns.** The rail shows from 80rem, so between 64 and 80rem the thread has the
      room. (R-15, DEC-513)
- [ ] **Phone chart.** No axis on phones; levels as hairlines with the label at the right edge.
      (R-20) Waits on a decision: the axis is a canvas option, and the phone rule allows no script
      media query (DEC-207); either the rule takes a stated exception for canvas or the chart
      clips its axis by CSS.
- [x] **Phone More sheet order.** Approvals and Alerts first, then Search, then the rest; account
      and workspace at the bottom with the theme. (R-21, DEC-513)
- [x] **Needs you holds requests and the agents' conditions.** No feed lines (the strip and
      Alerts carry them); one line per agent and condition, not per instrument. (R-11, DEC-513)
- [x] **The palette's first row.** Go to first, the agents, then Safety with "Stop…" last. (R-18,
      DEC-513)
- [ ] **One request, one place.** An approval appears as a page, a card in the thread and a row
      in Needs you; one representation that the others link to.
- [ ] **Lists are the product.** The agent rows, the approvals and the decisions share one row
      shape: one line, a consistent right column, one height per density, walkable by keyboard.

## D. The state system (DEC)

- [x] **Modes named by what the agent may do:** Trading, Selling only, Paused, Stopped. (DEC-512)
- [x] **The mode chip is a dot and a word**, never a checkbox glyph. (R-8, DEC-512)
- [x] **Restrictions on a row are one line**, with detail on hover and on the agent page, not
      full-sentence pills that triple the row. (R-10, DEC-512)
- [ ] **Three nouns on the owner's screens:** agent, rules, account. Deployment, workspace,
      connection, environment stay in audit and settings.

## E. The owls (DEC)

- [x] **One pixel grid.** Owls draw only at 16, 32, 48, 64 or 80px, where a sprite pixel is whole
      (the 24, 28 and 56px owls are gone); the icons were on the grid already. (R-8, DEC-514)
- [x] **Four frames per owl.** Already shipped (DEC-217, `owl-sprite.ts`): eyes open when
      trading, lidded on selling only, asleep with a "z" when paused, shut when stopped; nothing
      follows P&L. The review missed it because every fixture agent was trading. (R-1)
- [x] **One distinguishing feature per agent.** Already shipped: ears and markings come from the
      agent's ID (`owlShape`), so the silhouettes differ in greyscale.
- [x] **The hatch.** When the runtime records a new agent its owl hatches from an egg once, 600ms;
      under reduced motion it is simply there. (DEC-514)
- [x] **Silence as a feature.** Already shipped: the all-clear is the brand owl and "All clear.
      Nothing needs you."
- [ ] **One era of type on the landing page:** the Pixelify wordmark, not the figlet ASCII. Waits
      on the founder: DEC-467 chose the upright block letters on 2026-10-05.

## F. Home and the agent page (DEC)

- [x] **Needs you in the volt card at every width**, with the deadline as a fixed time, so the
      one thing asking for the owner is the one tinted thing on the screen. (R-2, DEC-515) Still
      open: when something needs the owner, Needs you as the screen itself, the rest quieter.
- [ ] **The mandate becomes the agent page.** The ladder and the headroom as the main column, the
      chart inside it. (R-6) Done: "Equity now" is a marked row, no longer a filled block that read
      as a button. (R-22, DEC-515)
- [ ] **The mandate you can feel.** Scrub the chart and the headroom bars move; drag a limit line
      and a sentence says what would have happened last week under that rule, from the journal,
      never a forecast.
- [x] **Agent-row sparklines** scaled to their own range with the loss limit as the pale volt
      region below the line, rising into view as the agent nears it. (R-5, DEC-515)
- [ ] **The agent's diary.** Activity in the first person, from the journal, deterministic, no
      model.
- [ ] **"Try to change it" on every real record**, with the owner's own data.
- [ ] **The deadline as a sand clock:** a pixel hourglass that is simply there, the same at nine
      minutes and one.
- [x] **The Stop sheet's three account choices at one weight**, outlined rows, with Close
      everything's list behind its confirmation on the record screen. (R-13, DEC-515) Still open:
      the switch-under-a-cover drawing.
- [ ] **Unreachable deployment:** screens that need it greyed; Stop opens straight onto "reach the
      broker directly". (R-12)
- [ ] **The ladder drawn once.** The mandate page's top rail of ticks duplicates the list below
      it. (R-22)
- [ ] **Trust you can test.** "What if I do nothing?" as a line on every request; the Stop sheet
      says what will rest at the broker afterwards; Verify chain runs in front of the owner.
- [ ] **The approval shows the mandate after the fill:** the headroom bars as they would be if
      this order filled (the risk-impact block starts this).
- [ ] **Typing a change shows its consequence:** which rung it moves and whether the gate would
      allow it, before it is confirmed.
- [ ] **The mandate takes shape beside the set-up chat** as each field is understood, so the
      owner watches the rules form rather than reading a summary at the end. (R-7)

## G. Craft

- [ ] **Instant.** A per-route speed budget in CI; prefetch on hover and the dock; the hero number
      in the first HTML, never a skeleton.
- [ ] **Transitions that explain.** The owl and the name travel from the row to the page header;
      a resolved request slides into Resolved. Off under reduced motion.
- [ ] **Motion tuned.** Draw-in 400ms or first visit only; dollar figures roll on action and snap
      on a live tick; the pulsing last point removed; the range pill at 250ms.
- [ ] **Keyboard.** `j`/`k` through requests, `a`/`s` for approve and skip at equal weight, `g h`
      for Home, `?` for the map.
- [ ] **Type.** Optical alignment of the `$` on the hero; a thin space before `%`; prose never
      over 65 characters.
- [ ] **An 8px rhythm with no exceptions**, audited by overlay.
- [ ] **Every state designed:** first run, one agent, twenty, a $12 account, a $4M account, a
      nine-character ticker, a Vietnamese name.
- [ ] **One voice.** Every string read aloud in one sitting; the landing page's voice kept.
- [ ] **Dark mode tuned as a second design:** hairlines lighter, the chart line thinner, the volt
      less neon, the moonlight prints at night. (R-24)
- [ ] **One conversation.** Messages, the copilot and set-up share one composer, one voice, one
      place.
- [ ] **Sound, off by default:** one click for a confirmed approval, one thud for Stop, nothing
      for money.

## H. The website

- [ ] **The desktop is a place.** Windows beside each other; double-click maximises; the taskbar
      switches; Alt+Tab; a real clock; Display Properties changes the wallpaper among the six
      prints; a screensaver after two minutes idle (off under reduced motion).
- [ ] **One live thing.** A real window with a demo paper agent's last five decisions, read-only.
- [ ] **A print page.** `⌘P` on any record gives a clean document with the hash at the foot.
- [ ] **Sign-in carries the owls:** the lineup under the heading of the logon window. (R-23)
- [ ] **The first screen after sign-in** keeps something of the logon window (its bevel, its
      letter), so the landing page and the product read as one place. (R-8)
- [ ] **Tour.mp4** is the one pre-rendered thing in the product and the one place a video tool
      belongs; its re-render with the product's owls is already tracked.

## I. Process and codebase (the founder, 2026-10-08: "let's fix all of it")

The speed has become too slow; the process must not be the bottleneck. Once #669 merges, the
process is revised, and decisions already taken may be revised with it.

- [x] **One branch per plan item** (DEC-516 item 2).
- [ ] **Scenarios as journals.** A fixture scenario is a journal of events from which the
      workspace, the market and the timeline are derived, so a scenario cannot contradict itself;
      the 2%-step test is the tripwire until then.
- [x] **`npm run shots`.** Every listed screen on the fixture scenarios at two widths in both
      themes into `web/.shots/`, with a contact sheet (`index.html`) that puts the golden path
      first. (DEC-516)
- [x] **The tracker on one page.** `08-work-tracker.md` is where things stand; `11-work-log.md`
      is how they got there. (2026-10-08)
- [ ] **A decisions index by area** (web, journal, risk, process) so an agent finds what was
      decided before deciding it again.
- [ ] **A first-load budget per route in CI.** The largest chunk is 906 KB and `static/chunks`
      4.2 MB; measure, then budget: Kumo's reach, Motion, Lightweight Charts only where a chart is.
- [ ] **The real model behind set-up** (DEC-476 item 7). The hero flow runs on a fixture parser
      until the founder decides the model, the route, the spend cap and the data terms.
- [ ] **Three nouns** for the owner's screens: agent, rules, account; the rest stay in audit and
      settings. A vocabulary decision, written once.
- [ ] **Dependabot:** six alerts on `main`, one high.
- [ ] **A screen-reader pass** over the golden path.
- [ ] **A read-aloud copy pass**, cutting a third.
- [x] **A faster process:** [DEC-516](../../docs/project/decisions/DEC-516.md), proposed. Two
      lanes; one item per PR; review from pictures; tests pin invariants; one row and one paragraph
      per session; numbers taken across every branch. Waits on the founder's yes for the light
      lane's merge rule.

## J. From the weekly critique (2026-10-09)

The fixes the first weekly critique proposed, one per screen, most severe first. The evidence is
[critique-2026-10-09.md](critique-2026-10-09.md), cited as C-n. Findings already covered by an
item above (C-3, C-9) are not repeated.

- [ ] **DESIGN.md names the palette that ships.** Its front matter, Colors table and Meaning Rule
      say Ink and Volt (DEC-214), while the product ships Azure and Sun with trend-coloured hero
      lines (DEC-217). DEC-217 is Accepted and supersedes DEC-214, so DESIGN.md is aligned to it
      with no new DEC. (C-1)
- [ ] **Stop above the palette's scrim.** Check that Stop takes a press with the command palette
      open; if not, keep the frame's Stop live as the More sheet does. Safety lane. (C-21)
- [x] **A loud Stop says why.** The unknown-order condition joins Needs you as the drawdown and
      reconciliation conditions do. (C-25, #1057)
- [ ] **A request once on Home.** While a request is open in Needs you, Home's Decisions rail
      leaves out its "Asked you" row. (C-5)
- [ ] **"At the limit:" on the overview card**, so a limit's action never reads as the current
      mode. (C-7)
- [ ] **A time from another day carries its date**, through one formatter shared with the
      timeline. (C-23)
- [ ] **"Who acts: You" says where.** It names, and links to if built, where the owner ends a
      restriction, keeping its step-up. Safety lane. (C-24)
- [ ] **Sparklines without the false limit line.** When the limit is below the line's range, the
      pale sliver alone, with no dashed rule. (C-11)
- [ ] **Re-scope the account chart's blank paint** (section B): blank in light and dark alike, a
      draw race after load, not the mode. (C-22)
- [ ] **Rules by their sentence, not their id.** "low_score" and "large_orders" stay in the record
      and the audit, not on the request, Home or Approvals. Safety lane for the request. (C-6)
- [ ] **Positions on a phone as two-line rows**, so the value and the P&L are on screen. (C-14)
- [ ] **Shots without the scenario panel**, and each scenario captured on the agent it affects.
      Tooling. (C-27, C-28)
- [ ] **The mode chip in the desktop agent header**, on every tab. (C-8)
- [ ] **Owls in the Stop sheet's agent rows.** Safety lane. (C-10)
- [ ] **The Allowed chip keeps its edge in dark mode**, with its fill in the contrast pairs. (C-18)
- [ ] **A labelled composer in set-up**, with a flat send button; still no placeholder or example.
      (C-4)
- [ ] **Alerts says all clear** when every feed answers and no agent is restricted. (C-13)
- [ ] **Day headings on the timeline.** (C-19)
- [ ] **The status strip's state chips outlined**, not ink. (C-26)
- [ ] **Workspace out of More** until one section is built (a DEC amending DEC-513 item 2); its
      subtitle drops "Rules". (C-20)
- [ ] **One left edge across densities** on the audit screens. (C-17)
- [ ] **One paragraph per set of shared rules** on the Approvals rail. (C-12)
- [ ] **The phone thread header without the slug.** (C-16)
- [ ] **The welcome page's buttons above the fold on a phone.** (C-2)
- [ ] **The brand owl never wears an agent's colour.** Waits on the founder: DEC-452 is the
      founder's "choose the color at random". (C-15)

## Learnings (not action items)

- **The cringe test.** Quirk is a consistent world; cringe is a joke the product tells about
  itself. Ask of every idea: would it still work on a bad day with real money?
- **Cut before you add.** The product's weaknesses were duplicates and unbuilt doors, not missing
  features. A cutting pass makes a clean canvas; decoration on a cluttered one is noise.
- **A test pins an invariant, not the look** (DEC-511). Feedback that became a measurement that
  became a test is how the rules file outran the product.
- **A fixture is a story and must be consistent with itself.** The drawdown scenario edited the
  agent's state and left its orders and prices telling the old story; the chart showed the seam.
  `market.test.ts` now holds every scenario to a path.
- **Review from viewport captures, not full-page ones.** A full-page screenshot paints fixed
  chrome (the dock, the scenario panel) mid-page; the artefact looks like a defect.
- **The repository's CI rejects any `Co-authored-by` trailer** (`docs-checks.sh`); commits carry
  the session link only.
- **The set-up flow may never suggest an amount** (DEC-476, DEC-477, pinned by a test): example
  prompts and placeholders are out, however helpful they would be.

## What to refuse

Mascot speech; jokes, exclamation marks, "oops" and "yay"; anything that reacts to money going up
or down; easter eggs that change a setting; pixel art for the data; example amounts in the set-up
(a test forbids them, DEC-476 and DEC-477).

## Order

B, then C and D together (the cutting pass and the state system make one clean canvas), then E
and F (the owls, Home and the agent page, each with its DEC), then G and H as they fit. A, in
parallel, from now.
