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

- [x] DESIGN.md holds principles; the rendering and the history move to `web/design/` (DEC-502,
      #669).
- [ ] **Test sort** (DEC-502 item 5). Loosen look-pinning tests to the invariant they protect,
      remove the ones that only notice the look changed, keep every safety and compliance test
      exactly as it is. Its own PR.
- [ ] **A weekly critique.** Screenshots of every screen; one question: what is the worst thing on
      this screen; fix only that.
- [ ] **A golden path.** Sign in, set up an agent, see it ask, approve, read the record, stop it:
      ten minutes that must be perfect on every release.
- [ ] **Five people, watched.** Twenty minutes each, say nothing, write down where they hesitate.

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
- [ ] **Landing windows.** Open beside the last rather than on top; "Try it." gets a button;
      Questions shows its scrollbar. (R-30)
- [ ] **Set-up replies say what they read** and, on a second miss, what shape of answer would
      work; lookback and z-score default with "default" marked. (R-26, R-27)

## C. The cutting pass (one PR, no new rules)

- [ ] **One status badge system.** One style, a fixed column, and "Waiting for you" distinct from
      "Allowed", so "Allowed" only ever means the gate passed it. (R-3)
- [ ] **One provenance tag style.** "You said", "You entered", "From template", "Proposed by the
      platform", "Platform default" are one idea in four styles. (R-22)
- [ ] **Less fine print.** One status line per screen for the as-of time, the simulated note and
      the fixture tag; required disclosures untouched. (R-4)
- [ ] **Remove the unbuilt.** Every "next slice" page and row out of the dock, the palette and the
      More sheet until it exists. (R-17)
- [ ] **Less chrome.** The breadcrumb gone on top-level screens; the paper badge in the header
      only, not in every title; Audit and Connections into More on desktop as on the phone. (R-8)
- [ ] **The set-up summary, short first.** The six sentences and the three dollar losses, then
      Create agent; the thirty rows folded under "Everything it will hold to"; the hash in a
      footnote. (R-28)
- [ ] **Messages columns.** The thread is the widest column; the rail only above about 80rem.
      (R-15)
- [ ] **Phone chart.** No axis on phones; levels as hairlines with the label at the right edge.
      (R-20)
- [ ] **Phone More sheet order.** Approvals and Alerts first, then Search, then the rest; account
      and workspace at the bottom with the theme. (R-21)
- [ ] **Needs you holds requests only.** Degraded feeds go to the degraded bar; one line per
      agent, not per instrument. (R-11)

## D. The state system (DEC)

- [ ] **Modes named by what the agent may do:** Trading, Selling only, Paused, Stopped.
- [ ] **The mode chip is a dot and a word**, never a checkbox glyph. (R-8)
- [ ] **Restrictions live in the chip**, with detail on hover and on the agent page, not as
      full-sentence pills that triple the row. (R-10)
- [ ] **Three nouns on the owner's screens:** agent, rules, account. Deployment, workspace,
      connection, environment stay in audit and settings.

## E. The owls (DEC)

- [ ] **One pixel grid** for the owls, the icons, the info symbol and the hourglass; snap every
      icon box to whole pixels. (R-8)
- [ ] **Four frames per owl:** awake, watching (stale feed), asleep (paused, with the "z"),
      stopped (eyes shut, greyed). One frame change per state, never a loop; the face never
      changes with P&L. (R-1)
- [ ] **One distinguishing feature per agent** (a tuft, a brow, a belly) so silhouettes tell them
      apart in greyscale.
- [ ] **The hatch.** Create agent ends with the owl hatching once, 600ms, then on its branch.
- [ ] **Silence as a feature.** The all-clear on Home is the owl asleep and "Nothing needs you",
      and nothing else.
- [ ] **One era of type on the landing page:** the Pixelify wordmark, not the figlet ASCII.

## F. Home and the agent page (DEC)

- [ ] **Home answers one question.** Needs you is the screen when something needs the owner, in
      the phone's volt card at every width, the deadline as a fixed time; the money, the agents
      and the decisions below, quieter. (R-2)
- [ ] **The mandate becomes the agent page.** The ladder and the headroom as the main column, the
      chart inside it; "Equity now" no longer drawn as a button. (R-6, R-22)
- [ ] **The mandate you can feel.** Scrub the chart and the headroom bars move; drag a limit line
      and a sentence says what would have happened last week under that rule, from the journal,
      never a forecast.
- [ ] **Agent-row sparklines** scaled to their own range with the loss limit as a shaded band.
      (R-5)
- [ ] **The agent's diary.** Activity in the first person, from the journal, deterministic, no
      model.
- [ ] **"Try to change it" on every real record**, with the owner's own data.
- [ ] **The deadline as a sand clock:** a pixel hourglass that is simply there, the same at nine
      minutes and one.
- [ ] **The Stop sheet as a physical object:** the three choices at one visual weight, Pause as
      the cover, Stop as the switch, the bullets of Close everything behind its confirmation.
      (R-13)
- [ ] **Unreachable deployment:** screens that need it greyed; Stop opens straight onto "reach the
      broker directly". (R-12)

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

## What to refuse

Mascot speech; jokes, exclamation marks, "oops" and "yay"; anything that reacts to money going up
or down; easter eggs that change a setting; pixel art for the data; example amounts in the set-up
(a test forbids them, DEC-476 and DEC-477).

## Order

B, then C and D together (the cutting pass and the state system make one clean canvas), then E
and F (the owls, Home and the agent page, each with its DEC), then G and H as they fit. A, in
parallel, from now.
