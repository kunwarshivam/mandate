# The global control plane's first slice (E21-43, E20-1, E20-2, E20-5)

- **Spec:** [control-plane design](../../../../docs/design/control-plane.md) §1.2, §3.1, §3.2,
  §3.4, CP-1, CP-4, CP-7, CP-8, CP-11; DEC-984; DEC-985 (Proposed, where it runs).
- **Code:** `mandate-cp` (`crates/mandate-cp`). `src/directory.rs` (`ControlPlane` over the `Store`
  port: `create_deployment`, `enroll`, `authenticate`, `heartbeat`, `fleet`, `sync_directory`,
  `routes`; `MemoryStore`; `Ident`, `Endpoint`), `src/request.rs` (`Signed`,
  `signed_request_bytes`, the 300-second skew), `src/fleet.rs` (`Heartbeat`, `Component`, `Health`,
  `FleetRow`, `Reachability`).
- **Tests:** `tests/control_plane.rs`: a single-use, expiring token binds the key generated on site;
  only the enrolled key within the skew authenticates; a deployment silent for a day is
  unreachable and nothing is revoked; a deployment writes only its own directory; routing by
  active membership; no record has a field for personal data; endpoint and ID grammars; a key given
  at creation is not bound; a property that only the enrolled key authenticates.
- **Run:** `cargo nextest run -p mandate-cp`.
