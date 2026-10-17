# Release images, signing, and the start-up signature check (E21-35)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §7.1 steps 3 and 4
  (signing, promotion from `main`), OPS-15 (refuse an unsigned binary outside dev and CI), §11
  (no shell, non-root, read-only root); DEC-971.
- **Code:** `mandate-release`'s `src/startup.rs` (`Tier`, `ReleaseKey`, `decide`, `check`,
  `check_running`, `RELEASE_KEYS`); `mandate-agentd`'s `main` runs the check first;
  `xtask/src/image.rs` (the OCI layout of one binary, `push` over the distribution API, `unpack`);
  `xtask/src/release.rs` (`release images [--test-sign]`, `release drill`, `release publish`,
  cosign pinned); `.github/workflows/release.yml` (`build-a` builds and drills the images,
  `publish` on a push to `main` only).
- **Tests:** `crates/mandate-release/tests/startup.rs`: a release key starts everywhere; unsigned
  only in dev and CI; a test key only in dev and CI; a wrong, truncated or other-binary signature
  refused in every tier; no release key compiled in yet; a signature cosign itself wrote verifies
  (`tests/fixtures`); malformed PEMs refused; the check's files and variables, and the test key
  never read outside dev and CI; a property over random binaries, tiers and signers against a
  restated rule. `crates/mandate-agentd/tests/process.rs`: the unsigned test build refuses with no
  tier, `staging` and `production`, and a malformed signature in CI. `xtask/src/image.rs`'s tests:
  the image is a pure function of its inputs, the binary layer does not depend on the signature,
  non-root with the binary as the only entrypoint, content-addressed blobs (`sha256sum`), each
  layer exactly its one file (`tar`). `xtask/src/release.rs`: only the `main` publish job can write
  packages or mint an OIDC token.
- **Run:** `cargo nextest run -p mandate-release -p mandate-agentd`; `cargo test -p xtask -- release
  image`; after `cargo xtask release build <dist>`, `cargo xtask release images <dist> <out>
  --test-sign` and `cargo xtask release drill <dist> <out>`.
