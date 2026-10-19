//! E8-11 slice S3b (notifications spec §4.4, §5.2; DEC-700 item 4, DEC-820 item 4, DEC-2508,
//! DEC-2510): the email adapter mails the founder alone, through a transport bound to the
//! founder's address, and answers every other send `permanent { recipient_not_permitted }` before
//! the transport is called: another recipient, another channel, and the brief. A transport's
//! closed failure maps to its outcome, and every captured message passes NT-1's canary scan.

use mandate_dispatcher::{EmailProvider, MailFailure, RecordedTransport};
use mandate_notify::canary::scan;
use mandate_notify::email::{FOUNDER_FOOTER, FounderAddress};
use mandate_notify::{
    AddressHandle, IdempotencyKey, NoticeId, Notification, NotifyError, Origin, Outcome, Provider,
    PushChannel, Reason, Recipient, SecureRandom, TextKey,
};

const FOUNDER: &str = "founder@mail.example";
const FOUNDER_ID: &str = "p_01j8z2founder0000000000000";
const ORIGIN: &str = "https://app.owlhead.example";

/// The text keys a mail is rendered for: every one but the brief's.
const MAILED: [TextKey; 3] = [
    TextKey::ApprovalNeeded,
    TextKey::AttentionNeeded,
    TextKey::AccountChanged,
];

/// A deterministic source: each id is its own counter's bytes, so no two notices share one.
struct Counter(u8);

impl SecureRandom for Counter {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        self.0 = self.0.wrapping_add(1);
        *bytes = [self.0; 16];
        Ok(())
    }
}

fn answer<T>(what: &str, result: Result<T, NotifyError>) -> T {
    result.unwrap_or_else(|e| panic!("{what}: {e:?}"))
}

fn origin() -> Origin {
    answer("origin", Origin::parse(ORIGIN))
}

fn recipient(id: &str) -> Recipient {
    answer(id, Recipient::parse(id))
}

fn handle(id: &str, channel: PushChannel) -> AddressHandle {
    AddressHandle {
        recipient: recipient(id),
        channel,
    }
}

fn provider(transport: RecordedTransport) -> EmailProvider<RecordedTransport> {
    answer(
        "provider",
        EmailProvider::new(recipient(FOUNDER_ID), transport),
    )
}

fn bound() -> RecordedTransport {
    let address = answer("address", FounderAddress::parse(FOUNDER));
    answer("transport", RecordedTransport::bound_to(address))
}

/// Sends a fresh notice of `text` to `to` and returns the outcome.
fn send(
    provider: &mut EmailProvider<RecordedTransport>,
    random: &mut Counter,
    text: TextKey,
    to: &AddressHandle,
) -> Outcome {
    let notice = answer("notice", NoticeId::mint(random));
    let key = answer("key", IdempotencyKey::of(&notice, to));
    let notification = Notification { notice, text };
    answer("send", provider.send(&origin(), &notification, to, &key))
}

fn captured(provider: &EmailProvider<RecordedTransport>) -> Vec<String> {
    let transport = answer("transport", provider.transport());
    answer("captured", transport.captured())
        .iter()
        .map(|wire| String::from_utf8(wire.clone()).expect("a captured message is text"))
        .collect()
}

fn refused() -> Outcome {
    Outcome::Permanent {
        reason: Reason::RecipientNotPermitted,
    }
}

/// Spec §4.4, DEC-820 item 4: a mailed notice to the founder is taken once per send, addressed
/// to the bound address alone, with exactly DEC-820's footer, and accepted under the fixture's id,
/// which names no address.
#[test]
fn the_founder_is_mailed_at_the_bound_address_alone() {
    let (mut provider, mut random) = (provider(bound()), Counter(0));
    let to = handle(FOUNDER_ID, PushChannel::Email);
    for (n, key) in MAILED.into_iter().enumerate() {
        let id = format!("recorded-{}", n + 1);
        assert_eq!(
            send(&mut provider, &mut random, key, &to),
            Outcome::Accepted {
                provider_message_id: id
            },
            "{key:?}"
        );
    }
    let mails = captured(&provider);
    assert_eq!(mails.len(), MAILED.len(), "one message per send");
    for mail in &mails {
        assert_eq!(mail.matches(FOUNDER).count(), 1, "the address once: {mail}");
        assert!(mail.contains(&format!("\r\nTo: {FOUNDER}\r\n")), "{mail}");
        assert_eq!(
            mail.matches(FOUNDER_FOOTER).count(),
            2,
            "both parts: {mail}"
        );
        for header in ["\r\nCc:", "\r\nBcc:", "\r\nReply-To:", "List-Unsubscribe"] {
            assert!(!mail.contains(header), "{header} in {mail}");
        }
    }
}

