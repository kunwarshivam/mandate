- **E11-pf1 (Proposed; W)** As the founder, I want the realized P&L that `GET /agents/{id}/pnl`
  serves (DEC-2413, #3028) tested against an independent oracle that covers shorts and flips, so
  that a sign or basis error on a position that crosses zero cannot pass unseen. Today the property
  test checks realized against an integer sum over long-only fills. *Accepted when:* a property
  test in `mandate-gate-context` generates fill sequences that open short, add, reduce, and flip
  through zero, and its oracle (written apart from `Position::after_fill`, by the trading domain
  spec §8.1's rule) matches every per-instrument realized figure exactly; the test is shown to fail
  on a seeded sign error.
<!-- section: E11 Web app: dashboard and controls -->
