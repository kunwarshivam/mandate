//! Route 2's judge (§4.10 route 2 items 2 and 4, DEC-2324 item 5, DEC-2319 item 2; E9-1 S2b-3a):
//! a held passkey whose member signed in and reaches the hosted workspace opens; every other
//! request, including one whose signature the decoy's published secret made, opens nothing.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::control::{Rows, T0, activation, change, member};
use common::passkey::{Signer, enrolled, removed, rp};
use common::{at, hosted};
use mandate_api_server::members::{ControlLookup, credential_records, membership_records};
use mandate_api_server::opening::{DECOY_MEMBER, Opened, Presented, Reads, judge};
use mandate_api_server::standing::Standings;
use mandate_authn::SubjectStanding;
use mandate_canon::Digest;
use mandate_identity::{LookupFailed, PrincipalId, Role};
use mandate_passkey::Challenge;

const ALICE: PrincipalId = PrincipalId(7);
const BOB: PrincipalId = PrincipalId(8);
const CREDENTIAL: &str = "AAECAwQFBgcICQoLDA0ODw";
const STRANGER: &str = "EBESExQVFhcYGRobHB0eHw";
const CHALLENGE: [u8; 32] = [0x42; 32];
/// A hosted workspace where ALICE is an active operator who enrolled `signer` as CREDENTIAL and
/// signed in on this process at T0 - 500.
struct World {
    rows: Rows,
    keys: Option<BTreeMap<Digest, Vec<u8>>>,
    standings: Standings,
    unreadable_credentials: bool,
    unreadable_memberships: bool,
}

impl World {
    fn new(signer: &Signer) -> Self {
        let mut rows = Rows::opened();
        let operator = BTreeSet::from([Role::Operator]);
        rows.hosted(
            "MemberActivated",
            T0 - 1_000,
            activation(ALICE, &operator, T0 - 1_000),
        );
        let key = signer.cose_key();
        rows.hosted(
            "CredentialEnrolled",
            T0 - 900,
            enrolled(ALICE, CREDENTIAL, &key, signer.alg()),
        );
        let standings = Standings::new();
        standings.signed_in(ALICE, at(T0 - 500));
        Self {
            rows,
            keys: Some(BTreeMap::from([(Digest::of(&key), key)])),
            standings,
            unreadable_credentials: false,
            unreadable_memberships: false,
        }
    }

    fn judge(&self, passkey_id: &str, assertion: &(Vec<u8>, Vec<u8>, Vec<u8>)) -> Option<Opened> {
        let credentials = credential_records(hosted().workspace, &self.rows.rows).unwrap();
        let memberships = membership_records(hosted().workspace, &self.rows.rows).unwrap();
        let reads = Reads {
            hosted: hosted(),
            credentials: (!self.unreadable_credentials).then_some(credentials.as_slice()),
            memberships: (!self.unreadable_memberships).then_some(memberships.as_slice()),
            standings: &self.standings,
            keys: self
                .keys
                .as_ref()
                .map(|keys| keys as &(dyn mandate_journal::ArtifactSource + Send + Sync)),
        };
        let presented = Presented {
            passkey_id,
            authenticator_data: &assertion.0,
            client_data_json: &assertion.1,
            signature: &assertion.2,
        };
        judge(
            &rp(),
            &Challenge::new(&CHALLENGE).unwrap(),
            &presented,
            &reads,
            at(T0),
        )
    }
}

fn opened_for_alice() -> Option<Opened> {
    Some(Opened {
        member: ALICE,
        standing: SubjectStanding {
            last_sign_in: Some(at(T0 - 500)),
            last_deprovision: None,
        },
    })
}

