- **E7-4 (lane X, DEC-2849 item 8a's remainder): resize a holder's resting legs above its coverage**
  (`leg_excess`). Item 8a (#2897) gives each holder a leg of its own at re-placement, and 8b
  (#2906) cancels and re-places another holder's legs when they would pass that holder's coverage
  after an exit. A holder's legs that pass its coverage outside an exit (after a partial fill of
  its own exit, an owner's sale outside Mandate, or a release) are not yet cancelled and re-placed
  at the coverage left. They never sell more than the position (rule I-C holds), but they can
  over-cover one holder while another waits for room. *Accepted when:* the executor cancels each
  holder's legs above its Pᵢ and re-places them at Pᵢ less its own exits, tests first, with a hand
  test per trigger and #2927's acceptance fuzz extended to the resize, its seeded bug caught.
