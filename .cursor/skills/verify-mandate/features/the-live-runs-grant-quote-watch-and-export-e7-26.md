# The live run's grant, quote cross-check, watch and export (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1b, LT-4, LT-10, the founder's-run
  steps 4 to 6); `docs/project/decisions/DEC-529.md` items 1 and 12;
  `docs/project/decisions/DEC-912.md`; mandate spec §6.1, §6.4; trading-domain spec §4.2, §5.4,
  §9.6; journal spec §2, §9.13, §12.
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/grant.rs` (`Asked`, `GrantRefusal`, `grant` — LT-4's door: the `ask`
  opening submits only after an answer whose content hash equals the request's, any difference a
  refusal with nothing sent, the answers read in journal order, the timeout a deny), re-exported
  by `crates/mandate-paper/src/lib.rs` as `PaperError`'s `Grant` variant;
  `crates/mandate-paper/src/quote.rs` (`QuoteRefusal`, `cross_check` — DEC-529 item 12: the
  broker's ask agrees with the independent IEX ask within the collar's own aggressiveness or the
  opening is refused before any submission, a missing quote refused alike), re-exported as the
  `CrossCheck` variant; `crates/mandate-paper/src/lt10.rs` (`Watched`, `Lt10Refusal`, `lt10` —
  LT-10: the run ends on a terminal entry and a covered or flat position, or an uncovered one
  only once the unprotected interval's alert key is in the run's report, and the reads fail
  closed past the session's cap, never one more), re-exported as the `Lt10` variant;
  `crates/mandate-paper/src/evidence.rs` (`Evidence`, `EvidenceRefusal`, `evidence` — the
  founder's run's step 6: the agent, account and control streams the run's `journal export`
  reads, named as journal spec §2 names them, refused when the run kept no journal), re-exported
  as the `Evidence` variant. The stubs are DEC-77 stubs; the seams are pure and hold no host,
  credential or order.
- **Tests:** `crates/mandate-paper/tests/live_run.rs`: the grant pins (an unanswered ask holding
  the submit before its deadline; a grant repeating the request's hash releasing it; an answer
  repeating another hash refusing with nothing sent, whatever its verdict; the first answer of
  the ask deciding it whatever rows follow — the journal order, with a command row that is not
  an answer consulting nothing; a skip denying; a silent or late ask denying at its deadline;
  an answer of another approval never releasing this one; an answer the run cannot read refusing
  as a difference), the cross-check pins (a broker quote within the collar proceeding; one
  beyond it in either direction refusing before any submission; a missing quote of either side
  refusing; the collar's own aggressiveness deciding the agreement — the same reads agreeing at
  2% and refusing at 1%; the band's ceiling and floor each rounding inward at the nine places a
  price holds, as `Price::collar_bound` rounds them — a price on the inward-rounded ceiling or
  floor proceeding and one grid step past either refusing), the
  watch pins (a terminal covered or flat entry ending the run; an uncovered quantity keeping it
  reading; an uncovered quantity ending only with its alert key reported; another interval's
  alert key — `stop_watchdog`, the other key of the notifications spec §4.2 `protection` row —
  holding the run open; an entry still working keeping it reading; the reads failing closed
  past the cap, a settled watch ending clean at it), the export pins (a placing run holding the
  three streams its export reads, each a journal-spec §2 stream id of its type; the stream names
  deriving from the run's own workspace, agent and account ref; a run keeping no journal
  exporting nothing), and a live test that pins every refusal's message and reason code, the
  stub's own report with them (DEC-137).
- **Run:** `cargo nextest run -p mandate-paper` (the live suite) and `cargo nextest run -p
  mandate-paper --run-ignored ignored-only --no-fail-fast` (the pending pins, each failing at its
  stub until E7-26's implementation lands).
