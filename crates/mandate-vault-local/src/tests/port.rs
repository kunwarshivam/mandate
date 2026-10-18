//! The connect core's vault port over the local vault: the API writes a code, the executor
//! exchanges it for a token, and each role is refused the calls DEC-692 item 1 does not give it.

use mandate_connections::ConnectError;
use mandate_connections::grant::check_scope;
use mandate_connections::record::Environment;
use mandate_connections::start::PkceVerifier;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey};
use secrecy::{ExposeSecret, SecretString};

use super::fixtures::{code, conn, token};
use super::layout::Layout;
use crate::port::{ApiPort, ExecutorPort};

fn key() -> VaultKey {
    VaultKey("ws/c1".to_owned())
}

fn code_secret(text: &str) -> AuthorizationCode {
    AuthorizationCode(SecretString::from(text.to_owned()))
}

fn verifier(text: &str) -> PkceVerifier {
    PkceVerifier(SecretString::from(text.to_owned()))
}

fn access(text: &str) -> AccessToken {
    AccessToken(SecretString::from(text.to_owned()))
}

/// The oracle for a stored field list: each a two-byte big-endian length and its bytes.
fn fields(parts: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(&u16::try_from(part.len()).unwrap().to_be_bytes());
        out.extend_from_slice(part.as_bytes());
    }
    out
}

#[test]
fn a_code_goes_in_through_the_api_and_out_through_the_executor() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let mut api_port = ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let mut exec_port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    assert!(exec_port.read_code(&key()).unwrap().is_none());
    api_port
        .put_code(&key(), code_secret("the-code"), verifier("the-verifier"))
        .unwrap();
    let raw = executor.read(&code("ws/c1", "c1")).unwrap().unwrap();
    assert_eq!(
        raw.expose_secret(),
        &fields(&["the-code", "the-verifier"])[..]
    );
    let waiting = exec_port.read_code(&key()).unwrap().unwrap();
    assert_eq!(waiting.code.0.expose_secret(), "the-code");
    assert_eq!(waiting.verifier.0.expose_secret(), "the-verifier");

    let scopes = check_scope("trading data").unwrap();
    exec_port
        .put_token(&key(), access("the-token"), &scopes)
        .unwrap();
    let raw = executor.read(&token("ws/c1", "c1")).unwrap().unwrap();
    assert_eq!(
        raw.expose_secret(),
        &fields(&["the-token", "data trading"])[..]
    );
    exec_port.delete_code(&key()).unwrap();
    assert!(exec_port.read_code(&key()).unwrap().is_none());
    assert_eq!(layout.names("tokens").len(), 1);
}

#[test]
fn a_code_bound_to_another_connection_or_environment_is_refused() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    }
    .put_code(&key(), code_secret("c"), verifier("v"))
    .unwrap();
    for (connection, environment) in [("c2", Environment::Paper), ("c1", Environment::Live)] {
        let mut port = ExecutorPort {
            vault: &executor,
            connection_id: conn(connection),
            environment,
        };
        assert_eq!(
            port.read_code(&key()).map(|w| w.is_some()),
            Err(ConnectError::VaultUnavailable),
            "{connection} {environment:?}"
        );
    }
}

#[test]
fn delete_removes_the_code_and_the_token_from_either_role() {
    for api_deletes in [true, false] {
        let layout = Layout::new();
        let (api, executor) = layout.both();
        let mut api_port = ApiPort {
            vault: &api,
            connection_id: conn("c1"),
            environment: Environment::Paper,
        };
        let mut exec_port = ExecutorPort {
            vault: &executor,
            connection_id: conn("c1"),
            environment: Environment::Paper,
        };
        api_port
            .put_code(&key(), code_secret("c"), verifier("v"))
            .unwrap();
        exec_port
            .put_token(&key(), access("t"), &check_scope("data trading").unwrap())
            .unwrap();
        if api_deletes {
            api_port.delete(&key()).unwrap();
        } else {
            exec_port.delete(&key()).unwrap();
        }
        assert!(
            layout.names("pending").is_empty(),
            "api_deletes {api_deletes}"
        );
        assert!(
            layout.names("tokens").is_empty(),
            "api_deletes {api_deletes}"
        );
        api_port.delete(&key()).unwrap();
        api_port.delete_code(&key()).unwrap();
    }
}

#[test]
fn each_role_is_refused_the_calls_it_does_not_have() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let mut api_port = ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let mut exec_port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let scopes = check_scope("trading data").unwrap();
    assert_eq!(
        api_port.read_code(&key()).map(|w| w.is_some()),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(
        api_port.put_token(&key(), access("t"), &scopes),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(
        exec_port.put_code(&key(), code_secret("c"), verifier("v")),
        Err(ConnectError::VaultUnavailable)
    );
    assert!(layout.names("pending").is_empty());
    assert!(layout.names("tokens").is_empty());
}

#[test]
fn an_invalid_vault_key_is_refused_by_every_call() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let bad = VaultKey("not a key".to_owned());
    let scopes = check_scope("trading data").unwrap();
    let mut api_port = ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let mut exec_port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let unavailable = Err(ConnectError::VaultUnavailable);
    assert_eq!(
        api_port.put_code(&bad, code_secret("c"), verifier("v")),
        unavailable
    );
    assert_eq!(api_port.delete_code(&bad), unavailable);
    assert_eq!(api_port.delete(&bad), unavailable);
    assert_eq!(
        exec_port.read_code(&bad).map(|w| w.is_some()),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(exec_port.put_token(&bad, access("t"), &scopes), unavailable);
    assert_eq!(exec_port.delete_code(&bad), unavailable);
    assert_eq!(exec_port.delete(&bad), unavailable);
}

#[test]
fn a_code_entry_that_does_not_parse_is_refused() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let mut exec_port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let entry = code("ws/c1", "c1");
    let mut trailing = fields(&["c", "v"]);
    trailing.push(0);
    let mut long = vec![0xFF, 0xFF];
    long.extend_from_slice(b"short");
    for bad in [
        vec![0_u8],
        fields(&["only-one"]),
        trailing,
        long,
        fields(&["", "v"]),
        fields(&["c", ""]),
        vec![0, 2, 0xC3, 0x28, 0, 1, b'v'],
    ] {
        api.put_code(&entry, &secrecy::SecretSlice::from(bad.clone()))
            .unwrap();
        assert_eq!(
            exec_port.read_code(&key()).map(|w| w.is_some()),
            Err(ConnectError::VaultUnavailable),
            "{bad:?}"
        );
    }
}
