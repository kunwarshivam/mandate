//! Every refusal has a stable code, and only an outage keeps a lease.

use crate::SourceError;

#[test]
fn every_error_has_its_code_and_only_outages_are_outages() {
    let cases = [
        (SourceError::Denied, "denied", false),
        (SourceError::Unavailable, "unavailable", true),
        (SourceError::Sealed, "sealed", true),
        (SourceError::DevServerRefused, "dev_server_refused", false),
        (SourceError::Malformed, "malformed", false),
        (SourceError::Integrity, "integrity", false),
        (SourceError::Missing, "missing", false),
        (SourceError::GrantInvalid, "grant_invalid", false),
        (SourceError::Config, "config", false),
        (SourceError::CredentialFile, "credential_file", false),
        (SourceError::RoleMissing, "role_missing", false),
        (
            SourceError::Unimplemented { story: "E21-32" },
            "unimplemented",
            false,
        ),
    ];
    for (error, code, outage) in cases {
        assert_eq!(error.code(), code, "{error:?}");
        assert_eq!(error.is_outage(), outage, "{error:?}");
    }
}
