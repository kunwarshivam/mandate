//! A canary secret goes through every path, and no `Debug` output, error, or file on disk shows
//! it (CN-1, OPS-1, AGENTS.md rule 7).

use std::fs;

use mandate_connections::record::Environment;
use mandate_connections::vault::{AuthorizationCode, Vault, VaultKey};
use secrecy::{ExposeSecret, SecretString};

use super::fixtures::{
    CANARY, assert_no_canary, assert_no_canary_bytes, client_secret, code, conn, secret, token,
};
use super::layout::Layout;
use crate::import::import_client_secret;
use crate::port::{ApiPort, ExecutorPort};
use crate::startup::Role;
use crate::store::open;

#[test]
fn the_canary_never_leaves_the_vault_in_the_clear() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let c = code("ws/c1", "c1");
    let t = token("ws/c1", "c1");
    api.put_code(&c, &secret(CANARY)).unwrap();
    executor.put(&t, &secret(CANARY)).unwrap();
    import_client_secret(&executor, &mut CANARY.as_bytes()).unwrap();

    for dir in ["pending", "tokens"] {
        for name in layout.names(dir) {
            assert_no_canary(&name, "a file name");
            let bytes = fs::read(layout.vault.join(dir).join(&name)).unwrap();
            assert_no_canary_bytes(&bytes, &name);
        }
    }

    let read = executor.read(&t).unwrap().unwrap();
    assert_eq!(read.expose_secret(), CANARY.as_bytes());
    let outputs = [
        format!("{api:?}"),
        format!("{executor:?}"),
        format!("{read:?}"),
        format!("{:?}", executor.read(&client_secret()).unwrap()),
        format!("{:?}", open(&layout.inputs(Role::Executor)).unwrap()),
    ];
    for output in outputs {
        assert_no_canary(&output, "Debug");
    }

    let mut api_port = ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    api_port
        .put_code(
            &VaultKey("ws/c1".to_owned()),
            AuthorizationCode(SecretString::from(CANARY.to_owned())),
            mandate_connections::start::PkceVerifier(SecretString::from(CANARY.to_owned())),
        )
        .unwrap();
    let mut exec_port = ExecutorPort {
        vault: &executor,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let waiting = exec_port
        .take_code(&VaultKey("ws/c1".to_owned()))
        .unwrap()
        .unwrap();
    assert_eq!(waiting.code.0.expose_secret(), CANARY);
    assert_no_canary(&format!("{waiting:?}"), "WaitingCode");
    assert_no_canary(&format!("{api_port:?}"), "ApiPort");
    assert_no_canary(&format!("{exec_port:?}"), "ExecutorPort");
}

#[test]
fn no_refusal_carries_the_canary() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t = token("ws/c1", "c1");
    executor.put(&t, &secret(CANARY)).unwrap();
    let path = layout.file(&t);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path, &bytes).unwrap();

    let mut refusals = vec![
        executor.read(&t).map(|_| ()).unwrap_err(),
        api.put_code(&t, &secret(CANARY)).unwrap_err(),
        executor
            .put(&code("ws/c1", "c1"), &secret(CANARY))
            .unwrap_err(),
        import_client_secret(&executor, &mut format!("{CANARY} {CANARY}").as_bytes()).unwrap_err(),
        import_client_secret(&executor, &mut format!("{CANARY}\0").as_bytes()).unwrap_err(),
    ];
    refusals.push(executor.put(&t, &secret(&CANARY.repeat(400))).unwrap_err());
    for error in &refusals {
        assert_no_canary(&format!("{error:?}"), "an error's Debug");
        assert_no_canary(&error.to_string(), "an error's Display");
    }

    let mut api_port = ApiPort {
        vault: &api,
        connection_id: conn("c1"),
        environment: Environment::Paper,
    };
    let refused = api_port
        .put_code(
            &VaultKey("bad key".to_owned()),
            AuthorizationCode(SecretString::from(CANARY.to_owned())),
            mandate_connections::start::PkceVerifier(SecretString::from(CANARY.to_owned())),
        )
        .unwrap_err();
    assert_no_canary(&format!("{refused:?} {refused}"), "a port refusal");
}
