# The binding gate's reviewed inputs (DEC-2380 item 5)

- **Spec:** DEC-2380 item 5 (one reading of the gate's reviewed inputs for every process that
  assembles a binding gate input); DEC-470 item 2 and DEC-505 (the reviewed artifacts); DEC-958
  (the live Robinhood fee schedule); trading-domain spec §9 (the gate's configuration).
- **Code:** `mandate-gate-inputs` (`crates/mandate-gate-inputs`): `src/rules.rs` (`artifact_from`,
  `fee_artifact`, `fees`, `rule_set`, `reference`, the config artifacts' exact members),
  `src/mandate.rs` (`gate_mandate`, the mandate's limits as the gate reads them; `reservation`,
  `mandate-accounting`'s fee reservation for one order), `src/reviewed.rs` (`Reviewed::from_registered`, one deployment's reviewed artifacts from its
  confirmed version and effective registrations; `Reviewed::from_admitted`, a research
  deployment's for one admitted instrument, with `control::admitted_configuration`, DEC-2819), `src/view.rs` (`DeployedView`, the executor's mandate and instrument ports for one
  deployment; `fill_refs`, `stop_limit_offset`, `protection_prices`), `src/error.rs` (`InputError`), and `src/control.rs` and `src/approvers.rs`, the control stream's
  confirmed version, effective registrations and approver set, moved whole from the shell, which
  re-exports them as `mandate_shell::control` and `mandate_shell::approvers`. The shell's
  paper assembly (`crates/mandate-shell/src/paper/artifacts.rs`, `gate.rs`) calls them, its reason
  codes unchanged through `From<InputError> for Cause`.
- **Tests:** `crates/mandate-gate-inputs/tests/rules.rs`: the reviewed rule set read whole, each
  member refused when missing, extra, zero or out of range, and both fee schedules held to their
  environment and broker; `crates/mandate-gate-inputs/tests/mandate.rs`: the limits and ladder
  copied field by field, and the fee reservation equal to `mandate-accounting`'s;
  `tests/reviewed.rs`: the registered instrument and objects bound, the Nasdaq listing, and a
  binding from another stream, another model and another instrument refused; `tests/view.rs`:
  fill references, the stop-limit offset only while protection is on, the bracket or a refusal
  without a stop, and the view answering only its agent and instrument; `tests/admitted.rs`: an
  admitted instrument read from its own snapshot, symbol included, and the admitting model's
  registration, an instrument or model unregistered, and a research version reviewed only as
  admitted and a pinned one only as pinned (DEC-2819); `tests/control.rs`,
  `tests/governance.rs` and `tests/registrations.rs`, moved with their module. The shell's own
  paper tests still pin the assembly through its stages.
- **Run:** `cargo nextest run -p mandate-gate-inputs -p mandate-shell`.
