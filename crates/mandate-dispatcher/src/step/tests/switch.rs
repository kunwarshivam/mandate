//! E8-10 slice D10b (DEC-2504 item 4, DEC-2506 item 7, notifications spec v0.8 §2 "Web push is off
//! in the managed cell", §4.6): while web push is off, a step over an audience that names web push
//! makes no web-push attempt and journals none, and every other channel is sent as before. The
//! oracles are the provider's own captures and the notice stream's committed rows.

use super::{A1, AUDIENCE, Checked, alerts, payloads, provider, run_over, text, world, writer};
use crate::WebPushSwitch;
use mandate_notify::PushChannel;

#[test]
fn with_web_push_off_a_step_makes_no_web_push_attempt_and_journals_none() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit"), (2, "agent_held")])?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    let audience = WebPushSwitch::default().sends(&AUDIENCE)?;
    run_over(&mut journal.clone(), &writer, &[A1], &audience, &mut sends)??;
    let web_push = PushChannel::WebPush.key()?;
    let email = PushChannel::Email.key()?;
    let sent: Vec<&str> = sends.seen.iter().map(|(_, c, _)| c.as_str()).collect();
    assert!(!sent.is_empty(), "the email sends still go");
    assert!(
        sent.iter().all(|c| *c == email),
        "no web-push send: {sent:?}"
    );
    let attempted = payloads(&journal, "NoticeAttempted");
    let channels: Vec<String> = attempted.iter().map(|p| text(p, "channel")).collect();
    assert_eq!(
        channels.len(),
        payloads(&journal, "NoticeIssued").len(),
        "one email attempt per notice, and nothing else"
    );
    assert!(
        channels.iter().all(|c| c != web_push),
        "no web-push attempt is journaled: {channels:?}"
    );
    Ok(())
}
