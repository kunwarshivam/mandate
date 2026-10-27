- **E21-32v (Proposed, M8; lane EX)** As the coordinator, I want the cell's vault provisioning to
  start only once the vault answers as active, so that a seeded cell job never fails on a denial in
  the first second after unseal. Evidence: CI run 40564301498 on c23ef195, job `cell-failover
  (seeded)` printed `vault: unsealed, on raft storage` at 19:16:58.87, then 0.3 s later
  `Error: this identity's policy does not allow that (denied)` (`mandate-vault`'s
  `error.rs`) and `provisioning the vault failed`. The same job passed on e1477d2 and 6a5ca93,
  which change no vault, provisioner or policy file, so the denial is a race after unseal, not a
  policy. *Accepted when:* `xtask/src/cell.rs` waits, after unseal and the pod's `Ready`, for the
  vault's `sys/health` to answer 200 (active, unsealed) within a bound before `provision_vault`
  runs, or retries the provisioning once on a denial within that bound; and a test pins the wait
  or the single retry, so a second denial still fails the job.
<!-- section: E21 Operations and infrastructure -->
