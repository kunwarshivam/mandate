# Review rules for the Mandate repository

Flag these as bugs, citing the file and line. They come from `AGENTS.md`; the build does not catch
them yet.

- An order intent, submission, or other state change without a journal event written before the
  side effect it authorizes (AGENTS.md rules 5 and "How to work" 6).
- Money, quantity, or price handled as `f32`/`f64`, parsed with `rust_decimal`'s `FromStr`, or
  written to the journal as a JSON number.
- A clock read, randomness, `HashMap`, or `HashSet` in a pure-core crate (`xtask/layers.toml`).
- Order details, positions, prices, mandate content, or credentials in a log line, error message,
  or notification payload (AGENTS.md rules 6 and 7).
- A code path that lets an agent act outside its mandate, adds risk without approval, or resolves a
  timeout toward more risk (AGENTS.md rules 1 to 3).
- A risk exit, protective order, or kill switch that conduct controls, eligibility, day-trade
  budgets, buying power, or session rules could deny (AGENTS.md rule 13).
- A test whose assertion would still pass if the code under test returned a default value, or an
  oracle that reuses the implementation's own predicate.
- A protected path (`docs/specs/`, `schemas/`, `reference/`, `fixtures/refcases/`,
  `crates/mandate-refcases/status.toml`) changed in the same PR as code.
- An implementation PR that edits test files beyond deleting `#[ignore = "pending <story>"]` lines
  (DEC-77).
- A new direct dependency without a row in `docs/dependencies.md`, or a live-trading host,
  credential, or `live` feature.

Treat style preferences as non-blocking suggestions.
