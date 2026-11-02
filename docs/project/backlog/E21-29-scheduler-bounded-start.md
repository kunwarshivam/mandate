- **E21-29c (Proposed, M8; SC)** As the founder, I want the scheduler to start in bounded memory, so that
  a long-lived workspace's clock stream never outgrows its pod. On 2026-11-02 staging's scheduler
  was OOMKilled at its 128Mi limit on restart with its code unchanged, as its stream of ticks grew;
  the limit was raised to 256Mi to bring staging back. *Accepted when:* the scheduler resumes from
  its stream's head (or a kept checkpoint) instead of folding every tick, and a test with a year of
  ticks starts it within a fixed memory budget.
<!-- section: E21 Operations and infrastructure -->
