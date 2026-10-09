# The CLI's configuration and model registrations (E10-16, D1)

- **Spec:** `docs/specs/journal.md` §9.2 (`ConfigSnapshotRegistered` versions 1 and 2); the first
  paper trade brief (D1); DEC-504 item 3, DEC-523, DEC-526.
- **Code:** `crates/mandate-cli/src/config.rs` (`register`, which stores any kind's object and
  registers it, holding the instrument snapshot to exactly DEC-523's object; `register_model`,
  whose content and hash come only from `mandate_modelhost::content`; both paper only), stubbed.
- **Tests:** `crates/mandate-cli/tests/config_register.rs`, pending E10-16, each committed draft
  read back through `Draft::parse`.
- **The commands (D1b, DEC-527):** `crates/mandate-cli/src/register.rs` (`OwnerArgs`, the
  required `--workspace` and `--user`, always in paper; `run` and `run_model` over P0's
  `--journal` and `--store`), stubbed, and `main`; `crates/mandate-cli/tests/register_commands.rs`,
  pending E10-16: the flags, the refusals without a database, and the binary against Postgres.
- **`workspace open` (D1c, DEC-527 items 7 and 8):** `crates/mandate-cli/src/workspace.rs` (`open`,
  the control stream's one `StreamOpened` as the `control_services` opener, in paper), stubbed;
  `crates/mandate-cli/tests/workspace_open.rs`, pending E10-16 but for the live flags test.
- **Run:** `cargo nextest run -p mandate-cli --test config_register --test register_commands`;
  `cargo xtask ci pending`; `cargo xtask ci postgres` for the binary test.