#[test]
fn a_held_passkey_whose_member_signed_in_and_reaches_the_workspace_opens_for_them() {
    for signer in [Signer::es256(), Signer::ed25519()] {
        let world = World::new(&signer);
        assert_eq!(
            world.judge(CREDENTIAL, &signer.assert(&CHALLENGE)),
            opened_for_alice(),
            "{}",
            signer.alg()
        );
    }
    let signer = Signer::es256();
    let mut cooling = World::new(&signer);
    cooling.rows = Rows::opened();
    let key = signer.cose_key();
    cooling.rows.hosted(
        "MemberActivated",
        T0 - 1_000,
        activation(ALICE, &BTreeSet::from([Role::Operator]), T0 + 1_000),
    );
    cooling.rows.hosted(
        "CredentialEnrolled",
        T0 - 900,
        enrolled(ALICE, CREDENTIAL, &key, "es256"),
    );
    assert_eq!(
        cooling.judge(CREDENTIAL, &signer.assert(&CHALLENGE)),
        opened_for_alice(),
        "a cooling-off membership and a cooling-off passkey can pause and kill switch"
    );
}

/// Asserts `world` opens nothing for `assertion` naming `passkey_id`.
fn refused(name: &str, world: &World, passkey_id: &str, assertion: &(Vec<u8>, Vec<u8>, Vec<u8>)) {
    assert_eq!(world.judge(passkey_id, assertion), None, "{name}");
}

#[test]
fn every_other_request_opens_nothing() {
    let signer = Signer::es256();
    let good = signer.assert(&CHALLENGE);
    let world = World::new(&signer);
    refused("an unknown passkey id", &world, STRANGER, &good);
    let mut forged = good.clone();
    forged.2[12] ^= 1;
    refused("a bad signature", &world, CREDENTIAL, &forged);
    refused(
        "another challenge",
        &world,
        CREDENTIAL,
        &signer.assert(&[0x43; 32]),
    );
    refused(
        "another passkey's signature",
        &world,
        CREDENTIAL,
        &Signer::es256().assert(&CHALLENGE),
    );
    let mut gone = World::new(&signer);
    gone.rows
        .hosted("CredentialRemoved", T0 - 800, removed(ALICE, CREDENTIAL));
    refused("a removed passkey", &gone, CREDENTIAL, &good);
    let mut deactivated = World::new(&signer);
    deactivated
        .rows
        .hosted("MemberDeactivated", T0 - 600, member(ALICE));
    deactivated.standings.signed_in(ALICE, at(T0 - 100));
    refused("a deactivated member", &deactivated, CREDENTIAL, &good);
    let mut stranger = World::new(&signer);
    stranger.rows.hosted(
        "CredentialEnrolled",
        T0 - 850,
        enrolled(BOB, STRANGER, &signer.cose_key(), "es256"),
    );
    stranger.standings.signed_in(BOB, at(T0 - 500));
    refused("a member of no workspace", &stranger, STRANGER, &good);
    let mut unseen = World::new(&signer);
    unseen.standings = Standings::new();
    refused(
        "no sign-in since the process started",
        &unseen,
        CREDENTIAL,
        &good,
    );
    let mut changed = World::new(&signer);
    changed.rows.hosted(
        "MemberRoleChanged",
        T0 - 100,
        change(
            ALICE,
            &BTreeMap::from([(Role::Approver, T0 + 100)]),
            &BTreeSet::new(),
        ),
    );
    refused(
        "a membership record since the sign-in",
        &changed,
        CREDENTIAL,
        &good,
    );
    let mut keyless = World::new(&signer);
    keyless.keys = Some(BTreeMap::new());
    refused("a key artifact not stored", &keyless, CREDENTIAL, &good);
    keyless.keys = None;
    refused("no key store", &keyless, CREDENTIAL, &good);
    let mut corrupt = World::new(&signer);
    if let Some(keys) = corrupt.keys.as_mut() {
        keys.values_mut().for_each(|key| key[12] ^= 1);
    }
    refused(
        "a key whose bytes do not hash to their reference",
        &corrupt,
        CREDENTIAL,
        &good,
    );
    let mut mislabelled = World::new(&signer);
    mislabelled.rows = Rows::opened();
    mislabelled.rows.hosted(
        "MemberActivated",
        T0 - 1_000,
        activation(ALICE, &BTreeSet::from([Role::Operator]), T0 - 1_000),
    );
    mislabelled.rows.hosted(
        "CredentialEnrolled",
        T0 - 900,
        enrolled(ALICE, CREDENTIAL, &signer.cose_key(), "eddsa"),
    );
    refused(
        "an ES256 key recorded as eddsa",
        &mislabelled,
        CREDENTIAL,
        &good,
    );
    let ed25519 = Signer::ed25519();
    let mut recorded_es256 = World::new(&ed25519);
    recorded_es256.rows = Rows::opened();
    recorded_es256.rows.hosted(
        "MemberActivated",
        T0 - 1_000,
        activation(ALICE, &BTreeSet::from([Role::Operator]), T0 - 1_000),
    );
    recorded_es256.rows.hosted(
        "CredentialEnrolled",
        T0 - 900,
        enrolled(ALICE, CREDENTIAL, &ed25519.cose_key(), "es256"),
    );
    refused(
        "an Ed25519 key recorded as es256",
        &recorded_es256,
        CREDENTIAL,
        &ed25519.assert(&CHALLENGE),
    );
    let mut unread = World::new(&signer);
    unread.unreadable_credentials = true;
    refused(
        "passkey records that could not be read",
        &unread,
        CREDENTIAL,
        &good,
    );
    let mut unmembered = World::new(&signer);
    unmembered.unreadable_memberships = true;
    refused(
        "membership records that could not be read",
        &unmembered,
        CREDENTIAL,
        &good,
    );
}

