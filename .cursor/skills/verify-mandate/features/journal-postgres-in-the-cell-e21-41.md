# The journal's Postgres in the cell: synchronous standby, WAL archive, narrowed egress (E21-41)

- **Spec:** infrastructure design §2.1, §4.1, OPS-2; journal spec §5.3; the managed-cells plan (lane
  S2); DEC-967 item 8, DEC-973 item 6, DEC-1050.
- **Code:** `deploy/cell/charts/journal-db/` (`templates/cluster.yaml`: the synchronous standby, the
  failure-domain rule and annotation, the narrowed egress; `templates/archive.yaml`: the Barman
  Cloud plugin's ObjectStore and the scheduled base backup), `deploy/cell/charts/backup-plugin-tls/`
  (the plugin's certificates, in place of cert-manager), `deploy/cell/charts/archive-store/` (the
  local cell's stand-in for the S1 endpoint), and `xtask/src/cell.rs` (`cargo xtask cell up`
  installs the plugin and the store; the `standby-down` and `archive` drills; `cargo xtask ci
  cell-journal`).
- **Tests:** `deploy/tests/test_journal_db.py` (each invariant, each shown failing on a seeded bug,
  and the values that must be refused); the drills `standby-down` and `archive`, each also run with
  its bug planted (`cargo xtask cell drill --seeded standby-down archive`).
- **Run:** `cargo xtask ci cell-journal` (needs docker, k3d, helm and kubeconform;
  `deploy/cell/README.md`); the chart tests also run in `cargo xtask ci cell`'s chart checks and in
  the Python tests.
