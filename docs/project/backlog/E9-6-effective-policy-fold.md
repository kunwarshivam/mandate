- **E9-6 effective-policy fold (Proposed, M8; SC)** As an owner, I want every reader of
  `independent_approval_required` to take it from one fold of the workspace's effective policy, so
  that an acknowledgment, an approval and a deployment are judged against the same requirement. No
  such fold exists: the executor process (E7-4b, `mandate-executord`'s `acknowledgments`) takes
  the latest `MemberActivated`, `MemberRoleChanged` or `MemberReactivated` record's
  `independent_approval_required`, `false` with none, and hands no acknowledgment while that record
  does not read; other readers take `false`. *Accepted when:* one fold reads `PolicyChanged` at
  every level with the membership records, as mandate spec §4.3 resolves them, fails closed on a
  record that does not read, and the executor process and every other reader take it from there.
<!-- section: E9 Identity, tenancy, and policy -->
