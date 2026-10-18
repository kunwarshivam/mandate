//! A sealed vault or a dev server is refused before any login (DEC-977 items 5 and 6).

use crate::SourceError;
use reqwest::StatusCode;

use mandate_connections::record::ConnectionId;
use serde_json::json;

use crate::openbao::{refusal, role_settings, seal_verdict};
use crate::policy::Identity;

const UNSEALED_RAFT: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":3,"n":5,"storage_type":"raft"}"#;
const UNSEALED_DEV: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":1,"n":1,"storage_type":"inmem"}"#;
const SEALED: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":true,"t":3,"n":5,"storage_type":"raft"}"#;

#[test]
fn a_production_server_passes_and_a_dev_server_needs_the_test_allowance() {
    assert_eq!(seal_verdict(UNSEALED_RAFT, false), Ok(()));
    assert_eq!(seal_verdict(UNSEALED_RAFT, true), Ok(()));
    assert_eq!(
        seal_verdict(UNSEALED_DEV, false),
        Err(SourceError::DevServerRefused)
    );
    assert_eq!(seal_verdict(UNSEALED_DEV, true), Ok(()));
}

#[test]
fn a_sealed_or_unreadable_status_is_refused() {
    for allow_dev in [false, true] {
        assert_eq!(seal_verdict(SEALED, allow_dev), Err(SourceError::Sealed));
        assert_eq!(
            seal_verdict(
                br#"{"initialized":false,"sealed":false,"storage_type":"raft"}"#,
                allow_dev
            ),
            Err(SourceError::Sealed),
            "an uninitialized vault waits for its operator as a sealed one does"
        );
        for bad in [
            &b""[..],
            b"[]",
            br#"{"sealed":"no","storage_type":"raft"}"#,
            br#"{"storage_type":"raft"}"#,
            br#"{"sealed":false}"#,
            br#"{"sealed":false,"storage_type":7}"#,
        ] {
            assert_eq!(
                seal_verdict(bad, allow_dev),
                Err(SourceError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}

#[test]
fn only_the_vaults_own_refusals_are_refusals_and_the_rest_are_outages() {
    for code in [200_u16, 204, 299] {
        assert_eq!(
            refusal(StatusCode::from_u16(code).unwrap()),
            Ok(()),
            "{code}"
        );
    }
    for (code, want) in [
        (400_u16, SourceError::Denied),
        (401, SourceError::Denied),
        (403, SourceError::Denied),
        (404, SourceError::Missing),
        (503, SourceError::Sealed),
        (100, SourceError::Unavailable),
        (301, SourceError::Unavailable),
        (307, SourceError::Unavailable),
        (405, SourceError::Unavailable),
        (408, SourceError::Unavailable),
        (412, SourceError::Unavailable),
        (429, SourceError::Unavailable),
        (500, SourceError::Unavailable),
        (502, SourceError::Unavailable),
        (504, SourceError::Unavailable),
    ] {
        let got = refusal(StatusCode::from_u16(code).unwrap());
        assert_eq!(got, Err(want.clone()), "{code}");
        assert_eq!(
            got.is_err_and(|e| e.is_outage()),
            !matches!(want, SourceError::Denied | SourceError::Missing),
            "{code}"
        );
    }
}

#[test]
fn each_role_lives_as_long_as_its_process_type_may() {
    let c = ConnectionId::new("c1").unwrap();
    assert_eq!(
        role_settings(&Identity::TokenExchange(c.clone())),
        json!({"token_ttl": "600s", "token_max_ttl": "600s", "secret_id_num_uses": 1, "secret_id_ttl": "600s"})
    );
    assert_eq!(
        role_settings(&Identity::Executor(c)),
        json!({"token_ttl": "86400s", "token_max_ttl": "2592000s", "secret_id_num_uses": 0, "secret_id_ttl": "0s"})
    );
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
    ] {
        assert_eq!(
            role_settings(&identity),
            json!({"token_ttl": "86400s", "token_max_ttl": "86400s", "secret_id_num_uses": 0, "secret_id_ttl": "0s"}),
            "{identity:?}"
        );
    }
}
