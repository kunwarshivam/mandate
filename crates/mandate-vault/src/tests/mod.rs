//! Unit tests of the policy table and the lease rule, and the conformance suite both backends
//! pass (`conformance`), run by `local_vault` and `openbao_server`.

mod conformance;
mod debug;
mod errors;
mod lease;
mod local_vault;
mod openbao;
mod openbao_server;
mod platform;
mod policy;
