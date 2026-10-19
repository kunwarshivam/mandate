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
    assert!(exec_port.take_code(&key()).unwrap().is_none());
    api_port
        .put_code(&key(), code_secret("the-code"), verifier("the-verifier"))
        .unwrap();
    let raw = executor.read(&code("ws/c1", "c1")).unwrap().unwrap();
    assert_eq!(
        raw.expose_secret(),
        &fields(&["the-code", "the-verifier"])[..]
    );
    let waiting = exec_port.take_code(&key()).unwrap().unwrap();
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
    assert!(exec_port.take_code(&key()).unwrap().is_none());
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
            port.take_code(&key()).map(|w| w.is_some()),
            Err(ConnectError::VaultUnavailable),
            "{connection} {environment:?}"
        );
    }
    let mut right = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let waiting = right.take_code(&key()).unwrap();
    assert!(
        waiting.is_some(),
        "a refused take puts the code back for its own connection"
    );
}

/// Of concurrent takers of one code, exactly one gets it and nothing is left behind (DEC-2628
/// item 2): the take is one rename, which only one of them can make.
#[test]
fn of_concurrent_takers_exactly_one_gets_the_code() {
    for round in 0..20 {
        let layout = Layout::new();
        let (api, executor) = layout.both();
        ApiPort {
            vault: &api,
            connection_id: conn("c1"),
            environment: Environment::Paper,
        }
        .put_code(&key(), code_secret("c"), verifier("v"))
        .unwrap();
        let got = std::thread::scope(|scope| {
            let takers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let mut port = ExecutorPort {
                            vault: &executor,
                            connection_id: conn("c1"),
                            environment: Environment::Paper,
                        };
                        port.take_code(&key()).unwrap().is_some()
                    })
                })
                .collect();
            takers
                .into_iter()
                .map(|taker| taker.join().unwrap())
                .filter(|got| *got)
                .count()
        });
        assert_eq!(got, 1, "round {round}");
        assert!(layout.names("pending").is_empty(), "round {round}");
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
        api_port.take_code(&key()).map(|w| w.is_some()),
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
        exec_port.take_code(&bad).map(|w| w.is_some()),
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
            exec_port.take_code(&key()).map(|w| w.is_some()),
            Err(ConnectError::VaultUnavailable),
            "{bad:?}"
        );
    }
}

/// A take whose rename fails for any reason but an absent entry is refused, never read as "no code
/// waiting": here the pending directory has been replaced by a file.
#[test]
fn a_take_the_vault_cannot_make_is_refused_not_none() {
    let layout = Layout::new();
    let (_api, executor) = layout.both();
    let pending = layout.vault.join("pending");
    std::fs::remove_dir_all(&pending).unwrap();
    std::fs::write(&pending, b"not a directory").unwrap();
    let mut port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    assert_eq!(
        port.take_code(&key()).map(|w| w.is_some()),
        Err(ConnectError::VaultUnavailable)
    );
}
