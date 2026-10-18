# GitOps for staging and production on DOKS (E21-56)

- **Spec:** DEC-2015 (Flux, read-only from `main`; every chart by OCI digest; interim workload
  secrets from a GitHub environment); the founder's DEC-2011; infrastructure design §7.1; DEC-971
  item 6.
- **Code:** `xtask/src/release.rs` (`release publish-charts`: `HOSTED_CHARTS`, `chart_version`,
  `pushed_digest`); `.github/workflows/release.yml` (`publish-index` publishes and signs the charts
  after the index, on a push to `main` only, and keeps `CHARTS`).
- **Tests:** `xtask/src/release.rs`: a chart's version carries the commit as build metadata; only a
  well-formed SHA-256 digest from `helm push` is taken; every chart under `deploy/cell/charts/` is
  published or named as the local cell's alone; only the `main` publish jobs can write packages or
  sign.
- **Run:** `cargo test -p xtask -- release`.
