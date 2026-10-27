- **E7-fill-refs-outage (Proposed; trading core; ranked high by the coordinator; lane X)** As an
  owner, I want a fill of my agent's order booked, or its exits visibly safe, even while the
  executor cannot read the fill's configuration.
  - **The path:** an own-order fill arrives in an instrument whose deployment does not resolve now
    and has not resolved since the process started. That means a restart, so the deployment cache
    is empty, while the confirmed version or its registered instrument-snapshot artifact does not
    read (a store or artifact outage, or a deleted registration).
  - **What happens now:** `Deployment::fill_refs` answers none, so `FillApplied` names no
    `fee_config` or `instrument_snapshot`. The journal refuses it, DEC-2805 holds every opening,
    and the activity cursor does not pass the fill. The broker keeps reporting it, so the first
    pass after the configuration reads again books it.
  - **The rule-13 angle:** until then the position is unbooked. Exits and a kill switch's flatten
    size from the model's position and can under-sell the broker's real position.

  *Accepted when:*
  - during an unbooked own fill, an exit or a flatten either sizes from the broker's position
    (risk-reducing only) or is visibly held with an owner alert;
  - a refused own fill raises an alert whose subject names the fill until it is booked;
  - a test drives the outage and then the recovery: the fill is booked once, nothing is under-sold,
    and the alert ends.
