- **E8-3 (from DEC-2859): the runtime binds an ask's approver count from `two_approver_above_usd`.**
  `mandate-runtime`'s `bound_action` binds `approvers_required` 1 to every ask it requests, so a
  request whose `order_usd` exceeds the mandate's `two_approver_above_usd` is asked of one approver,
  the agent's as well as the owner's a reviewer holds under mandate spec §6.2 step 5d (DEC-2859
  item 4). It adds no risk (every ask still waits for an approver and the binding gate), but a team
  gets one approver where §6.4 says two. *Accepted when:* the runtime binds `required` 2 above the
  threshold, from the version in effect, the approval content shows it, check 7 counts two distinct
  approvers before an act, and MC-D31's shape is pinned end to end in `mandate-runtime`'s tests.
<!-- section: E8 Escalation and approvals -->
