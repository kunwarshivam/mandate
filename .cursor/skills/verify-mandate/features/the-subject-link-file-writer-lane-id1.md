# The subject-link file's writer: `mandate identity link` (lane ID1)

- **Spec:** [workspace API spec](../../../../docs/specs/workspace-api.md) §3.11 item 7 and
  DEC-978 item 7 (the file the API server reads); identity spec §3.1 and §3.3; DEC-992.
- **Code:** `mandate-cli`, `crates/mandate-cli/src/identity.rs`: `subject_digest`, `parse_links`,
  `render_links`, `with_link`, `link_file` (exclusive staging file, sync, rename), and `link`,
  which reads the subject from standard input.
- **Tests:** `crates/mandate-cli/tests/identity_link.rs`: the digest recomputed from hand-written
  canonical bytes, the arguments (no subject argument), the file's text and mode, idempotence and
  `subject_linked_elsewhere`, refusals of unsafe and malformed files, a busy writer, the binary over
  standard input, and a property against a model of its own.
- **Run:** `cargo nextest run -p mandate-cli --test identity_link`.
