# The reproducible release build, its SBOMs, and the build digest (E21-8)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §7.1 step 2 (a
  `--locked` static build, `SOURCE_DATE_EPOCH`, a double-build diff, an SBOM and cargo-auditable
  metadata), OPS-15 (the build digest as `actor.build`); [journal spec](../../../../docs/specs/journal.md)
  §3 (the `actor.build` content reference); DEC-972.
- **Code:** `mandate-release`, in `crates/mandate-release` (`BuildDigest`: the SHA-256 of an executable's bytes,
  `running()` from `/proc/self/exe`, the `sha256:<hex>` reference and its parser);
  `xtask/src/release.rs` (`cargo xtask release tools`, `release build <out>`, `release compare
  <a> <b>`: the shipped binaries, the remapped flags, the pinned cargo-auditable and syft, the
  `SHA256SUMS` format and its comparison); `.github/workflows/release.yml` (two builds of one
  commit on two runners in two checkout paths, compared).
- **Tests:** `crates/mandate-release/tests/digest.rs`: FIPS 180-2 vectors, a file's digest and
  the running test binary's digest against `sha2` over the bytes read independently, a missing
  file refused without its path, and properties that every digest round-trips through its
  reference and that any other prefix is refused. `xtask/src/release.rs`'s tests: the flags remap
  every machine path and differ between checkouts only by the path; the build is `--locked`,
  musl, every shipped package, with no feature flag; no shipped package may declare `live`; the
  `SHA256SUMS` format, its refusals, and a comparison that refuses one differing, missing or extra
  binary; every tool pinned to a GitHub release asset with a SHA-256 and a registry row at its
  version; and the build refuses, before anything runs, rustflags, compiler, wrapper and profile
  variables and any cargo configuration it reads that sets a profile, rustflags or a compiler or
  wrapper, with no `--config` on its own command line (DEC-2027).
- **Run:** `cargo nextest run -p mandate-release`; `cargo test -p xtask release`; for the build
  itself, `cargo xtask release build <dir>` twice from two checkouts, then `cargo xtask release
  compare <dir-a> <dir-b>` (about three minutes each on four cores).
