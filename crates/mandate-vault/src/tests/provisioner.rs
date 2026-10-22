//! The role provisioner's refusals (DEC-2350 item 1): every vault error is one of §9.33's two
//! `ConnectionRolesRefused` reasons, an outage or a sealed vault `vault_unavailable` and every
//! other refusal `role_refused`.

use crate::SourceError;
use crate::provisioner::RoleRefusal;

#[test]
#[ignore = "pending E21-33"]
fn every_vault_error_is_one_of_the_two_reasons() {
    for (error, refusal) in [
        (SourceError::Unavailable, RoleRefusal::VaultUnavailable),
        (SourceError::Sealed, RoleRefusal::VaultUnavailable),
        (SourceError::Denied, RoleRefusal::RoleRefused),
        (SourceError::Missing, RoleRefusal::RoleRefused),
        (SourceError::Malformed, RoleRefusal::RoleRefused),
        (SourceError::Config, RoleRefusal::RoleRefused),
        (SourceError::RoleMissing, RoleRefusal::RoleRefused),
    ] {
        assert_eq!(RoleRefusal::of(&error), refusal, "{error:?}");
    }
    assert_eq!(RoleRefusal::VaultUnavailable.reason(), "vault_unavailable");
    assert_eq!(RoleRefusal::RoleRefused.reason(), "role_refused");
}
