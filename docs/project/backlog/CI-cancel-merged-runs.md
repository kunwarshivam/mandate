- **CI-cancel-merged-runs (Proposed; lane RN; CODEOWNERS: `.github/` is founder-owned)** As an
  agent, I want a merged pull request's remaining runs of `ci`, CodeQL, `release-check` and `web`
  cancelled when it merges, so that they stop holding CI's slots: on 2026-10-27 about 200 queued
  runs from merged pull requests kept every new run waiting for hours. *Accepted when:* the
  workflow that merges a pull request (`merge.yml`, or a job beside it) cancels that pull
  request's queued and in-progress runs with `gh run cancel`, holding `actions: write` in that
  one job only; a test pins the job and its permission; and agents who merge through the API
  cancel the same way, or the job runs on `pull_request: closed` instead. Not a job-level
  concurrency group, whose reach into another workflow's run is undocumented (#2778).
