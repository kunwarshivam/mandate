//! The start-up signature check (E21-35, DEC-971, OPS-15): outside dev and CI a process starts
//! only when its executable's cosign signature verifies against a compiled-in release key; in dev
//! and CI an unsigned build starts and a test key is honoured; a signature no trusted key verifies
//! is refused in every tier. Keys are generated here with `ring`, and one fixture was signed by
//! cosign itself, so the check reads what cosign writes.

use std::fs;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use mandate_release::startup::{self, RELEASE_KEYS, SIGNATURE_VAR, TEST_KEY_VAR, TIER_VAR, decide};
use mandate_release::{Admitted, BuildDigest, Refused, ReleaseKey, Tier, Trust};
use proptest::prelude::*;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair as _};

const TIERS: [Tier; 4] = [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production];
const SPKI_P256_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

/// A signing key and its cosign-shaped public PEM.
struct Signer {
    pair: EcdsaKeyPair,
    pem: String,
}

impl Signer {
    fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .unwrap_or_else(|e| panic!("generate: {e}"));
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                .unwrap_or_else(|e| panic!("pkcs8: {e}"));
        let mut spki = SPKI_P256_PREFIX.to_vec();
        spki.extend_from_slice(pair.public_key().as_ref());
        let body = STANDARD.encode(&spki);
        let pem = format!("-----BEGIN PUBLIC KEY-----\n{body}\n-----END PUBLIC KEY-----\n");
        Self { pair, pem }
    }

    fn key(&self) -> ReleaseKey {
        ReleaseKey::from_pem(&self.pem).unwrap_or_else(|e| panic!("pem: {e}"))
    }

    /// The signature as cosign's `--output-signature` file holds it: base64 of the DER signature.
    fn sign(&self, message: &[u8]) -> String {
        let signature = self
            .pair
            .sign(&SystemRandom::new(), message)
            .unwrap_or_else(|e| panic!("sign: {e}"));
        STANDARD.encode(signature.as_ref())
    }
}

const RELEASE: &[u8] = b"the bytes of a release binary";

fn admitted(trust: Trust) -> Result<Admitted, Refused> {
    Ok(Admitted {
        digest: BuildDigest::of_bytes(RELEASE),
        trust,
    })
}

#[test]
fn no_setting_and_an_unknown_setting_are_production() {
    assert_eq!(Tier::from_setting(None), Tier::Production);
    for unknown in ["", "prod", "DEV", "Ci", " dev", "local", "test"] {
        assert_eq!(Tier::from_setting(Some(unknown)), Tier::Production, "{unknown:?}");
    }
    assert_eq!(Tier::from_setting(Some("dev")), Tier::Dev);
    assert_eq!(Tier::from_setting(Some("ci")), Tier::Ci);
    assert_eq!(Tier::from_setting(Some("staging")), Tier::Staging);
    assert_eq!(Tier::from_setting(Some("production")), Tier::Production);
}

/// No release key is wired until the founder's hardware key exists (H8), so nothing passes the
/// check outside dev and CI yet.
#[test]
fn no_release_key_is_compiled_in_yet() {
    assert!(RELEASE_KEYS.is_empty());
}

#[test]
fn a_release_signature_starts_in_every_tier() {
    let release = Signer::new();
    let signature = release.sign(RELEASE);
    for tier in TIERS {
        assert_eq!(
            decide(tier, RELEASE, Some(&signature), &[release.key()], None),
            admitted(Trust::Release),
            "{tier:?}"
        );
    }
}

#[test]
fn an_unsigned_build_starts_only_in_dev_and_ci() {
    let release = Signer::new();
    for tier in TIERS {
        let expected = match tier {
            Tier::Dev | Tier::Ci => admitted(Trust::Unsigned),
            Tier::Staging | Tier::Production => Err(Refused::Unsigned),
        };
        assert_eq!(decide(tier, RELEASE, None, &[release.key()], None), expected, "{tier:?}");
    }
}

#[test]
fn a_test_key_is_honoured_only_in_dev_and_ci() {
    let release = Signer::new();
    let test = Signer::new();
    let signature = test.sign(RELEASE);
    for tier in TIERS {
        let expected = match tier {
            Tier::Dev | Tier::Ci => admitted(Trust::Test),
            Tier::Staging | Tier::Production => Err(Refused::NotVerified),
        };
        assert_eq!(
            decide(tier, RELEASE, Some(&signature), &[release.key()], Some(&test.key())),
            expected,
            "{tier:?}"
        );
    }
}

