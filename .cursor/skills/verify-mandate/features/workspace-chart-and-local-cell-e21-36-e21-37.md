# Workspace chart and local cell (E21-36, E21-37)

- **Spec:** infrastructure design §3.1 to §3.3, §7.2 (OPS-13), §9 (OPS-5);
  the managed-cells plan (lanes K and C, PR #1431); DEC-967, DEC-990, DEC-993, DEC-1040.
- **Code:** `deploy/cell/charts/workspace/` (the chart, rendered from
  `deploy/cell/charts/workspace/processes.json`), `deploy/cell/charts/journal-db/` (the journal's
  Postgres under CloudNativePG), `deploy/cell/compose/compose.json` (generated), and
  `xtask/src/cell.rs` (`cargo xtask cell up`, `drill`, `demo`, `down`, `generate`; `cargo xtask ci cell`); the image source on a host that cannot build (`MANDATE_CELL_IMAGE`).
- **Tests:** `deploy/tests/test_cell.py` (each chart and Compose invariant, each shown failing on a
  seeded bug), `deploy/tests/test_artifacts.py` (the shared artifact store's mode in each pod); the
  drills `boot`, `journal`, `egress` and `compose`, each also run with its bug planted, and
  `kill-switch` and `admission` (DEC-2045) in `cargo xtask ci cell-kill-switch` (DEC-2602), and
  `executor` (`xtask/src/cell/executor.rs`, DEC-2035) in `cargo xtask ci cell-executor` (DEC-2615; CI's shards, DEC-2641),
  and `vault` in `cargo xtask ci cell-vault` (DEC-2616); `restart` runs in
  `cargo xtask ci cell-failover` (E21-41)
  (`cargo xtask cell drill --seeded`).
- **Run:** `cargo xtask ci cell` (needs docker, k3d, helm and kubeconform; `deploy/cell/README.md`);
  the lint job runs `cargo xtask cell generate`, the drift check of the Compose and roles files.
