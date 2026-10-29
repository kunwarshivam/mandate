- **E9-4 (debt, Proposed, M8; SC) The credential fold owes a waiver for any member's assertion
  inside the cool-off (R1's F1 on #2920).** Journal spec v0.113 §9.38's converse names no member: a
  `StepUpAsserted` whose credential's enrolment `cool_off_ends` is after its `event_time` is owed
  its waiver whoever made it. `mandate_passkey::credentials::CredentialFold` owes one only for the
  credential's own member's assertion, so it reports no rule-318 break for another member's
  assertion inside the cool-off. It does not fail open, since rule 254 already breaks both members
  at that `seq`. *Accepted when:* the fold drops the member condition (DEC-176, a tightening), and
  `another_members_assertion_inside_the_cool_off_is_owed_a_waiver_too` and the oracle test without
  it run live.
<!-- section: E9 Identity, tenancy, and policy -->
