//! Unit tests of the policy table and the lease rule, and the conformance suite both backends
//! pass (`conformance`), run by `local_vault` and `openbao_server`.

mod conformance;
mod credentials;
mod debug;
mod errors;
mod fingerprint;
mod kube;
mod kubernetes;
mod lease;
mod local_vault;
mod mounted;
mod openbao;
mod openbao_server;
mod platform;
mod policy;
mod policy_prefix;
mod provisioner;
mod registry;
mod take;
