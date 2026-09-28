//! The shell's mappings from a stage's answer to what the path does next. Each is a total function
//! whose only permitting arm reads a permitting **answer**: no error, absence, or ambiguity maps to
//! a verdict that permits an order (TI-3). There is no wildcard arm in this module, so a new
//! variant in any upstream type fails to compile until it is mapped here.

use mandate_accounting::InstrumentId;
use mandate_backtest::Signal;
use mandate_canon::{Key, Object, Value};
use mandate_executor::{BrokerOutcome, ConnectorError};
use mandate_risk::{CheckOutcome, Decision, Verdict};
use mandate_runtime::{Autonomy, DryRunVerdict, ModelOutput, Proposal, Purpose, RiskClock};

use crate::error::{Cause, ShellError};
use crate::stages::{ModelRef, Stage};

/// The reason code of an opening `Allow` that carries a check the gate never reached (TI-11).
pub const CHECK_NOT_REACHED: &str = "check_not_reached";

/// The advisory verdict for the gate's answer on a proposal of `purpose`.
///
/// An error is a denial carrying the error's own code, so the gate's
/// `Err(GateError::Unimplemented(..))` for an opening while a check is owed (DEC-129 item 29) is
/// carried through unsoftened. An opening `Allow` whose `checks` hold any `NotReached` is refused
/// too, read from the gate's own output with no list of which story owns which check (TI-11).
pub fn verdict_of(answer: &Result<Decision, Cause>, purpose: Purpose) -> DryRunVerdict {
    match answer {
        Err(cause) => DryRunVerdict::Deny {
            reason_code: cause.code().to_owned(),
        },
        Ok(decision) => decided(decision, purpose),
    }
}

/// The advisory verdict for a decision the gate reached.
///
/// A `Defer` or a `Hold` is the gate pacing or holding an **exit**, never denying one; the dry run
/// can only narrow, so narrowing an exit away here would make an advisory pass the reason an exit
/// did not go out, which `AGENTS.md` rule 13 forbids. The binding gate paces it. On an opening,
/// neither permits.
pub fn decided(decision: &Decision, purpose: Purpose) -> DryRunVerdict {
    let opening = purpose.adds_risk();
    match decision.verdict {
        Verdict::Allow => {
            if opening && decision.checks.iter().any(is_not_reached) {
                deny(CHECK_NOT_REACHED)
            } else {
                DryRunVerdict::Allow
            }
        }
        Verdict::Deny => deny(reason_of(decision)),
        Verdict::Defer | Verdict::Hold => {
            if opening {
                deny(reason_of(decision))
            } else {
                DryRunVerdict::Allow
            }
        }
    }
}

fn is_not_reached(outcome: &CheckOutcome) -> bool {
    match outcome {
        CheckOutcome::NotReached(_) => true,
        CheckOutcome::Passed(_) | CheckOutcome::Failed(..) => false,
    }
}

fn reason_of(decision: &Decision) -> &'static str {
    match decision.reason {
        Some(reason) => reason.as_str(),
        None => "gate_gave_no_reason",
    }
}

fn deny(reason_code: &str) -> DryRunVerdict {
    DryRunVerdict::Deny {
        reason_code: reason_code.to_owned(),
    }
}

/// The classification for the builder's answer: an error classifies `Deny`.
pub fn autonomy_of(answer: &Result<Autonomy, Cause>) -> Autonomy {
    match answer {
        Ok(autonomy) => *autonomy,
        Err(_) => Autonomy::Deny,
    }
}

