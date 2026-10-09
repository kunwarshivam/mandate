# Behaviour-only pending tests

The pending tests that fail on the answer a partly implemented crate gives rather than at a stub,
named one by one so the exception cannot spread. This directory was the `BEHAVIOUR_ONLY_TESTS`
array in `xtask/src/main.rs` until every tests PR appending to that one array conflicted with
every other.

DEC-110's rule still holds for each of them: it must run and must fail. Each row goes when its
story lands, and `cargo xtask ci pending` names every row it applies (DEC-137). E6-6 and E6-8
(DEC-163) retired the `mandate-risk` rows with the session rules, §5.3 rule 4 and the pacing that
decided them.

The stub check runs first, so a row whose test stops at a stub is reported for deletion rather
than applied (#194 review, round 1, finding 4). A row naming a test that is no longer pending in
its file is reported for deletion too: deleting the row is how the exception expires.

## One file a row

Each row is one TOML file here, so adding a row adds a file and removing one deletes a file, and
two PRs never touch the same lines:

```toml
file = "crates/<crate>/tests/<suite>.rs"
test = "<test path as the pending marker names it, module::name>"
reason = """
Why this test fails on behaviour rather than at its story's stub, and what deletes the row.
"""
```

The file is named `<crate>__<suite>__<test>.toml`, with `::` in the test path written `__`:
`crates/mandate-executor/tests/hand.rs`'s `outlier_close` would be
`mandate-executor__hand__outlier_close.toml`. A row may also name xtask's own unit tests:
`file = "xtask/src/main.rs"`, with a test in its `tests` module, named as crate `xtask` and suite
`main`, so `tests::name` is `xtask__main__tests__name.toml`. No other file outside
`crates/<crate>/` may be named. `cargo xtask ci pending` refuses a file whose name
does not match its contents, a file with any other field or a missing or empty one, a duplicate
row, and anything here other than this README and `.toml` rows.
