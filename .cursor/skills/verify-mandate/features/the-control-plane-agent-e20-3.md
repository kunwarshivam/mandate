# The control-plane agent's inbound checks (E20-3)

- **Spec:** [control-plane design](../../../../docs/design/control-plane.md) §3, §3.8, CP-1, CP-4,
  CP-5 (a) and (c); journal spec §9.17 (the refusal reasons); DEC-982.
- **Code:** `mandate-cp-agent` (`crates/mandate-cp-agent`). `src/instruction.rs` (the five closed inbound bodies:
  `License`, `ReleaseManifest`, `ReleaseWithdrawn`, `CatalogEntry`, `DataBundle`; `Ident`,
  `Sequence`), `src/verify.rs` (`PinnedKeys`, `MessageKind`, `RefusalReason`, `Verified` with
  private fields, `Refusal`, `signed_bytes`, the 64 KiB cap and the canonical-form check),
  `src/port.rs` (`InstructionSink`, which workspace services implement; `HandOver`; `receive`).
- **Tests:** `tests/verify.rs`: each kind signed by its pinned key is verified with its key and
  digest; only the canonical form is verified; the size cap; unsigned, wrongly signed, tampered,
  key-substituted, unpinned, out-of-list, malformed and misaddressed messages refused with their
  reasons; a known answer for the signed bytes; pinning each key once; a property that only a
  pinned signature is ever verified.
- **Run:** `cargo nextest run -p mandate-cp-agent`.
