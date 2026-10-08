//! The pending tests of E7-16's transport. They live inside the crate because plain `http` to a
//! loopback server is accepted only in this crate's own test build.

mod budget;
mod endpoint;
mod errors;
