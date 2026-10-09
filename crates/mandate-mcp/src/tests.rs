//! The pending tests of E7-16's transport. They live inside the crate because plain `http` to a
//! loopback server is accepted only in this crate's own test build.

mod answers;
mod bounds;
mod budget;
mod contract;
mod drift;
mod endpoint;
mod errors;
mod server;
