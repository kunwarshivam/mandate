//! E8-10 slice D10b (DEC-2504 items 4 and 5, DEC-2506 item 7, notifications spec v0.8 §2 "Web push
//! is off in the managed cell", §4.6; journal spec v0.51 §9.15, DEC-2505): a step over an audience
//! that names web push makes no web-push attempt and journals none, whether the process's switch
//! withholds those pairs or not, since a version-3 web-push attempt names one address and the
//! per-address fan-out is not built; every recipient is still listed on each notice, and every
//! other channel is sent as before. The oracles are the provider's own captures and the notice
//! stream's committed rows.

use super::{A1, Checked, alerts, payloads, provider_for, run_over, text, world, writer};
use crate::WebPushSwitch;
use mandate_canon::Value;
use mandate_notify::PushChannel;

/// The founder on email and web push, and a second recipient whose only channel is web push.
const WITH_WEB_PUSH: [(&str, PushChannel); 3] = [
    ("founder", PushChannel::Email),
    ("founder", PushChannel::WebPush),
    ("watcher", PushChannel::WebPush),
];

#[test]
fn a_step_makes_no_web_push_attempt_and_journals_none_whatever_the_switch_says() -> Checked {
    for switch in [WebPushSwitch::Off, WebPushSwitch::On] {
        let journal = world()?;
        alerts(&journal, A1, 1, &[(1, "risk_limit"), (2, "agent_held")])?;
        let (writer, mut sends) = (writer(&journal)?, provider_for(&journal, &WITH_WEB_PUSH)?);
        let audience = switch.sends(&WITH_WEB_PUSH)?;
        run_over(&mut journal.clone(), &writer, &[A1], &audience, &mut sends)??;
        let web_push = PushChannel::WebPush.key()?;
        let email = PushChannel::Email.key()?;
        let sent: Vec<&str> = sends.seen.iter().map(|(_, c, _)| c.as_str()).collect();
        assert!(!sent.is_empty(), "{switch:?}: the email sends still go");
        assert!(
            sent.iter().all(|c| *c == email),
            "{switch:?}: no web-push send: {sent:?}"
        );
        let issued = payloads(&journal, "NoticeIssued");
        let attempted = payloads(&journal, "NoticeAttempted");
        let channels: Vec<String> = attempted.iter().map(|p| text(p, "channel")).collect();
        assert_eq!(
            channels.len(),
            issued.len(),
            "{switch:?}: one email attempt per notice, and nothing else"
        );
        assert!(
            channels.iter().all(|c| c != web_push),
            "{switch:?}: no web-push attempt is journaled: {channels:?}"
        );
        assert!(
            attempted
                .iter()
                .all(|p| p.get("address_ref") == Some(&Value::Null)),
            "{switch:?}: every attempt names no address"
        );
        if switch == WebPushSwitch::On {
            let listed = |p: &Value| {
                p.get("recipients").is_some_and(|r| {
                    *r == Value::Array(vec![text_value("founder"), text_value("watcher")])
                })
            };
            assert!(
                issued.iter().all(listed),
                "the web-push-only recipient is still listed on each notice"
            );
        }
    }
    Ok(())
}

fn text_value(value: &str) -> Value {
    Value::Str(value.to_owned())
}
