# Dependency registry

Every direct dependency of a workspace crate or Python project needs a row here, added in the same
change that introduces it ([ADR-0001](adr/0001-engineering-setup.md) ES-14). `cargo xtask deps`
fails on any direct dependency without a row, which also blocks misspelled or invented package
names. This file is founder-owned (CODEOWNERS): adding a row is the approval.

Safety-critical crates may also use only the external crates listed in their `allowed_external`
entry in `xtask/layers.toml`.

| Name | Ecosystem | Used by | Purpose | Alternatives considered | License | Approved |
|---|---|---|---|---|---|---|
| `anyhow` | cargo | xtask | Error context in tooling (binaries only, ES-09) | Plain `Box<dyn Error>` | MIT OR Apache-2.0 | DEC-72 |
| `serde` | cargo | xtask | Deserializing `cargo metadata` and TOML policy files | Hand parsing | MIT OR Apache-2.0 | DEC-72 |
| `serde_json` | cargo | xtask | Reading `cargo metadata` output (never on the hashing path, ES-07) | `json` crate | MIT OR Apache-2.0 | DEC-72 |
| `toml` | cargo | xtask | Reading `xtask/layers.toml` and `pyproject.toml` | `toml_edit` | MIT OR Apache-2.0 | DEC-72 |
| `pyyaml` | python | mandate-tools | Loading reference-case YAML exactly as the reference implementation does (ES-11) | ruamel.yaml (YAML 1.2 typing differs) | MIT | DEC-72 |
| `pytest` | python | python workspace (dev) | Test runner | unittest | MIT | DEC-72 |
| `ruff` | python | python workspace (dev) | Lint and format | flake8 + black | MIT | DEC-72 |