#[test]
fn an_assertion_the_decoys_secret_signs_never_opens_even_for_a_member_named_like_the_decoy() {
    let signer = Signer::es256();
    let mut world = World::new(&signer);
    world.rows.hosted(
        "MemberActivated",
        T0 - 990,
        activation(DECOY_MEMBER, &BTreeSet::from([Role::Operator]), T0 - 990),
    );
    world.standings.signed_in(DECOY_MEMBER, at(T0 - 500));
    for decoy in Signer::decoys() {
        let assertion = decoy.assert(&CHALLENGE);
        assert_eq!(
            world.judge(STRANGER, &assertion),
            None,
            "{}: the decoy verified, and an unknown passkey still opens nothing",
            decoy.alg()
        );
    }
    assert_eq!(
        world.judge(CREDENTIAL, &signer.assert(&CHALLENGE)),
        opened_for_alice(),
        "the same world opens for the held passkey"
    );
}

#[test]
fn a_membership_reaches_the_workspace_while_active_or_cooling_off() {
    let mut rows = Rows::opened();
    let operator = BTreeSet::from([Role::Operator]);
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &operator, T0 - 100),
    );
    rows.hosted(
        "MemberActivated",
        T0 - 90,
        activation(BOB, &operator, T0 + 100),
    );
    let deactivated = PrincipalId(9);
    rows.hosted(
        "MemberActivated",
        T0 - 80,
        activation(deactivated, &operator, T0 - 80),
    );
    rows.hosted("MemberDeactivated", T0 - 70, member(deactivated));
    let gone = PrincipalId(10);
    rows.hosted(
        "MemberActivated",
        T0 - 60,
        activation(gone, &operator, T0 - 60),
    );
    rows.hosted("MemberDeactivated", T0 - 50, member(gone));
    rows.hosted("MemberRemoved", T0 - 40, member(gone));
    let lookup = rows.lookup(T0);
    assert_eq!(lookup.reaches(ALICE), Ok(true), "active");
    assert_eq!(lookup.reaches(BOB), Ok(true), "cooling off");
    assert_eq!(lookup.reaches(deactivated), Ok(false), "deactivated");
    assert_eq!(lookup.reaches(gone), Ok(false), "removed");
    assert_eq!(lookup.reaches(PrincipalId(11)), Ok(false), "never a member");
    let failed = ControlLookup::new(hosted(), Err(LookupFailed), at(T0));
    assert_eq!(failed.reaches(ALICE), Err(LookupFailed));
}
