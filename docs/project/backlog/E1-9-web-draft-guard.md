- **E1-9w (Proposed; lane DR, CI capacity)** As a builder sharing GitHub's runners, I want
  `web.yml` to start no job on a draft pull request, as `ci.yml` already does (DEC-610), so that a
  draft touching `web/` does not hold a runner for its roughly eight-minute `web` job.
  *Accepted when:* `web.yml` runs on `ready_for_review` too and its `web` job carries
  `if: ${{ !github.event.pull_request.draft }}`; `.github/scripts/merge-approved.sh` accepts a
  `web.yml` run only when its `web` job ran and passed, not on the run's `success` alone, since a
  run whose only job was skipped reports `success` (today it reads the conclusion alone); the
  script's `gh` stand-in in `xtask/src/main.rs` answers the jobs of `web`'s latest run as it does
  `ci`'s, with a case that refuses a skipped `web` job; a test pins the guard as
  `ci_runs_nothing_on_a_draft` does; and DEC-610 is amended to name `web.yml`. Deferred by the
  coordinator on 2026-10-27: the capacity gain (drafts touching only `web/`) is small next to a
  change to the merge script.
<!-- section: E1 Foundations -->
