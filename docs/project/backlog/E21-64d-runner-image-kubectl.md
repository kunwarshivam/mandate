- **E21-64d (Should, before the founder sets `CI_RUNNER`; lane RN)** As the founder, I want the CI
  runners' job image to carry everything the cell jobs use from GitHub's `ubuntu-24.04` image, so
  that no cell job fails the first time it runs on the self-hosted pool. `cell demo` needs
  `kubectl` for its port-forward and to point the API at the run's workspace, and since #2699 it
  refuses to start without it; GitHub's image ships `kubectl`, but the job image's build installs
  none. *Accepted when:* `modules/ci-runners`' image build installs `kubectl` pinned and
  sha256-verified, at `infra.yml`'s `KUBECTL_VERSION` and `KUBECTL_SHA256`, kept in step with them;
  the image also has docker, k3d and helm; and every cell job, `cell-demo` included, passes on a
  self-hosted runner before `CI_RUNNER` is set for all of them (DEC-2065, DEC-2072).
<!-- section: E21 Operations and infrastructure -->
