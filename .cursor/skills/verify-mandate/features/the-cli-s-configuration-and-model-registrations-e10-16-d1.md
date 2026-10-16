# The CLI's configuration and model registrations (E10-16, D1)

- **Spec:** `docs/specs/journal.md` §9.2 (`ConfigSnapshotRegistered` versions 1 and 2); the first
  paper trade brief (D1); DEC-504 item 3, DEC-523, DEC-526; DEC-952 (the live workspace).
- **Code:** `crates/mandate-cli/src/config.rs` (`register`, which stores any kind's object and
  registers it, holding the instrument snapshot to exactly DEC-523's object; `register_model`,
  whose content and hash come only from `mandate_modelhost::content`; both in paper, or in live
  only in a live workspace, DEC-952), stubbed.
- **Tests:** `crates/mandate-cli/tests/config_register.rs`, pending E10-16, each committed draft
  read back through `Draft::parse`.
- **The commands (D1b, DEC-527):** `crates/mandate-cli/src/register.rs` (`OwnerArgs`, the
  required `--workspace` and `--user`, and `--environment`, `paper` by default (DEC-952); `run`
  and `run_model` over P0's
  `--journal` and `--store`), stubbed, and `main`; `crates/mandate-cli/tests/register_commands.rs`,
  pending E10-16: the flags, the refusals without a database, and the binary against Postgres.
- **`workspace open` (D1c, DEC-527 items 7 and 8):** `crates/mandate-cli/src/workspace.rs` (`open`,
  the control stream's one `StreamOpened` as the `control_services` opener, in paper or, with
  `--environment live`, in live), stubbed; `crates/mandate-cli/tests/workspace_open.rs`, pending
  E10-16 but for the live flags test.
- **The live workspace (R1b, DEC-952):** `crates/mandate-cli/src/control.rs` (`same_environment`:
  an owner's event on a control stream of the other environment is refused
  `workspace_environment` before anything is stored, minted or appended);
  `crates/mandate-cli/tests/live_workspace.rs` (a registration, an answer and a caused record in
  each environment, and every owner command's `--environment`) and `workspace_open.rs`'s
  `a_live_workspace_is_opened_live_and_never_shares_its_id` over Postgres.
- **Run:** `cargo nextest run -p mandate-cli --test config_register --test register_commands
  --test live_workspace --test workspace_open`;
  `cargo xtask ci pending`; `cargo xtask ci postgres` for the binary test.