/// Spec §4.4: a notice for any other recipient, a workspace owner who is not the founder included,
/// is refused `recipient_not_permitted` before any connection. The transport is not called: its
/// scripted failure is still the first answer the founder's next send gets.
#[test]
fn every_other_recipient_is_refused_before_the_transport() {
    let address = answer("address", FounderAddress::parse(FOUNDER));
    let transport = answer(
        "transport",
        RecordedTransport::failing(address, vec![MailFailure::Timeout]),
    );
    let (mut provider, mut random) = (provider(transport), Counter(0));
    for other in [
        "p_01j8z2owner00000000000000",
        "founder",
        "p_01j8z2founder0000000000001",
    ] {
        for key in MAILED {
            let to = handle(other, PushChannel::Email);
            assert_eq!(
                send(&mut provider, &mut random, key, &to),
                refused(),
                "{other} {key:?}"
            );
        }
    }
    assert!(
        captured(&provider).is_empty(),
        "nothing is captured for a refusal"
    );
    let founder = handle(FOUNDER_ID, PushChannel::Email);
    assert_eq!(
        send(
            &mut provider,
            &mut random,
            TextKey::ApprovalNeeded,
            &founder
        ),
        Outcome::Retryable {
            reason: Reason::Timeout
        },
        "the refusals never reached the transport"
    );
}

/// DEC-2508 item 5, the #1615 review: the brief is not mailed, even to the founder. It is refused
/// `recipient_not_permitted`, which spec §5.2 never retries and which marks no address.
#[test]
fn the_brief_is_refused_before_the_transport() {
    let (mut provider, mut random) = (provider(bound()), Counter(0));
    let to = handle(FOUNDER_ID, PushChannel::Email);
    assert_eq!(
        send(&mut provider, &mut random, TextKey::BriefReady, &to),
        refused()
    );
    assert!(captured(&provider).is_empty());
}

/// DEC-2510 item 2: the adapter sends only on the email channel; a handle for any other channel,
/// the founder's included, is refused before the transport.
#[test]
fn another_channel_is_refused_before_the_transport() {
    let (mut provider, mut random) = (provider(bound()), Counter(0));
    for channel in PushChannel::ALL
        .into_iter()
        .filter(|c| *c != PushChannel::Email)
    {
        let to = handle(FOUNDER_ID, channel);
        assert_eq!(
            send(&mut provider, &mut random, TextKey::AttentionNeeded, &to),
            refused(),
            "{channel:?}"
        );
    }
    assert!(captured(&provider).is_empty());
}

/// DEC-2510 item 3, spec §5.2: each closed transport failure has its outcome, and none captures a
/// message; the send after them is taken as the first.
#[test]
fn each_transport_failure_has_its_outcome() {
    let cases = [
        (
            MailFailure::Timeout,
            Outcome::Retryable {
                reason: Reason::Timeout,
            },
        ),
        (
            MailFailure::RateLimited,
            Outcome::Retryable {
                reason: Reason::RateLimited,
            },
        ),
        (
            MailFailure::Unavailable,
            Outcome::Retryable {
                reason: Reason::ProviderError,
            },
        ),
        (
            MailFailure::AuthFailed,
            Outcome::Permanent {
                reason: Reason::AuthFailed,
            },
        ),
        (
            MailFailure::AddressRejected,
            Outcome::Permanent {
                reason: Reason::AddressRejected,
            },
        ),
        (
            MailFailure::TooLarge,
            Outcome::Permanent {
                reason: Reason::TooLarge,
            },
        ),
    ];
    let address = answer("address", FounderAddress::parse(FOUNDER));
    let script = cases.iter().map(|(failure, _)| *failure).collect();
    let transport = answer("transport", RecordedTransport::failing(address, script));
    let (mut provider, mut random) = (provider(transport), Counter(0));
    let to = handle(FOUNDER_ID, PushChannel::Email);
    for (failure, expected) in cases {
        assert_eq!(
            send(&mut provider, &mut random, TextKey::AccountChanged, &to),
            expected,
            "{failure:?}"
        );
    }
    assert!(
        captured(&provider).is_empty(),
        "a failed submission captures nothing"
    );
    assert_eq!(
        send(&mut provider, &mut random, TextKey::AccountChanged, &to),
        Outcome::Accepted {
            provider_message_id: "recorded-1".to_owned()
        }
    );
    assert_eq!(captured(&provider).len(), 1);
}

/// NT-1: nothing a captured message holds is trading data.
#[test]
fn every_captured_message_passes_the_canary_scan() {
    let (mut provider, mut random) = (provider(bound()), Counter(40));
    let to = handle(FOUNDER_ID, PushChannel::Email);
    for key in MAILED {
        send(&mut provider, &mut random, key, &to);
    }
    let mails = captured(&provider);
    assert_eq!(mails.len(), MAILED.len());
    for mail in mails {
        assert_eq!(
            answer("scan", scan(mail.as_bytes())),
            Vec::<&str>::new(),
            "{mail}"
        );
    }
}
