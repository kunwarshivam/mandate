//! Spec §5.2, DEC-710 item 8, DEC-712: the provider's closed `Reason`, and a fixture provider that
//! records the key's digest and the rendered message, never the address, and answers
//! `permanent { address_missing }` for a handle with no vault entry.

mod common;

use common::{ORIGIN, answer};
use mandate_notify::{
    AddressHandle, FixtureProvider, IdempotencyKey, NoticeId, Notification, Origin, Outcome,
    Provider, PushChannel, Reason, Recipient, Sent, TextKey,
};

const NOTICE: &str = "6f1c2a9e4b7d03581e2f9a6c4d8b0e17";
const EMAIL_KEY: &str = "b177305accb4e5ddbb2d527970491a28a0a454f3d780e04545170bea2b1b4605";
const SMS_KEY: &str = "0ba1c40129d70f096e01f47ad9300ead212ca25c3b0d21849f010f2109379f54";
const MESSAGE: &str = "An agent in your workspace needs your approval\nhttps://app.owlhead.example/n/6f1c2a9e4b7d03581e2f9a6c4d8b0e17";

fn handle(recipient: &str, channel: PushChannel) -> AddressHandle {
    AddressHandle {
        recipient: answer(recipient, Recipient::parse(recipient)),
        channel,
    }
}

fn send(provider: &mut FixtureProvider, address: &AddressHandle) -> Outcome {
    let notice = answer("notice", NoticeId::parse(NOTICE));
    let key = answer("key", IdempotencyKey::of(&notice, address));
    let notification = Notification {
        notice,
        text: TextKey::ApprovalNeeded,
    };
    let origin = answer("origin", Origin::parse(ORIGIN));
    answer("send", provider.send(&origin, &notification, address, &key))
}

fn record(address: &AddressHandle, key: &str) -> Sent {
    Sent {
        address: address.clone(),
        key: key.to_owned(),
        message: MESSAGE.to_owned(),
    }
}

fn accepted(id: &str) -> Outcome {
    Outcome::Accepted {
        provider_message_id: id.to_owned(),
    }
}

#[test]
#[ignore = "pending E8-9"]
fn the_providers_reasons_are_the_specs_closed_set() {
    let keys: Vec<&str> = Reason::ALL
        .into_iter()
        .map(|r| answer("key", r.key()))
        .collect();
    let spec = "timeout rate_limited provider_error address_rejected auth_failed too_large \
        recipient_not_permitted address_missing bounced complained unsubscribed";
    assert_eq!(keys, spec.split(' ').collect::<Vec<_>>());
}

/// The keys are `send.rs`'s `sha256sum` literals for `owner`, notice `6f1c…0e17`, on email and SMS
/// (DEC-710 item 5); the message is §4.2's text, a newline, and the link (item 6).
#[test]
#[ignore = "pending E8-9"]
fn the_fixture_accepts_a_held_address_and_records_the_digest_and_message_never_the_address() {
    let email = handle("owner", PushChannel::Email);
    let sms = handle("owner", PushChannel::Sms);
    let mut provider = answer(
        "holding",
        FixtureProvider::holding(vec![
            (email.clone(), "zqpersonal.owner@example.com".to_owned()),
            (sms.clone(), "+15555550100".to_owned()),
        ]),
    );
    assert_eq!(send(&mut provider, &email), accepted("fixture-1"));
    assert_eq!(send(&mut provider, &sms), accepted("fixture-2"));
    let sent = answer("sent", provider.sent());
    assert_eq!(sent, [record(&email, EMAIL_KEY), record(&sms, SMS_KEY)]);
}

/// DEC-712, spec §5.1: an active address whose vault entry is gone is a terminal failure that
/// sends nothing; a held address still goes.
#[test]
#[ignore = "pending E8-9"]
fn a_handle_with_no_vault_entry_is_permanent_address_missing_and_sends_nothing() {
    let email = handle("owner", PushChannel::Email);
    let mut provider = answer(
        "holding",
        FixtureProvider::holding(vec![(email.clone(), "owner@example.com".to_owned())]),
    );
    let missing = Outcome::Permanent {
        reason: Reason::AddressMissing,
    };
    for absent in [
        handle("owner", PushChannel::Telegram),
        handle("u_7", PushChannel::Email),
    ] {
        assert_eq!(send(&mut provider, &absent), missing, "{absent:?}");
    }
    assert_eq!(answer("sent", provider.sent()), [] as [Sent; 0]);
    assert_eq!(send(&mut provider, &email), accepted("fixture-1"));
    assert_eq!(answer("sent", provider.sent()), [record(&email, EMAIL_KEY)]);
}
