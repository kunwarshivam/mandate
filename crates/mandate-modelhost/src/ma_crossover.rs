//! `quant.ma_crossover`'s own code in the host: how its parameters are read and how its signal
//! becomes an output. With the crossover itself (`mandate-backtest`'s `strategy/ma_crossover.rs`),
//! it is one of the source files the model's content hash lists
//! ([DEC-504](../../../docs/project/decisions/DEC-504.md) item 1), so an edit here is an edit to the
//! model and needs a new model version.

/// The source files whose bytes the content object lists, by path from the repository root,
/// sorted, as this build embedded them (DEC-504 item 2).
pub(crate) const SOURCES: [(&str, &[u8]); 2] = [
    (
        "crates/mandate-backtest/src/strategy/ma_crossover.rs",
        include_bytes!("../../mandate-backtest/src/strategy/ma_crossover.rs"),
    ),
    (
        "crates/mandate-modelhost/src/ma_crossover.rs",
        include_bytes!("ma_crossover.rs"),
    ),
];
