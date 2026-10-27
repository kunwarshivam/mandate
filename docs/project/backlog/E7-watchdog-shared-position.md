- **E7-watchdog-shared-position (Proposed, ranked high; trading core; lane X)** As an owner, I
  want the triggered-stop watchdog to exit a position several of my agents hold, so that a stop
  that triggered without a fill never leaves that position unprotected.
  - **Today:** the watchdog's exit names the position's single holder, or none (`*`) where several
    agents hold it (trading spec §5.4). Its `IntentReceived` must name a `mandate_version`
    (journal spec §9), and the executor process resolves none for `*`. Under DEC-2846 item 8 the
    exit waits, alerted once, where before the tick failed and the process stopped.
  - **The question:** a shared position's watchdog could split into one exit per holder, each
    under that agent's references, or journal under the account's references alone.

  *Accepted when:* the trading and journal specs say how a shared position's watchdog exit is
  journaled, the executor raises it, and a judged executord test shows it sold while two agents
  hold the position.