#[test]
fn a_wrong_signature_is_refused_in_every_tier() {
    let release = Signer::new();
    let test = Signer::new();
    let stranger = Signer::new();
    let cases = [
        ("a stranger's key", stranger.sign(RELEASE)),
        ("another binary", release.sign(b"another binary")),
        ("a truncated signature", {
            let full = STANDARD.decode(release.sign(RELEASE)).unwrap_or_default();
            STANDARD.encode(&full[..full.len() - 1])
        }),
    ];
    for tier in TIERS {
        for (what, signature) in &cases {
            assert_eq!(
                decide(tier, RELEASE, Some(signature), &[release.key()], Some(&test.key())),
                Err(Refused::NotVerified),
                "{what} in {tier:?}"
            );
        }
    }
}

#[test]
fn a_signature_that_is_not_base64_is_malformed_in_every_tier() {
    let release = Signer::new();
    for tier in TIERS {
        assert_eq!(
            decide(tier, RELEASE, Some("not base64 !"), &[release.key()], None),
            Err(Refused::Malformed),
            "{tier:?}"
        );
    }
}

/// With no release key at all, as now, a signed build is refused outside dev and CI too.
#[test]
fn without_a_release_key_nothing_starts_outside_dev_and_ci() {
    let test = Signer::new();
    let signature = test.sign(RELEASE);
    for tier in [Tier::Staging, Tier::Production] {
        assert_eq!(decide(tier, RELEASE, None, &[], Some(&test.key())), Err(Refused::Unsigned));
        assert_eq!(
            decide(tier, RELEASE, Some(&signature), &[], Some(&test.key())),
            Err(Refused::NotVerified)
        );
    }
}

#[test]
fn a_signature_file_may_end_in_a_newline() {
    let release = Signer::new();
    let signature = format!("{}\n", release.sign(RELEASE));
    assert_eq!(
        decide(Tier::Production, RELEASE, Some(&signature), &[release.key()], None),
        admitted(Trust::Release)
    );
}

/// A signature cosign itself wrote (`cosign sign-blob --key … --output-signature`) over the
/// fixture blob verifies against cosign's own PEM public key.
#[test]
fn a_cosign_signature_verifies() {
    let key = ReleaseKey::from_pem(include_str!("fixtures/cosign.pub"))
        .unwrap_or_else(|e| panic!("cosign pem: {e}"));
    let blob = include_bytes!("fixtures/blob");
    let signature = include_str!("fixtures/blob.sig");
    assert_eq!(
        decide(Tier::Production, blob, Some(signature), &[key.clone()], None),
        Ok(Admitted {
            digest: BuildDigest::of_bytes(blob),
            trust: Trust::Release
        })
    );
    let mut altered = blob.to_vec();
    altered.push(b'!');
    assert_eq!(
        decide(Tier::Production, &altered, Some(signature), &[key], None),
        Err(Refused::NotVerified)
    );
}

#[test]
fn a_pem_that_is_not_a_p256_public_key_is_malformed() {
    let good = Signer::new().pem;
    let body: String = good
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let mut der = STANDARD.decode(&body).unwrap_or_default();
    let point_byte = der.len() - 1;
    let mut wrong_curve = der.clone();
    wrong_curve[20] ^= 0x01;
    der[point_byte] ^= 0x01;
    let pem = |bytes: &[u8]| {
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(bytes)
        )
    };
    for bad in [
        String::new(),
        body.clone(),
        good.replace("PUBLIC KEY", "PRIVATE KEY"),
        pem(&wrong_curve),
        pem(&der[..der.len() - 1]),
        "-----BEGIN PUBLIC KEY-----\n!!!\n-----END PUBLIC KEY-----\n".to_owned(),
    ] {
        assert_eq!(ReleaseKey::from_pem(&bad), Err(Refused::Malformed), "{bad:?}");
    }
    assert_eq!(ReleaseKey::from_pem(&good).map(|k| k.point().len()), Ok(65));
}

#[test]
fn every_code_is_generic_and_distinct() {
    let refusals = [
        Refused::Unsigned,
        Refused::NotVerified,
        Refused::Malformed,
        Refused::Unreadable(std::io::ErrorKind::NotFound),
    ];
    let codes: Vec<&str> = refusals.iter().map(Refused::code).collect();
    assert_eq!(
        codes,
        [
            "release_unsigned",
            "release_signature_not_verified",
            "release_signature_malformed",
            "release_unreadable"
        ]
    );
    let trusts: Vec<&str> = [Trust::Release, Trust::Test, Trust::Unsigned]
        .iter()
        .map(Trust::code)
        .collect();
    assert_eq!(trusts, ["release", "test", "unsigned"]);
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mandate-release-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("scratch: {e}"));
    dir
}

