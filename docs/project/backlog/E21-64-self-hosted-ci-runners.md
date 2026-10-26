- **E21-64 (Done, lane RN; DEC-2065)** As the founder, I want CI's cell jobs on self-hosted,
  ephemeral runners on DigitalOcean within 150 USD a month, so that CI is not starved by GitHub's
  shared runners. *Accepted when:* `deploy/tofu/environments/ci-runners` plans three runners at
  144 USD; the cell jobs run on `CI_RUNNER` only for `main` and this repository's branches; and
  `docs/runbooks/ci-runners.md` lists the founder's steps and the rollback.
- **E21-64a (Proposed; lane RN)** As the founder, I want the CI runners' egress limited to named
  hosts (GitHub, the Rust, Python and npm registries, the container registries, Ubuntu's archive),
  so that a job cannot reach any other address. *Accepted when:* the job containers reach the
  internet only through a host-side proxy that admits those hosts, and a job's request to any other
  host fails.
- **E21-64b (Proposed; lane RN)** As the founder, I want the CI runners to pull Docker Hub images
  through a pull-through cache on each host, k3s's own pulls included, so that Docker Hub's
  per-address limit never fails a cell job. *Accepted when:* the cell's k3d cluster and the job's
  Docker daemon use the cache as their docker.io mirror, and a run of all eleven cell jobs makes at
  most one Docker Hub pull per image per host per day.
- **E21-64c (Proposed; lane RN; needs the founder)** As the founder, I want the runners' App to need
  less than `Administration: write`, so that a key taken from a runner cannot change the
  repository's settings. *Accepted when:* the repository is in an organization, the runners are in
  a runner group of their own, and the App holds only the organization's "Self-hosted runners"
  permission.
