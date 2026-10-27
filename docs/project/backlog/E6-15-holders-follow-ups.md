# E6-15 follow-ups from DEC-2849 item 5 (`mandate_executor::holders`)

- **E6-15u (Should, safety-critical; debt)** As an owner, I want every place the executor works out
  an agent's lots to read them one way, so that the kill switch, protection and sell authority never
  disagree about whose shares are whose. `holders::holders` answers DEC-2849's Lᵢ, but
  `ExecutorState::holds_lots`, `fold::leg_agent` (DEC-160's single holder) and protection's
  `single_holder` and `protectable` still sum an agent's fills their own way. Also: a crypto asset
  fee (`FeesCharged` with `family: crypto_asset`) names no order, so the fold takes it from the
  owner's residual R rather than from the buying agent's lots ([DEC-2850](../decisions/DEC-2850.md)).
  That only tightens: D rises by up to the fee, so every other holder's Sᵢ falls by up to it and
  openings in the instrument hold (I6) until the owner releases the gap. The journal admits only
  `family: equities` today, but the fee must come off the buy before crypto goes live. A replaced
  order's chain is attributed by each order's own `agent`, which DEC-2393 item 1 sets along the
  chain. And while an account switch's close is outstanding in an instrument, any sell fill naming
  no order is read as the close's (I5), so an owner's own sale in that window comes out of the
  agents' lots once R is spent; a broker field that tells the close's fills apart would end that.
  *Accepted when:* those readers call `holders` (or are shown equal to it by a property test), and
  the asset fee comes off the buying agent's lots, with a test.
<!-- section: E6 Agent runtime and risk -->
