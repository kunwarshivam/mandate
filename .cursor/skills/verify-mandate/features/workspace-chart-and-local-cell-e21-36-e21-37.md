# Workspace chart and local cell (E21-36, E21-37)

- **Spec:** infrastructure design §3.1 to §3.3, §7.2 (OPS-13), §9 (OPS-5);
  the managed-cells plan (lanes K and C, PR #1431); DEC-967, DEC-990, DEC-993.
- **Code:** `deploy/cell/charts/workspace/` (the chart, rendered from
  `deploy/cell/charts/workspace/processes.json`), `deploy/cell/charts/journal-db/` (the journal's
  Postgres under CloudNativePG), `deploy/cell/compose/compose.json` (generated), and
  `xtask/src/cell.rs` (`cargo xtask cell up`, `drill`, `demo`, `down`, `generate`; `cargo xtask ci cell`).
- **Tests:** `deploy/tests/test_cell.py` (each chart and Compose invariant, each shown failing on a
  seeded bug); the drills `boot`, `journal` and `egress`, each also run with its bug planted
  (`cargo xtask cell drill --seeded`).
- **Run:** `cargo xtask ci cell` (needs docker, k3d, helm and kubeconform; `deploy/cell/README.md`);
  the lint job runs `cargo xtask cell generate`, the drift check of the Compose and roles files.