/// The check reads the signature beside the executable unless a file is named, and the test key
/// only in dev and CI, from the variables it is handed.
#[test]
fn the_check_reads_its_inputs_from_files_and_variables() {
    let dir = scratch("check");
    let executable = dir.join("process");
    fs::write(&executable, RELEASE).unwrap_or_else(|e| panic!("{e}"));
    let beside = dir.join("process.sig");
    let test = Signer::new();
    let key_file = dir.join("test.pub");
    fs::write(&key_file, &test.pem).unwrap_or_else(|e| panic!("{e}"));
    let key_path = key_file.display().to_string();
    let vars = |tier: &'static str, signature: Option<String>| {
        let key_path = key_path.clone();
        move |name: &str| match name {
            TIER_VAR => Some(tier.to_owned()),
            TEST_KEY_VAR => Some(key_path.clone()),
            SIGNATURE_VAR => signature.clone(),
            _ => None,
        }
    };
    assert_eq!(
        startup::check(&vars("ci", None), &executable, &beside),
        admitted(Trust::Unsigned)
    );
    assert_eq!(
        startup::check(&vars("staging", None), &executable, &beside),
        Err(Refused::Unsigned)
    );
    fs::write(&beside, test.sign(RELEASE)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        startup::check(&vars("ci", None), &executable, &beside),
        admitted(Trust::Test)
    );
    assert_eq!(
        startup::check(&vars("production", None), &executable, &beside),
        Err(Refused::NotVerified)
    );
    let named = dir.join("named.sig");
    fs::write(&named, Signer::new().sign(RELEASE)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        startup::check(&vars("ci", Some(named.display().to_string())), &executable, &beside),
        Err(Refused::NotVerified)
    );
    assert_eq!(
        startup::check(
            &vars("ci", Some(dir.join("absent.sig").display().to_string())),
            &executable,
            &beside
        ),
        Err(Refused::Unreadable(std::io::ErrorKind::NotFound))
    );
    assert_eq!(
        startup::check(&vars("ci", None), &dir.join("absent"), &beside),
        Err(Refused::Unreadable(std::io::ErrorKind::NotFound))
    );
}

/// Outside dev and CI the test key's file is never read: a missing one changes nothing.
#[test]
fn outside_dev_and_ci_the_test_key_is_not_read() {
    let dir = scratch("nokey");
    let executable = dir.join("process");
    fs::write(&executable, RELEASE).unwrap_or_else(|e| panic!("{e}"));
    let vars = |name: &str| match name {
        TIER_VAR => Some("staging".to_owned()),
        TEST_KEY_VAR => Some(dir.join("absent.pub").display().to_string()),
        _ => None,
    };
    assert_eq!(
        startup::check(&vars, &executable, &dir.join("process.sig")),
        Err(Refused::Unsigned)
    );
}

proptest! {
    /// Over random binaries, tiers and signers: the check admits exactly when an unsigned build
    /// runs in dev or CI, or the signature is over these bytes by the release key, or by the test
    /// key in dev or CI; the oracle restates the rule from who signed what.
    #[test]
    fn the_check_admits_exactly_what_the_rule_allows(
        bytes in proptest::collection::vec(any::<u8>(), 1..256),
        other in proptest::collection::vec(any::<u8>(), 1..256),
        tier_at in 0usize..4,
        signer_at in 0usize..4,
        over_other in any::<bool>(),
        give_test_key in any::<bool>(),
    ) {
        prop_assume!(bytes != other);
        let tier = TIERS[tier_at];
        let (release, test, stranger) = (Signer::new(), Signer::new(), Signer::new());
        let message = if over_other { &other } else { &bytes };
        let signature = match signer_at {
            0 => None,
            1 => Some(release.sign(message)),
            2 => Some(test.sign(message)),
            _ => Some(stranger.sign(message)),
        };
        let test_key = give_test_key.then(|| test.key());
        let lenient = matches!(tier, Tier::Dev | Tier::Ci);
        let expected = match (signer_at, over_other) {
            (0, _) if lenient => Ok(Trust::Unsigned),
            (0, _) => Err(Refused::Unsigned),
            (1, false) => Ok(Trust::Release),
            (2, false) if lenient && give_test_key => Ok(Trust::Test),
            _ => Err(Refused::NotVerified),
        };
        let got = decide(tier, &bytes, signature.as_deref(), &[release.key()], test_key.as_ref());
        prop_assert_eq!(got.map(|a| (a.digest, a.trust)), expected.map(|t| (BuildDigest::of_bytes(&bytes), t)));
    }
}