/// The proposal the runtime receives for the builder's answer, and the refusal the run reports
/// when there is none. A hold (`Ok(None)`) is an answer, so it is not a refusal of the builder; a
/// proposal of quantity zero is structurally impossible and refused (PB-14). A quantity above the
/// mandate's caps is **not** refused here: that is a limit, and limits are the gate's (DEC-138
/// item 3).
pub fn proposal_of(
    answer: Result<Option<Proposal>, Cause>,
) -> (Option<Proposal>, Option<ShellError>) {
    match answer {
        Err(cause) => (
            None,
            Some(ShellError::Refused {
                stage: Stage::Size,
                cause,
            }),
        ),
        Ok(None) => (None, None),
        Ok(Some(proposal)) => {
            if proposal.qty.is_zero() {
                (None, Some(ShellError::ProposalInvalid))
            } else {
                (Some(proposal), None)
            }
        }
    }
}

/// The executor's input for one connector answer (task brief, `Stage::Executor`'s third mode).
///
/// An unknown outcome goes to the executor as `Input::Broker(Err(..))`, which makes it query and
/// never resubmit. An answer the connector could not read, or a request it never sent, is **not**
/// an unknown outcome: handing it on would ask the executor to interpret what nobody could, so it
/// stops the run instead (DEC-85).
pub fn broker_input(
    answer: Result<BrokerOutcome, ConnectorError>,
) -> Result<mandate_executor::Input, Cause> {
    match answer {
        Ok(outcome) => Ok(mandate_executor::Input::Broker(Ok(outcome))),
        Err(error) => match error.as_unknown() {
            Some(unknown) => Ok(mandate_executor::Input::Broker(Err(unknown))),
            None => Err(Cause::Connector(error)),
        },
    }
}

/// The model output a signal becomes (task brief step 4). Only `Long` opens: `Flat` while flat and
/// `Undecided` produce no output at all (PB-7). A `Long` is conviction 1 at confidence 1, the whole
/// of a single binary model's say (DEC-157 item 4); the builder does the sizing.
pub fn model_output(
    signal: Signal,
    model: &ModelRef,
    instrument: &InstrumentId,
    now: RiskClock,
) -> Result<ModelOutput, Cause> {
    match signal {
        Signal::Long => {
            let expires = now
                .secs()
                .checked_add(model.max_output_age_s)
                .ok_or(Cause::Absent {
                    what: "the output's expiry is past the risk clock's range",
                })?;
            Ok(ModelOutput {
                model: model.id.clone(),
                version: model.version.clone(),
                instrument: instrument.clone(),
                as_of: now,
                expires_at: RiskClock::from_secs(expires),
                content: long_content()?,
            })
        }
        Signal::Flat | Signal::Undecided => Err(Cause::NotLong(signal)),
    }
}

fn long_content() -> Result<Value, Cause> {
    let mut content = Object::new();
    for (name, value) in [("confidence", "1"), ("conviction", "1")] {
        let key = Key::new(name).map_err(|_| Cause::Absent {
            what: "a model output field could not be named",
        })?;
        content.insert(key, Value::Str(value.to_owned()));
    }
    Ok(Value::Object(content))
}

