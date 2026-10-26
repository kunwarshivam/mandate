//! The platform's own secrets in the table (DEC-2020): the table and the OpenBao policies it
//! generates, against an oracle written out by hand, over random identities and connections.

use mandate_connections::record::ConnectionId;
use proptest::prelude::*;

use super::conformance::{platform_holders, the_platform_secrets_have_their_holders};
use crate::SourceError;
use crate::policy::{
    Access, Identity, Mounts, PlatformSecret, SecretPath, allows, policy_name, require, role_name,
};

/// [`crate::policy::kv_path`], which fails only for an id it cannot place; every id here is one it
/// places.
fn kv_path(path: &SecretPath) -> String {
    crate::policy::kv_path(path).unwrap()
}

/// [`crate::policy::openbao_policy`], which fails only where [`kv_path`] does.
fn openbao_policy(identity: &Identity, connections: &[ConnectionId], mounts: &Mounts) -> String {
    crate::policy::openbao_policy(identity, connections, mounts).unwrap()
}

/// The oracle: who may do what to a platform secret, its own way: one reader each, the secret
/// import the only writer, and no deleter.
fn may(identity: &Identity, access: Access, secret: PlatformSecret) -> bool {
    matches!(
        (identity, access, secret),
        (
            Identity::ModelGateway,
            Access::Read,
            PlatformSecret::ModelProviderKey
        ) | (
            Identity::NewsReader,
            Access::Read,
            PlatformSecret::NewsDataKey
        ) | (
            Identity::EmailSender,
            Access::Read,
            PlatformSecret::EmailCredential
        ) | (Identity::SecretImport, Access::Write, _)
    )
}

fn connection() -> impl Strategy<Value = ConnectionId> {
    "c[a-z0-9]{0,6}".prop_map(|id| ConnectionId::new(&id).unwrap())
}

fn identity() -> impl Strategy<Value = Identity> {
    prop_oneof![
        Just(Identity::Api),
        Just(Identity::ConnectionManager),
        connection().prop_map(Identity::TokenExchange),
        connection().prop_map(Identity::Executor),
        Just(Identity::Runtime),
        Just(Identity::SecretImport),
        Just(Identity::ModelGateway),
        Just(Identity::NewsReader),
        Just(Identity::EmailSender),
        Just(Identity::Provisioner),
    ]
}

fn access() -> impl Strategy<Value = Access> {
    prop_oneof![
        Just(Access::Read),
        Just(Access::Write),
        Just(Access::Delete)
    ]
}

fn secret() -> impl Strategy<Value = PlatformSecret> {
    prop_oneof![
        Just(PlatformSecret::ModelProviderKey),
        Just(PlatformSecret::EmailCredential),
        Just(PlatformSecret::NewsDataKey)
    ]
}

#[test]
fn each_platform_secret_has_one_reader_and_only_import_writes_it() {
    the_platform_secrets_have_their_holders();
}

/// The new identities and paths have pinned names, so a policy or a stored path never moves.
#[test]
fn the_platform_names_are_pinned() {
    assert_eq!(role_name(&Identity::ModelGateway), "model-gateway");
    assert_eq!(role_name(&Identity::EmailSender), "email-sender");
    assert_eq!(role_name(&Identity::NewsReader), "news-reader");
    assert_eq!(
        policy_name(&Identity::ModelGateway, &Mounts::default()),
        "mandate-model-gateway"
    );
    assert_eq!(
        kv_path(&SecretPath::Platform(PlatformSecret::ModelProviderKey)),
        "platform/model-provider-key"
    );
    assert_eq!(
        kv_path(&SecretPath::Platform(PlatformSecret::EmailCredential)),
        "platform/email-credential"
    );
    assert_eq!(
        kv_path(&SecretPath::Platform(PlatformSecret::NewsDataKey)),
        "platform/news-data-key"
    );
}

/// The `path` blocks of an HCL policy that name a platform secret, as (path, capabilities).
fn platform_blocks(hcl: &str) -> Vec<(String, String)> {
    hcl.split("path \"")
        .skip(1)
        .filter_map(|block| {
            let (path, rest) = block.split_once('"')?;
            let caps = rest.split_once('[')?.1.split_once(']')?.0;
            path.contains("/platform/")
                .then(|| (path.to_owned(), caps.replace([' ', '"'], "")))
        })
        .filter(|(path, _)| !path.ends_with("alpaca-oauth-client-secret"))
        .collect()
}

proptest! {
    /// The table and its refusal agree with the oracle for every identity, access and platform
    /// secret: an allowed access passes [`require`], and anything else is [`SourceError::Denied`].
    #[test]
    fn the_table_holds_each_platform_secret_to_the_oracle(
        identity in identity(),
        access in access(),
        secret in secret(),
    ) {
        let path = SecretPath::Platform(secret);
        let want = may(&identity, access, secret);
        let required = require(&identity, access, &path);
        prop_assert_eq!(
            required.clone(),
            if want { Ok(()) } else { Err(SourceError::Denied) },
            "{:?} {:?} {:?}: {:?}", identity, access, secret, required
        );
        prop_assert_eq!(allows(&identity, access, &path), want);
    }

    /// Each generated policy, whatever connections the cell holds, grants exactly the oracle's
    /// platform paths, spelled out, with no wildcard: one read block for each reader and one
    /// write block per secret for the import.
    #[test]
    fn each_policy_grants_exactly_the_oracles_platform_paths(
        identity in identity(),
        connections in proptest::collection::vec(connection(), 0..4),
    ) {
        for secret in PlatformSecret::ALL {
            prop_assert_eq!(
                crate::policy::holders(secret),
                Ok(platform_holders(secret)),
                "{:?}", secret
            );
        }
        let hcl = openbao_policy(&identity, &connections, &Mounts::default());
        let prefixed = matches!(identity, Identity::Api | Identity::ConnectionManager);
        prop_assert!(!hcl.contains('*') && (prefixed || !hcl.contains('+')), "{}", hcl);
        let mut want = Vec::new();
        for secret in PlatformSecret::ALL {
            let path = kv_path(&SecretPath::Platform(secret));
            if may(&identity, Access::Read, secret) {
                want.push((format!("mandate/data/{path}"), "read".to_owned()));
            }
            if may(&identity, Access::Write, secret) {
                want.push((format!("mandate/data/{path}"), "create,update".to_owned()));
            }
        }
        let mut got = platform_blocks(&hcl);
        got.sort();
        want.sort();
        prop_assert_eq!(got, want, "{:?}: {}", identity, hcl);
    }
}
