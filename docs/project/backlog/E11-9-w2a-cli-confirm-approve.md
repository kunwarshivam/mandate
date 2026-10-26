- **E11-9 W-2a (Must, M9, before the founder's demo; SC)** As the owner, I want to approve a paper
  request in the web app with a code I confirm on the host, so that an approval reaches the gate
  without a passkey until E9-4's passkey step-up lands ([DEC-2375](../decisions/DEC-2375.md),
  [DEC-2104](../decisions/DEC-2104.md)). *Accepted when:* Approve asks a challenge bound to the
  request's content hash and shows its code and the `mandate step-up confirm` line; the grant carries
  that challenge's `cli_confirm` evidence with `issued_at` as served; a challenge for any other
  action sends nothing; and Skip never carries step-up. Landed in #2435.
<!-- section: E11 Web app: dashboard and controls -->
