#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The simulator harness of `mandate-shell`'s Robinhood connector (E7-6, DEC-928 item 4). It has
//! no code: its tests drive `mandate_shell::robinhood` against `mandate-rh-sim`'s `SimServer`.
//!
//! They cannot live in `mandate-rh-sim`, the Robinhood connector's own harness: a harness judges
//! the crate at the top of what it drives (DEC-866), so taking `mandate-shell` there would make
//! the shell the top and leave `mandate-robinhood`'s mutants unjudged by its simulator tests. Nor
//! in `mandate-shell`, since no product crate may depend on the simulator (the first live trade
//! brief). Its name ends in `-sim` so the mutation run finds it as the shell's harness.
