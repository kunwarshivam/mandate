//! The closed enums other lanes route on carry exactly the names their specs list: the refusal
//! codes of identity spec §4.5 (DEC-643), the step-up kinds of workspace API §3.6, the client
//! scopes of §3.8, and the step-up methods of identity spec §7.3. The kinds and scopes are read
//! from the spec's text, not from the code.

use std::collections::BTreeSet;

use crate::{ClientScope, Refusal, StepUpActionKind, StepUpMethod};

const API: &str = include_str!("../../../../docs/specs/workspace-api.md");

/// The text of the section headed `heading`, up to the next heading of its level.
fn section(heading: &str) -> &'static str {
    API.split(heading)
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .unwrap_or_else(|| panic!("workspace API {heading}"))
}

fn backticked(text: &str) -> BTreeSet<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn each_refusal_has_its_spec_code() {
    for (refusal, code) in [
        (Refusal::NoMembership, "no_membership"),
        (Refusal::Forbidden, "forbidden"),
        (Refusal::InactivePermission, "inactive_permission"),
        (Refusal::ReductionOnly, "reduction_only"),
        (Refusal::MembershipUnavailable, "membership_unavailable"),
        (Refusal::OwnRoles, "own_roles"),
        (Refusal::OwnerRoleReserved, "owner_role_reserved"),
        (Refusal::LastOwner, "last_owner"),
        (Refusal::LastAdmin, "last_admin"),
        (Refusal::Unimplemented { story: "E9-2" }, "unimplemented"),
    ] {
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}

#[test]
fn the_step_up_kinds_are_workspace_api_3_6s_list() {
    let list = section("### 3.6 Step-up")
        .split("with that list:")
        .nth(1)
        .and_then(|rest| rest.split("There is no kind").next())
        .expect("§3.6's list of kinds");
    let codes: BTreeSet<String> = StepUpActionKind::ALL
        .iter()
        .map(|k| k.code().to_owned())
        .collect();
    assert_eq!(codes.len(), StepUpActionKind::ALL.len(), "codes repeat");
    assert_eq!(codes, backticked(list));
}

#[test]
fn the_client_scopes_are_workspace_api_3_8s_table() {
    let scopes: BTreeSet<String> = section("### 3.8 Client scopes")
        .lines()
        .filter(|l| l.starts_with("| `"))
        .filter_map(|l| backticked(l.split('|').nth(1).unwrap_or_default()).pop_first())
        .collect();
    let codes: BTreeSet<String> = ClientScope::ALL
        .iter()
        .map(|s| s.code().to_owned())
        .collect();
    assert_eq!(codes.len(), ClientScope::ALL.len(), "codes repeat");
    assert_eq!(codes, scopes);
}

#[test]
fn the_step_up_methods_have_their_journal_names() {
    assert_eq!(StepUpMethod::Passkey.code(), "passkey");
    assert_eq!(StepUpMethod::CliConfirm.code(), "cli_confirm");
}
