# Cell #1 hosted bring-up (E21-51)

- **Spec:** DEC-2001; DEC-995 (two DigitalOcean droplets, `nyc3`); DEC-967 items 6 to 9; DEC-990
  item 2; DEC-971 item 7; the managed-cells plan, H1 and H3.
- **Code:** `deploy/hosted/cell1.py` (the bring-up: `cloudflare`, `droplets`, `nodes`, `cluster`,
  `check`; `--dry-run`; `--target k3d`; `cargo xtask cell hosted` in `xtask/src/cell.rs`), `deploy/hosted/cell1.json` and `deploy/hosted/firewall.json`
  (settings), `deploy/hosted/node/` (what cloud-init installs), `deploy/hosted/values/` (the chart's
  values on top of `deploy/cell/values/k3s.yaml`), and the chart's `deploymentTier` value.
- **Tests:** `deploy/tests/test_hosted.py` (each invariant shown failing on a seeded bug; a dry run
  that calls nothing; recorded runs that find every secret on stdin only);
  `deploy/tests/test_cell.py::test_deployment_tier_is_set_from_values`.
- **Run:** `python3 deploy/hosted/cell1.py --dry-run all ...` (the plan, `deploy/hosted/RUNBOOK.md`);
  CI's `hosted-k3d` job, `cargo xtask cell hosted`, which runs `cell1.py --target k3d all`
  (needs docker, k3d, helm and kubectl).
