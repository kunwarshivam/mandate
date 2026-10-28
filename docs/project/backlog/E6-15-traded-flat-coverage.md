- **E6-15-traded-flat-coverage (Proposed; debt; lane X)** As the founder, I want DEC-2833's traded,
  flat opening covered beyond its pinned `Normal` path: a test that the workspace API's
  `openings_held` serves `null` for a flat, traded, pinned agent whose counters read, and that a
  book reading `exits_only` or `paused`, or instrument restrictions, reach the binding gate; and the
  workspace API spec's `account_traded` text (§ agent read) updated to name the new causes (a
  position, an order working, unread counters, another related account) in a spec-only change
  (ES-22). *Accepted when:* those tests exist and the spec names the causes.
