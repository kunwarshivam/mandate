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
//! The mandate as code: the document, its validation, the policy hierarchy, the risk state, risk
//! days, goals, the condition language, and change classification
//! ([mandate spec](../../../docs/specs/mandate.md);
//! [task brief](../../../docs/project/tasks/M5-F-mandate-spec.md), DEC-128).
//!
//! **These are the rules the gate enforces, so they sit below the gate.** `mandate-risk` (layer 4)
//! reads this crate and cannot be where the limits are defined; that is what makes AGENTS.md rule 1
//! — "no code path may let an agent act outside its mandate" — a property of the crate graph rather
//! than of a convention (DEC-128 item 1). [`ValidatedMandate`] carries it one step further: it has a
//! single constructor, so a gate cannot be handed a document nobody validated.
//!
//! Everything here is pure. No clock, no randomness, no I/O, `BTreeMap` and `BTreeSet` throughout,
//! and every duration an integer count of seconds carried in by its caller (ES-21). The risk state
//! is a transition function over values: the account sub-ledger fold stays in `mandate-accounting`
//! and the executor wires the two together, so one crate owns each number (DEC-128 item 1).
//!
//! Arithmetic is `mandate-num`'s (ES-04). The comparisons §5.2 calls exact are exact: they
//! cross-multiply on 256-bit intermediates and return a boolean, so no product is ever materialised
//! and no scale limit can decide whether a risk limit fires (DEC-128 item 5).

mod dec;

pub mod change;
pub mod condition;
pub mod context;
pub mod document;
pub mod draft;
pub mod goal;
pub mod policy;
pub mod risk;
pub mod validate;

pub use dec::{DecGrammar, GrammarMismatch, SchemaDec};
pub use document::{Mandate, MandateVersion, Pointer};
pub use validate::{ValidatedMandate, ValidationContext, ValidationReport, Violation, Warning};

use mandate_domain::DomainError;
use mandate_num::NumError;
use mandate_time::TimeError;

/// Why a document is not a mandate. Every variant names the JSON Pointer it failed at, so a
/// rejection tells an author where to look (ES-09).
///
/// These are the 31 MC-S cases: a strict parse is what "passes the JSON Schema" means in Rust, and
/// ES-22 requires it to accept exactly what `jsonschema` accepts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("`{path}` is not a member the schema allows here")]
    UnknownMember { path: Pointer },
    #[error("`{path}` is required and missing")]
    MissingMember { path: Pointer },
    #[error("`{path}` must be a decimal string, not a JSON number")]
    DecimalAsNumber { path: Pointer },
    #[error("`{path}` is not of the type the schema gives it")]
    WrongType { path: Pointer },
    #[error("`{path}` is not one of the values the schema lists")]
    NotInEnum { path: Pointer },
    #[error("`{path}` does not match the pattern the schema gives it")]
    OffPattern { path: Pointer },
    #[error("`{path}` is outside the bounds the schema gives it")]
    OutOfBounds { path: Pointer },
    #[error("`{path}` is not in the `{grammar}` grammar the schema declares for it", grammar = grammar.as_str())]
    OffGrammar { path: Pointer, grammar: DecGrammar },
    /// An array the schema marks `uniqueItems` holds the same value twice; the path names the
    /// second one.
    #[error("`{path}` repeats an earlier item of an array whose items must be unique")]
    NotUnique { path: Pointer },
    /// A condition nests past the one level beyond V-017's limit that the parse reads, so V-017 can
    /// still report the first level too deep and nothing deeper is ever recursed into (DEC-151).
    #[error("conditions nest deeper than the schema allows")]
    TooDeep { path: Pointer },
    /// The public fields no longer hold what the document they were parsed from says, so there is no
    /// canonical form to hash: a version must name the mandate the gate enforces, never a stale one.
    #[error("the mandate's fields were changed after it was parsed, so it has no canonical form")]
    Diverged,
    /// The stubs of this story's tests PR return this, so every pending test fails on them
    /// (DEC-77, DEC-83); the implementation PR replaces the stubs and removes the variant.
    #[error("the mandate parser is not implemented yet")]
    Unimplemented,
    #[error(transparent)]
    Domain(#[from] DomainError),
}

impl ParseError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownMember { .. } => "unknown_member",
            Self::MissingMember { .. } => "missing_member",
            Self::DecimalAsNumber { .. } => "decimal_as_number",
            Self::WrongType { .. } => "wrong_type",
            Self::NotInEnum { .. } => "not_in_enum",
            Self::OffPattern { .. } => "off_pattern",
            Self::OutOfBounds { .. } => "out_of_bounds",
            Self::OffGrammar { .. } => "off_grammar",
            Self::NotUnique { .. } => "not_unique",
            Self::TooDeep { .. } => "too_deep",
            Self::Diverged => "diverged",
            Self::Unimplemented => "unimplemented",
            Self::Domain(e) => e.code(),
        }
    }
}

/// Why a rule could not be evaluated, or an input could not be applied.
///
/// [`SpecError::OutOfRange`] is the one that needs saying out loud: a mandate can be schema-valid and
/// still hold a decimal no exact arithmetic type in the workspace can multiply, because the schema
/// bounds no scale. That is an error naming the path, **not** a V-code — the document broke no rule
/// the spec states, and a report must never carry a code no specification defines (DEC-128 item 4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    #[error("`{path}` holds a decimal this arithmetic cannot represent exactly: {cause}")]
    OutOfRange { path: Pointer, cause: NumError },
    #[error("the risk clock went backwards")]
    ClockWentBackwards,
    #[error("no such restriction")]
    UnknownRestriction,
    /// An input that is not a market fact but a caller's mistake: a quantity increment of zero, where
    /// neither answer is safe (nothing is ever below a zero increment, so the goal would buy forever;
    /// treating it as done would stop a goal that is not done).
    #[error("`{what}` is not a usable value")]
    InvalidInput { what: &'static str },
    /// The stubs of a story's tests PR return this, so every pending test fails on them (DEC-77,
    /// DEC-83). A story implemented in slices returns it for whatever its landed slices do not fold
    /// yet, never a silent answer, and the variant goes with the last slice (DEC-167 item 5).
    #[error("this rule is not implemented yet")]
    Unimplemented,
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    Domain(#[from] DomainError),
}

impl SpecError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutOfRange { .. } => "out_of_range",
            Self::ClockWentBackwards => "clock_went_backwards",
            Self::UnknownRestriction => "unknown_restriction",
            Self::InvalidInput { .. } => "invalid_input",
            Self::Unimplemented => "unimplemented",
            Self::Parse(e) => e.code(),
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
            Self::Domain(e) => e.code(),
        }
    }
}
