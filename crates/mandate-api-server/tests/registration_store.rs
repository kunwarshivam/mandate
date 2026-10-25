//! The challenge store's registration challenges called directly (workspace API §3.6, DEC-2118
//! item 1): an id the store holds answers its own principal and workspace only. Over HTTP the id
//! is derived from both, so only a direct call can present a held id under another of either.

use mandate_api_server::challenges::{Challenges, Withheld};
use mandate_identity::{AssertionId, PrincipalId, WorkspaceId};
use mandate_time::UtcNanos;

const WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W50";
const ID: &str = "7KXV6BGDW36KYC7RE5BPQG28Z0";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::from_ulid_text(text).expect("a ULID")
}

#[test]
fn a_held_id_answers_only_the_principal_and_workspace_it_was_issued_to() {
    let store = Challenges::default();
    let now = UtcNanos::from_parts(1_791_000_000, 0).expect("a time");
    let alice = PrincipalId(7);
    let issued = store
        .register(alice, workspace(WORKSPACE), AssertionId(ID.to_owned()), now)
        .expect("issued");
    assert_eq!(
        store.register(alice, workspace(WORKSPACE), AssertionId(ID.to_owned()), now),
        Ok(issued),
        "a retry answers the held challenge"
    );
    assert_eq!(
        store.register(
            PrincipalId(8),
            workspace(WORKSPACE),
            AssertionId(ID.to_owned()),
            now
        ),
        Err(Withheld::Conflict),
        "another principal"
    );
    assert_eq!(
        store.register(
            alice,
            workspace(OTHER_WORKSPACE),
            AssertionId(ID.to_owned()),
            now
        ),
        Err(Withheld::Conflict),
        "another workspace"
    );
}
