# E6-15 follow-ups from DEC-2849 item 5 (`mandate_executor::holders`)

- **E6-15u (Should, safety-critical; debt)** As an owner, I want every place the executor works out
  an agent's lots to read them one way, so that the kill switch, protection and sell authority never
  disagree about whose shares are whose. `holders::holders` answers DEC-2849's Lᵢ, but
  `ExecutorState::holds_lots`, `fold::leg_agent` (DEC-160's single holder) and protection's
  `single_holder` and `protectable` still sum an agent's fills their own way. Also: a crypto asset
  fee (`FeesCharged` with `family: crypto_asset`) names no order, so the fold takes it from the
  owner's residual R rather than from the buying agent's lots as DEC-2849's definition of Lᵢ says.
  That only tightens (it can raise D, never Sᵢ), and the journal admits only `family: equities`
  today, but it must be attributed to the buy before crypto goes live; and a replaced order's chain
  is attributed by each order's own `agent`, which DEC-2393 item 1 sets along the chain.
  *Accepted when:* those readers call `holders` (or are shown equal to it by a property test), and
  the asset fee comes off the buying agent's lots, with a test.
<!-- section: E6 Agent runtime and risk -->
