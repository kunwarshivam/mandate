- **E21-32v (Proposed, M8; lane EX)** As the coordinator, I want a seeded cell job's one vault
  provisioning denial explained and, if it is a startup race, closed, so that the job fails only
  on a real policy fault. Evidence: CI run 40564301498 on c23ef195, job `cell-failover (seeded)`
  printed `vault: unsealed, on raft storage` at 19:16:58.87, then 0.3 s later `Error: this
  identity's policy does not allow that (denied)` and `provisioning the vault failed`. The same
  job passed on e1477d2 and 6a5ca93, which change no vault, provisioner or policy file. The cause
  is unconfirmed: a race after unseal is suspected, but `mandate-vault` maps only a 400, 401 or
  403 to `Denied` (a vault not yet active answers 503, 429 or a redirect, read as `Sealed` or
  `Unavailable`), so the vault itself refused the request that the `mandate` CLI sent, with the
  root token, from `provision_vault` in the k3s node.
  *Accepted when:* the provisioning's failure names the HTTP status and the path refused; the cause
  is found from a reproduction or that record; and if it is a startup race, `xtask/src/cell.rs`
  waits for the vault's own readiness (`bao status`, read through `vault_state`, as active and
  unsealed) within a bound of 60 s before `provision_vault` runs, with a test pinning the wait,
  so the seeded job stays within its ten minutes (`AGENTS.md`). A denial is never retried.
<!-- section: E21 Operations and infrastructure -->
