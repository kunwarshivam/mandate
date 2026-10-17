//! Control-plane design CP-5 (a) and (c) at `cp-agent` (E20-3, DEC-981): an inbound message is
//! handed to workspace services as verified only when its kind is one of the five instructions,
//! its signature verifies against a key the release pins for that kind, and its body is exactly
//! its closed schema; everything else is handed over as a refusal with §9.17's reason.

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cp_agent::{
    CatalogKind, Feature, HandOver, Ident, Instruction, InstructionSink, KeyKind, MessageKind,
    PinnedKeys, Refusal, RefusalReason, Site, Verified, WithdrawalReason, receive, signed_bytes,
};
use proptest::prelude::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

const ORG: &str = "org_01JACME";
const DEPLOYMENT: &str = "dep_01JCELL1";

fn pair(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn id(text: &str) -> Ident {
    Ident::parse(text).unwrap()
}

/// The release's pinned keys: one per kind, seeds 1 to 4.
fn keys() -> PinnedKeys {
    PinnedKeys::new()
        .pin(
            &id("license_key_1"),
            KeyKind::License,
            pair(1).public_key().as_ref(),
        )
        .and_then(|k| {
            k.pin(
                &id("release_key_1"),
                KeyKind::Release,
                pair(2).public_key().as_ref(),
            )
        })
        .and_then(|k| {
            k.pin(
                &id("registry_key_1"),
                KeyKind::Registry,
                pair(3).public_key().as_ref(),
            )
        })
        .and_then(|k| {
            k.pin(
                &id("data_key_1"),
                KeyKind::Data,
                pair(4).public_key().as_ref(),
            )
        })
        .unwrap()
}

fn site() -> Site {
    Site {
        deployment_id: id(DEPLOYMENT),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const LICENSE: &str = r#"{"license_id":"lic_0001","sequence":3,"org_id":"org_01JACME",
    "deployment_ids":["dep_01JCELL1"],"max_workspaces":5,"max_agents_deployed":20,
    "features":["connected_clients"],"not_before":"2026-10-01T00:00:00.000000000Z",
    "not_after":"2027-10-01T00:00:00.000000000Z","grace_days":30}"#;
const MANIFEST: &str = r#"{"release_id":"rel_2026_10_17","sequence":12,
    "images":[{"component":"api","digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}]}"#;
const WITHDRAWN: &str = r#"{"release_id":"rel_2026_10_10","reason":"defect"}"#;
const CONNECTOR: &str = r#"{"kind":"connector_package","entry_id":"connector_robinhood",
    "content_hash":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","sequence":4}"#;
const REGISTRY: &str = r#"{"kind":"model_registry_entry","entry_id":"momentum_v2",
    "content_hash":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","sequence":5}"#;
const BUNDLE: &str = r#"{"dataset":"sec_filings",
    "bundle_digest":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","sequence":101}"#;

/// One message: `body` signed as `kind` by `signer` under `key_id`.
fn claimed(kind: &str) -> MessageKind {
    [
        MessageKind::License,
        MessageKind::ReleaseManifest,
        MessageKind::ReleaseWithdrawn,
        MessageKind::CatalogEntry,
        MessageKind::DataBundle,
    ]
    .into_iter()
    .find(|k| k.as_str() == kind)
    .unwrap_or(MessageKind::Unknown)
}

/// The canonical envelope of `members`, each `(name, JSON)`.
fn envelope(members: &[(&str, &str)]) -> Vec<u8> {
    let mut object = mandate_canon::Object::new();
    for (name, json) in members {
        object.insert(
            mandate_canon::Key::new(name).unwrap(),
            parse(json.as_bytes()).unwrap(),
        );
    }
    to_canonical(&Value::Object(object))
}

fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// One message: `body` signed as `kind` by `signer` under `key_id`, in canonical form.
fn message(kind: &str, key_id: &str, body: &str, signer: &Ed25519KeyPair) -> Vec<u8> {
    let parsed = parse(body.as_bytes()).unwrap();
    let signature = signer.sign(&signed_bytes(claimed(kind), key_id, &parsed));
    envelope(&[
        ("kind", &quoted(kind)),
        ("key_id", &quoted(key_id)),
        ("body", body),
        ("signature", &quoted(&hex(signature.as_ref()))),
    ])
}

/// Every valid instruction: its kind, signing key ID, body, and signer.
fn valid() -> Vec<(&'static str, &'static str, &'static str, Ed25519KeyPair)> {
    vec![
        ("license", "license_key_1", LICENSE, pair(1)),
        ("release_manifest", "release_key_1", MANIFEST, pair(2)),
        ("release_withdrawn", "release_key_1", WITHDRAWN, pair(2)),
        ("catalog_entry", "release_key_1", CONNECTOR, pair(2)),
        ("catalog_entry", "registry_key_1", REGISTRY, pair(3)),
        ("data_bundle", "data_key_1", BUNDLE, pair(4)),
    ]
}

/// Workspace services as a recorder: every hand-over, in order.
#[derive(Default)]
struct Recorder {
    verified: Vec<Verified>,
    refused: Vec<Refusal>,
}

impl InstructionSink for Recorder {
    fn hand_over(&mut self, verified: Verified) -> HandOver {
        self.verified.push(verified);
        HandOver::Journaled
    }

    fn refused(&mut self, refusal: Refusal) -> HandOver {
        self.refused.push(refusal);
        HandOver::Journaled
    }
}

fn received(bytes: &[u8]) -> Recorder {
    let mut sink = Recorder::default();
    assert_eq!(
        receive(bytes, &keys(), &site(), &mut sink),
        HandOver::Journaled
    );
    assert_eq!(sink.verified.len() + sink.refused.len(), 1, "one hand-over");
    sink
}

fn refusal(bytes: &[u8]) -> Refusal {
    let sink = received(bytes);
    assert!(sink.verified.is_empty(), "nothing is verified");
    sink.refused[0]
}

/// `body` with `member` set to `value` (JSON), or removed when `value` is `None`.
fn edited(body: &str, member: &str, value: Option<&str>) -> String {
    let mut object = parse(body.as_bytes())
        .unwrap()
        .as_object()
        .cloned()
        .unwrap();
    let key = mandate_canon::Key::new(member).unwrap();
    match value {
        Some(v) => object.insert(key, parse(v.as_bytes()).unwrap()),
        None => object.remove(member),
    };
    String::from_utf8(to_canonical(&Value::Object(object))).unwrap()
}

#[test]
fn every_valid_instruction_is_handed_over_verified_with_its_key_and_digest() {
    for (kind, key_id, body, signer) in valid() {
        let bytes = message(kind, key_id, body, &signer);
        let sink = received(&bytes);
        assert!(
            sink.refused.is_empty(),
            "{kind} by {key_id}: {:?}",
            sink.refused
        );
        let verified = &sink.verified[0];
        assert_eq!(verified.key_id().as_str(), key_id);
        assert_eq!(verified.document_digest(), Digest::of(&bytes));
        let claimed = match verified.instruction() {
            Instruction::License(_) => "license",
            Instruction::ReleaseManifest(_) => "release_manifest",
            Instruction::ReleaseWithdrawn(_) => "release_withdrawn",
            Instruction::CatalogEntry(_) => "catalog_entry",
            Instruction::DataBundle(_) => "data_bundle",
        };
        assert_eq!(claimed, kind);
    }
}

#[test]
fn a_license_carries_its_members_and_sequence() {
    let sink = received(&message("license", "license_key_1", LICENSE, &pair(1)));
    let Instruction::License(license) = sink.verified[0].instruction().clone() else {
        panic!("a license");
    };
    assert_eq!(license.license_id.as_str(), "lic_0001");
    assert_eq!(license.sequence.get(), 3);
    assert_eq!(license.org_id.as_str(), ORG);
    assert_eq!(license.max_agents_deployed, 20);
    assert_eq!(license.grace_days, 30);
    assert_eq!(
        sink.verified[0].instruction().sequence().map(|s| s.get()),
        Some(3)
    );
}

#[test]
fn an_unsigned_instruction_is_refused_unsigned_with_its_claimed_sequence() {
    let bytes = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_1""#),
        ("body", LICENSE),
    ]);
    let r = refusal(&bytes);
    assert_eq!(
        (r.message, r.reason, r.sequence),
        (MessageKind::License, RefusalReason::Unsigned, Some(3))
    );
    assert_eq!(r.document_digest, Digest::of(&bytes));
    let null = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_1""#),
        ("body", LICENSE),
        ("signature", "null"),
    ]);
    assert_eq!(refusal(&null).reason, RefusalReason::Unsigned);
}