/// The name an autonomy is journaled and reported under.
pub fn autonomy_name(autonomy: Autonomy) -> &'static str {
    match autonomy {
        Autonomy::Auto => "auto",
        Autonomy::Ask => "ask",
        Autonomy::Deny => "deny",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mandate_backtest::{BacktestError, Signal};
    use mandate_builder::BuilderError;
    use mandate_executor::{BrokerOutcome, BrokerUnknown, ConnectorError, ExecutorError};
    use mandate_num::{Price, Qty};
    use mandate_risk::{
        Check, CheckOutcome, Computed, Decision, GateError, Purpose as GatePurpose, ReasonCode,
        Verdict,
    };
    use mandate_runtime::{Autonomy, DryRunVerdict, Proposal, Purpose, RiskClock, SinkError};
    use mandate_spec::SpecError;
    use proptest::prelude::*;

    use super::{
        CHECK_NOT_REACHED, autonomy_of, broker_input, decided, model_output, proposal_of,
        verdict_of,
    };
    use crate::error::{Cause, ShellError};
    use crate::stages::ModelRef;

    const CHECKS: [Check; 8] = [
        Check::AccountAndMode,
        Check::UniverseAndLimits,
        Check::SessionAndHalt,
        Check::OrderConstraints,
        Check::MarkAndCollar,
        Check::ConductControls,
        Check::BuyingPowerAndExposure,
        Check::DayTradeBudget,
    ];

    const PURPOSES: [Purpose; 7] = [
        Purpose::Open,
        Purpose::Increase,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ];

    /// One value of every source error type the shell maps, built fresh each time because the
    /// gate's error is neither `Clone` nor `PartialEq`.
    fn every_cause() -> Vec<Cause> {
        vec![
            Cause::Unimplemented { story: "E7-7" },
            Cause::Spec(SpecError::Unimplemented),
            Cause::Spec(SpecError::ClockWentBackwards),
            Cause::Backtest(BacktestError::NoBars),
            Cause::Backtest(BacktestError::StrategyWindowsCrossed),
            Cause::Builder(BuilderError::Unimplemented),
            Cause::Builder(BuilderError::CrossedQuote),
            Cause::Gate(GateError::Unimplemented("evaluate", "E6-6")),
            Cause::Gate(GateError::WorkingUniverseUnavailable),
            Cause::Gate(GateError::RiskStateStale),
            Cause::Gate(GateError::QuoteUnsane),
            Cause::Gate(GateError::InstrumentUnknown),
            Cause::GateAnswered {
                reason_code: "max_order_size".to_owned(),
            },
            Cause::Executor(ExecutorError::Unimplemented { story: "E7-4" }),
            Cause::Connector(ConnectorError::Unknown(BrokerUnknown::Timeout)),
            Cause::Connector(ConnectorError::Unreadable { code: "wire" }),
            Cause::Connector(ConnectorError::NotSent {
                code: "unimplemented",
            }),
            Cause::Sink(SinkError::Unavailable),
            Cause::Sink(SinkError::Refused {
                reason: "closed".to_owned(),
            }),
            Cause::Append { outcome: "Fenced" },
            Cause::NotLong(Signal::Flat),
            Cause::NotLong(Signal::Undecided),
            Cause::NotAuto { autonomy: "ask" },
            Cause::Absent { what: "gap" },
            Cause::Poisoned("unimplemented".to_owned()),
        ]
    }

    fn decision(
        verdict: Verdict,
        reason: Option<ReasonCode>,
        checks: Vec<CheckOutcome>,
    ) -> Decision {
        Decision {
            verdict,
            reason,
            purpose: GatePurpose::Open,
            pacing: None,
            checks,
            computed: Computed::default(),
        }
    }

    fn outcome_strategy() -> impl Strategy<Value = CheckOutcome> {
        let check = || proptest::sample::select(CHECKS.to_vec());
        prop_oneof![
            check().prop_map(CheckOutcome::Passed),
            (check(), proptest::sample::select(ReasonCode::ALL.to_vec()))
                .prop_map(|(c, r)| CheckOutcome::Failed(c, r)),
            check().prop_map(CheckOutcome::NotReached),
        ]
    }

    fn verdict_strategy() -> impl Strategy<Value = Verdict> {
        prop_oneof![
            Just(Verdict::Allow),
            Just(Verdict::Deny),
            Just(Verdict::Defer),
            Just(Verdict::Hold)
        ]
    }

    #[test]
    fn no_source_error_maps_to_an_allow_for_any_purpose() {
        for purpose in PURPOSES {
            for cause in every_cause() {
                let code = cause.code().to_owned();
                let verdict = verdict_of(&Err(cause), purpose);
                assert_eq!(
                    verdict,
                    DryRunVerdict::Deny { reason_code: code },
                    "{purpose:?}"
                );
            }
        }
    }

    #[test]
    fn the_gates_unimplemented_opening_is_carried_through_unsoftened() {
        let verdict = verdict_of(
            &Err(Cause::Gate(GateError::Unimplemented("evaluate", "E6-6"))),
            Purpose::Open,
        );
        assert_eq!(
            verdict,
            DryRunVerdict::Deny {
                reason_code: "unimplemented".to_owned()
            }
        );
    }

    #[test]
    fn no_source_error_classifies_anything_but_deny() {
        for cause in every_cause() {
            assert_eq!(autonomy_of(&Err(cause)), Autonomy::Deny);
        }
        for autonomy in [Autonomy::Auto, Autonomy::Ask, Autonomy::Deny] {
            assert_eq!(autonomy_of(&Ok(autonomy)), autonomy);
        }
    }

    #[test]
    fn no_source_error_proposes_anything() {
        for cause in every_cause() {
            let (proposal, refusal) = proposal_of(Err(cause));
            assert_eq!(proposal, None);
            assert!(
                matches!(
                    refusal,
                    Some(ShellError::Refused {
                        stage: crate::Stage::Size,
                        ..
                    })
                ),
                "{refusal:?}"
            );
        }
    }

    fn proposal(qty: &str) -> Result<Proposal, String> {
        Ok(Proposal {
            instrument: mandate_accounting::InstrumentId::new("AAPL").map_err(|e| e.to_string())?,
            side: mandate_accounting::Side::Buy,
            qty: Qty::parse(qty).map_err(|e| e.to_string())?,
            limit: Price::parse("255.2").map_err(|e| e.to_string())?,
            purpose: Purpose::Open,
            combined_score: mandate_canon::Value::Str("1".to_owned()),
        })
    }

    #[test]
    fn a_proposal_of_quantity_zero_is_refused_and_a_hold_is_not() -> Result<(), String> {
        let (none, refusal) = proposal_of(Ok(Some(proposal("0")?)));
        assert_eq!(none, None);
        assert!(
            matches!(refusal, Some(ShellError::ProposalInvalid)),
            "{refusal:?}"
        );
        let (hold, no_refusal) = proposal_of(Ok(None));
        assert_eq!(hold, None);
        assert!(no_refusal.is_none());
        let one = proposal("1")?;
        let (passed, none_refused) = proposal_of(Ok(Some(one.clone())));
        assert_eq!(passed, Some(one));
        assert!(none_refused.is_none());
        Ok(())
    }

    #[test]
    fn an_unknown_outcome_queries_and_an_uninterpretable_one_stops() {
        for unknown in [
            BrokerUnknown::Timeout,
            BrokerUnknown::Ambiguous,
            BrokerUnknown::Transport,
        ] {
            let input = broker_input(Err(ConnectorError::Unknown(unknown)));
            assert!(
                matches!(input, Ok(mandate_executor::Input::Broker(Err(u))) if u == unknown),
                "{input:?}"
            );
        }
        for error in [
            ConnectorError::Unreadable { code: "wire" },
            ConnectorError::NotSent {
                code: "unimplemented",
            },
        ] {
            let input = broker_input(Err(error));
            assert!(
                matches!(input, Err(Cause::Connector(e)) if e == error),
                "{input:?}"
            );
        }
        let absent = BrokerOutcome::Absent {
            client_order_id: "md-x".to_owned(),
        };
        let input = broker_input(Ok(absent.clone()));
        assert!(
            matches!(input, Ok(mandate_executor::Input::Broker(Ok(ref o))) if *o == absent),
            "{input:?}"
        );
    }

    fn model() -> ModelRef {
        ModelRef {
            id: "quant.ma_crossover".to_owned(),
            version: "1.0.0".to_owned(),
            max_output_age_s: 86_400,
            params: BTreeMap::new(),
        }
    }

    #[test]
    fn only_a_long_signal_becomes_a_model_output() -> Result<(), String> {
        let instrument =
            mandate_accounting::InstrumentId::new("AAPL").map_err(|e| e.to_string())?;
        let now = RiskClock::from_secs(1_790_000_000);
        for signal in [Signal::Flat, Signal::Undecided] {
            let output = model_output(signal, &model(), &instrument, now);
            assert!(
                matches!(output, Err(Cause::NotLong(s)) if s == signal),
                "{output:?}"
            );
        }
        let output =
            model_output(Signal::Long, &model(), &instrument, now).map_err(|e| e.to_string())?;
        assert_eq!(output.model, "quant.ma_crossover");
        assert_eq!(output.version, "1.0.0");
        assert_eq!(output.as_of, now);
        assert_eq!(output.expires_at, RiskClock::from_secs(1_790_086_400));
        assert_eq!(
            output
                .content
                .get("conviction")
                .and_then(mandate_canon::Value::as_str),
            Some("1")
        );
        assert_eq!(
            output
                .content
                .get("confidence")
                .and_then(mandate_canon::Value::as_str),
            Some("1")
        );
        let overflow = model_output(
            Signal::Long,
            &model(),
            &instrument,
            RiskClock::from_secs(i64::MAX),
        );
        assert!(
            matches!(overflow, Err(Cause::Absent { .. })),
            "{overflow:?}"
        );
        Ok(())
    }

    #[test]
    fn a_reducing_defer_or_hold_passes_the_advisory_pass_and_an_opening_one_does_not() {
        for verdict in [Verdict::Defer, Verdict::Hold] {
            let d = decision(verdict, Some(ReasonCode::SessionNotAllowed), Vec::new());
            assert_eq!(
                decided(&d, Purpose::DiscretionaryExit),
                DryRunVerdict::Allow
            );
            assert_eq!(
                decided(&d, Purpose::Open),
                DryRunVerdict::Deny {
                    reason_code: "session_not_allowed".to_owned()
                }
            );
        }
        let reasonless = decision(Verdict::Deny, None, Vec::new());
        assert_eq!(
            decided(&reasonless, Purpose::RiskExit),
            DryRunVerdict::Deny {
                reason_code: "gate_gave_no_reason".to_owned()
            }
        );
    }

    proptest! {
        /// TI-3: every decision the gate can return maps to a permitting verdict only when the
        /// gate itself allowed it, or deferred or held an exit. The expectation is computed from
        /// the decision's fields, not from the mapping's own predicate.
        #[test]
        fn a_mapped_verdict_permits_only_what_the_gate_permitted(
            verdict in verdict_strategy(),
            checks in proptest::collection::vec(outcome_strategy(), 0..9),
            purpose in proptest::sample::select(PURPOSES.to_vec()),
        ) {
            let d = decision(verdict, Some(ReasonCode::MaxOrderSize), checks.clone());
            let opening = matches!(purpose, Purpose::Open | Purpose::Increase);
            let owed = checks.iter().filter(|c| matches!(c, CheckOutcome::NotReached(_))).count();
            let permitted = match verdict {
                Verdict::Allow => !opening || owed == 0,
                Verdict::Deny => false,
                Verdict::Defer | Verdict::Hold => !opening,
            };
            prop_assert_eq!(decided(&d, purpose) == DryRunVerdict::Allow, permitted);
        }

        /// TI-11: an opening `Allow` carrying any `NotReached` is refused with its own code,
        /// wherever in `checks` it sits, and whichever check it names.
        #[test]
        fn an_opening_allow_with_a_check_not_reached_is_refused(
            before in proptest::collection::vec(outcome_strategy(), 0..4),
            owed in proptest::sample::select(CHECKS.to_vec()),
            after in proptest::collection::vec(outcome_strategy(), 0..4),
            opening in prop_oneof![Just(Purpose::Open), Just(Purpose::Increase)],
        ) {
            let mut checks = before;
            checks.push(CheckOutcome::NotReached(owed));
            checks.extend(after);
            let d = decision(Verdict::Allow, None, checks);
            prop_assert_eq!(
                decided(&d, opening),
                DryRunVerdict::Deny { reason_code: CHECK_NOT_REACHED.to_owned() }
            );
        }
    }
}
