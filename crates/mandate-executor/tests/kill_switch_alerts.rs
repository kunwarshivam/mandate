//! E8-9k (notifications spec §3.4, DEC-2514 item 2): a user's kill switch is one notice, the
//! `OwnerAlertSent` the API commits with its `OwnerCommandIssued`, so the executor applying it
//! raises none; every other switch it applies, a risk limit's or the platform operator's, it alerts
//! exactly once, journaled beside the switch. Both scopes, every initiator.

mod common;

use common::{FixedInstruments, FixedMandate, Shell, TestIds, agent, config, ports};
use mandate_canon::Value;
use mandate_executor::{AccountRef, Command, Effect, Initiator, Input, KillScope};

/// A started executor on an opened stream, reconciled, as a production shell starts.
fn opened(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

#[test]
fn the_executor_alerts_every_switch_but_a_users_exactly_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let scopes = [
        KillScope::Agent(agent(common::AGENT)),
        KillScope::Account(AccountRef(common::ACCOUNT.to_owned())),
    ];
    let mut failed = Vec::new();
    for scope in &scopes {
        for initiator in [
            Initiator::Owner,
            Initiator::RiskLimit,
            Initiator::PlatformOperator,
        ] {
            let mut shell = opened(&ports);
            let ran = shell.run(
                Input::Command(Command::KillSwitch {
                    scope: scope.clone(),
                    initiator,
                    confirmation: None,
                    cause: None,
                }),
                &ports,
            );
            let switch = ran
                .draft("KillSwitchActivated")
                .expect("the switch is journaled");
            let raised: Vec<_> = ran
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Notify(alert) => Some((alert.subject_event.clone(), alert.kind.key())),
                    _ => None,
                })
                .collect();
            let recorded: Vec<_> = ran
                .drafts
                .iter()
                .filter(|draft| draft.event_type == "OwnerAlertSent")
                .map(|draft| {
                    (
                        draft.causation_id.clone(),
                        draft.payload.get("kind").cloned(),
                    )
                })
                .collect();
            let want = usize::from(initiator != Initiator::Owner);
            let named = raised
                .iter()
                .all(|(subject, kind)| *subject == switch.event_id && *kind == "kill_switch")
                && recorded.iter().all(|(cause, kind)| {
                    cause.as_ref() == Some(&switch.event_id)
                        && *kind == Some(Value::Str("kill_switch".to_owned()))
                });
            if raised.len() != want || recorded.len() != want || !named {
                failed.push(format!(
                    "{scope:?} {initiator:?}: want {want}, raised {raised:?}, recorded {recorded:?}"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