#[test]
fn a_wrong_signature_is_refused_bad_signature() {
    let forged = message("license", "license_key_1", LICENSE, &pair(9));
    assert_eq!(refusal(&forged).reason, RefusalReason::BadSignature);
    let garbled = String::from_utf8(message("license", "license_key_1", LICENSE, &pair(1)))
        .unwrap()
        .replace(r#""signature":""#, r#""signature":"zz"#);
    assert_eq!(
        refusal(garbled.as_bytes()).reason,
        RefusalReason::BadSignature
    );
}

#[test]
fn a_key_not_pinned_for_its_kind_is_refused_unknown_key() {
    let unpinned = message("license", "license_key_2", LICENSE, &pair(1));
    assert_eq!(refusal(&unpinned).reason, RefusalReason::UnknownKey);
    let by_the_release_key = message("license", "release_key_1", LICENSE, &pair(2));
    assert_eq!(
        refusal(&by_the_release_key).reason,
        RefusalReason::UnknownKey
    );
    let registry_entry_by_release = message("catalog_entry", "release_key_1", REGISTRY, &pair(2));
    assert_eq!(
        refusal(&registry_entry_by_release).reason,
        RefusalReason::UnknownKey
    );
    let bundle_by_data_as_license = message("data_bundle", "license_key_1", BUNDLE, &pair(1));
    assert_eq!(
        refusal(&bundle_by_data_as_license).reason,
        RefusalReason::UnknownKey
    );
}

#[test]
fn a_signature_for_one_kind_does_not_verify_as_another() {
    let body = parse(WITHDRAWN.as_bytes()).unwrap();
    let signature = pair(2).sign(&signed_bytes(
        MessageKind::ReleaseWithdrawn,
        "release_key_1",
        &body,
    ));
    let relabeled = envelope(&[
        ("kind", r#""release_manifest""#),
        ("key_id", r#""release_key_1""#),
        ("body", WITHDRAWN),
        ("signature", &quoted(&hex(signature.as_ref()))),
    ]);
    assert_eq!(refusal(&relabeled).reason, RefusalReason::BadSignature);
}

#[test]
fn a_kind_outside_the_list_is_refused_out_of_list_with_no_sequence() {
    for kind in [
        "heartbeat",
        "receipt",
        "kill_switch",
        "mandate",
        "pause",
        "License",
    ] {
        let r = refusal(&message(kind, "license_key_1", LICENSE, &pair(1)));
        assert_eq!(
            (r.message, r.reason, r.sequence),
            (MessageKind::Unknown, RefusalReason::OutOfList, None),
            "{kind}"
        );
    }
    let kindless = format!(r#"{{"key_id":"license_key_1","body":{LICENSE}}}"#);
    assert_eq!(
        refusal(kindless.as_bytes()).reason,
        RefusalReason::OutOfList
    );
}

#[test]
fn bytes_that_are_not_a_message_are_refused_malformed() {
    for bytes in [&b"not json"[..], b"", b"[1,2]", b"{\"kind\":1.5}"] {
        let r = refusal(bytes);
        assert_eq!((r.message, r.sequence), (MessageKind::Unknown, None));
        assert!(
            matches!(
                r.reason,
                RefusalReason::Malformed | RefusalReason::OutOfList
            ),
            "{:?}",
            String::from_utf8_lossy(bytes)
        );
    }
    let body = parse(LICENSE.as_bytes()).unwrap();
    let signature = pair(1).sign(&signed_bytes(MessageKind::License, "license_key_1", &body));
    let extra = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_1""#),
        ("body", LICENSE),
        ("note", r#""x""#),
        ("signature", &quoted(&hex(signature.as_ref()))),
    ]);
    assert_eq!(refusal(&extra).reason, RefusalReason::Malformed);
}

#[test]
fn a_signed_body_outside_its_schema_is_refused_malformed() {
    let cases = [
        edited(LICENSE, "note", Some(r#""free text""#)),
        edited(LICENSE, "grace_days", None),
        edited(LICENSE, "sequence", Some("0")),
        edited(LICENSE, "deployment_ids", Some("[]")),
        edited(
            LICENSE,
            "deployment_ids",
            Some(r#"["dep_01JCELL2","dep_01JCELL1"]"#),
        ),
        edited(
            LICENSE,
            "deployment_ids",
            Some(r#"["dep_01JCELL1","dep_01JCELL1"]"#),
        ),
        edited(
            LICENSE,
            "features",
            Some(r#"["hybrid","connected_clients"]"#),
        ),
        edited(LICENSE, "features", Some(r#"["exits_disabled"]"#)),
        edited(
            LICENSE,
            "not_after",
            Some(r#""2026-10-01T00:00:00.000000000Z""#),
        ),
        edited(LICENSE, "not_before", Some(r#""2026-10-01""#)),
        edited(LICENSE, "org_id", Some(r#""org with spaces""#)),
    ];
    for body in cases {
        let r = refusal(&message("license", "license_key_1", &body, &pair(1)));
        assert_eq!(r.reason, RefusalReason::Malformed, "{body}");
    }
    let manifests = [
        edited(MANIFEST, "images", Some("[]")),
        edited(
            MANIFEST,
            "images",
            Some(r#"[{"component":"api","digest":"cc","x":1}]"#),
        ),
    ];
    for body in manifests {
        let r = refusal(&message(
            "release_manifest",
            "release_key_1",
            &body,
            &pair(2),
        ));
        assert_eq!(r.reason, RefusalReason::Malformed, "{body}");
    }
    let withdrawn = edited(WITHDRAWN, "reason", Some(r#""stop_trading""#));
    let r = refusal(&message(
        "release_withdrawn",
        "release_key_1",
        &withdrawn,
        &pair(2),
    ));
    assert_eq!(r.reason, RefusalReason::Malformed);
}

#[test]
fn a_license_for_another_deployment_is_refused_not_addressed() {
    for body in [
        edited(LICENSE, "deployment_ids", Some(r#"["dep_01JCELL2"]"#)),
        edited(
            LICENSE,
            "deployment_ids",
            Some(r#"["dep_01JCELL0","dep_01JCELL2"]"#),
        ),
    ] {
        let r = refusal(&message("license", "license_key_1", &body, &pair(1)));
        assert_eq!(
            (r.reason, r.sequence),
            (RefusalReason::NotAddressed, Some(3)),
            "{body}"
        );
    }
}

#[test]
fn the_spellings_are_the_journal_specs() {
    let reasons = [
        (RefusalReason::Unsigned, "unsigned"),
        (RefusalReason::BadSignature, "bad_signature"),
        (RefusalReason::UnknownKey, "unknown_key"),
        (RefusalReason::Replayed, "replayed"),
        (RefusalReason::OutOfList, "out_of_list"),
        (RefusalReason::Malformed, "malformed"),
        (RefusalReason::NotAddressed, "not_addressed"),
    ];
    for (reason, spelling) in reasons {
        assert_eq!(reason.as_str(), spelling);
    }
    assert_eq!(MessageKind::Unknown.as_str(), "unknown");
    assert_eq!(Feature::ConnectedClients.as_str(), "connected_clients");
    assert_eq!(Feature::Hybrid.as_str(), "hybrid");
    assert_eq!(WithdrawalReason::Defect.as_str(), "defect");
    assert_eq!(WithdrawalReason::Security.as_str(), "security");
    assert_eq!(WithdrawalReason::Superseded.as_str(), "superseded");
    assert_eq!(CatalogKind::ConnectorPackage.as_str(), "connector_package");
    assert_eq!(
        CatalogKind::ModelRegistryEntry.as_str(),
        "model_registry_entry"
    );
}

proptest! {
    /// Nothing but a message the release's own key signed is ever handed over verified: random
    /// bytes, and random bodies of every kind signed by a key the release does not pin.
    #[test]
    fn only_a_pinned_signature_is_ever_verified(
        noise in proptest::collection::vec(any::<u8>(), 0..256),
        seed in 5u8..=255,
        kind in prop::sample::select(vec!["license", "release_manifest", "release_withdrawn", "catalog_entry", "data_bundle"]),
        key_id in prop::sample::select(vec!["license_key_1", "release_key_1", "registry_key_1", "data_key_1"]),
        sequence in 0u64..1000,
    ) {
        prop_assert!(received(&noise).verified.is_empty());
        let body = format!(r#"{{"sequence":{sequence},"release_id":"r","license_id":"l"}}"#);
        let sink = received(&message(kind, key_id, &body, &pair(seed)));
        prop_assert!(sink.verified.is_empty());
    }
}

/// Review of #1467: one document has one digest. A genuine message re-spaced, with its members
/// reordered, or with its body reordered still carries a valid signature over the canonical body,
/// so only its form tells it apart; it is refused, so a replay in another form can neither dodge
/// the de-duplication of journal spec §9.17 nor take the genuine message's sequence.
#[test]
fn only_the_canonical_form_of_a_message_is_verified() {
    let genuine = message("license", "license_key_1", LICENSE, &pair(1));
    assert_eq!(received(&genuine).verified.len(), 1);
    let text = String::from_utf8(genuine.clone()).unwrap();
    let variants = [
        text.replacen(':', ": ", 1),
        format!(" {text}"),
        format!("{text}\n"),
        text.replacen(r#""body":{"#, r#""body":{ "#, 1),
        text.replace("lic_0001", "lic_\\u0030001"),
    ];
    for variant in variants {
        let r = refusal(variant.as_bytes());
        assert_eq!(
            (r.message, r.reason),
            (MessageKind::License, RefusalReason::Malformed),
            "{variant}"
        );
    }
}

#[test]
fn a_message_over_the_cap_is_refused_before_it_is_parsed() {
    let mut big = message("license", "license_key_1", LICENSE, &pair(1));
    big.resize(mandate_cp_agent::MAX_MESSAGE_BYTES + 1, b' ');
    let r = refusal(&big);
    assert_eq!(
        (r.message, r.reason),
        (MessageKind::Unknown, RefusalReason::Malformed)
    );
}

#[test]
fn a_tampered_body_or_a_substituted_key_is_refused_bad_signature() {
    let body = parse(LICENSE.as_bytes()).unwrap();
    let signature = hex(pair(1)
        .sign(&signed_bytes(MessageKind::License, "license_key_1", &body))
        .as_ref());
    let tampered = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_1""#),
        (
            "body",
            &edited(LICENSE, "max_agents_deployed", Some("2000")),
        ),
        ("signature", &quoted(&signature)),
    ]);
    assert_eq!(refusal(&tampered).reason, RefusalReason::BadSignature);
    let keys = keys()
        .pin(
            &id("license_key_2"),
            KeyKind::License,
            pair(1).public_key().as_ref(),
        )
        .unwrap();
    let substituted = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_2""#),
        ("body", LICENSE),
        ("signature", &quoted(&signature)),
    ]);
    let mut sink = Recorder::default();
    receive(&substituted, &keys, &site(), &mut sink);
    assert_eq!(
        sink.refused[0].reason,
        RefusalReason::BadSignature,
        "the key ID is signed"
    );
    for wrong in [
        signature.to_uppercase(),
        signature[..126].to_owned(),
        String::new(),
    ] {
        let bytes = envelope(&[
            ("kind", r#""license""#),
            ("key_id", r#""license_key_1""#),
            ("body", LICENSE),
            ("signature", &quoted(&wrong)),
        ]);
        assert_eq!(
            refusal(&bytes).reason,
            RefusalReason::BadSignature,
            "{wrong}"
        );
    }
}

/// A known answer for the signed bytes, written out by hand: the domain tag, the kind, the key ID
/// and the canonical body, separated by zero bytes (DEC-982 item 2).
#[test]
fn the_signed_bytes_are_the_tag_kind_key_and_canonical_body() {
    let body = parse(br#"{"reason":"defect", "release_id":"r1"}"#).unwrap();
    let want = b"mandate-cp/1\0release_withdrawn\0release_key_1\0{\"reason\":\"defect\",\"release_id\":\"r1\"}";
    assert_eq!(
        signed_bytes(MessageKind::ReleaseWithdrawn, "release_key_1", &body),
        want.to_vec()
    );
}

#[test]
fn a_claimed_sequence_is_recorded_only_when_it_is_a_sequence() {
    for (sequence, recorded) in [
        ("0", None),
        ("9007199254740991", Some(9_007_199_254_740_991)),
        ("\"3\"", None),
    ] {
        let body = edited(LICENSE, "sequence", Some(sequence));
        let bytes = envelope(&[
            ("kind", r#""license""#),
            ("key_id", r#""license_key_1""#),
            ("body", &body),
        ]);
        assert_eq!(refusal(&bytes).sequence, recorded, "{sequence}");
    }
}

#[test]
fn a_release_pins_each_key_once_and_only_a_32_byte_key() {
    let key = pair(1);
    let one = PinnedKeys::new().pin(&id("k"), KeyKind::License, key.public_key().as_ref());
    assert!(
        one.clone()
            .and_then(|k| k.pin(&id("k"), KeyKind::Release, key.public_key().as_ref()))
            .is_none()
    );
    assert!(
        PinnedKeys::new()
            .pin(&id("k"), KeyKind::License, &[0; 31])
            .is_none()
    );
    assert!(
        PinnedKeys::new()
            .pin(&id("k"), KeyKind::License, &[0; 33])
            .is_none()
    );
    assert!(one.is_some());
}
