- **E7-watchdog-shared-position (Proposed, ranked high; trading core; lane X)** As an owner, I
  want the triggered-stop watchdog to exit a position several of my agents hold, so that a stop
  that triggered without a fill never leaves that position unprotected.
  - **Today:** the watchdog's exit names the position's single holder, or none (`*`) wherever the
    position is not exactly one agent's lots: several agents' lots, or the owner's own or released
    shares beside one agent's (trading spec §5.4, `single_holder`). Its `IntentReceived` must name a `mandate_version`
    (journal spec §9), and the executor process resolves none for `*`. Under DEC-2846 item 8 the
    exit waits, alerted once, where before the tick failed and the process stopped.
  - **The founder's direction (2026-10-27):** one exit per holding agent, each sized to that
    agent's own lots (DEC-2849's per-agent position), never the owner's or another holder's shares.

  *Accepted when:* the trading and journal specs say how a shared position's watchdog exit is
  journaled, the executor raises it, and judged executord tests show it sold while two agents
  hold the position and while the owner's shares sit beside one agent's.
