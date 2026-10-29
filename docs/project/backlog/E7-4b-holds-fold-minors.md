- **E7-4b holds fold minors (Proposed, M7; SC)** As an owner, I want `mandate agent holds` and the
  shared fold `mandate_journal::holds` (both from PR #2762) to read every input as the executor
  does and to print commands I can run as they stand, so that the list never disagrees with what
  the executor will accept. R3's four minors on #2762, deferred under the freeze rule:
  1. **Must** (the coordinator's ranking): `independence_required` reads only the workspace's
     latest membership record, while §9.28 check 4 takes the strictest overlay across every agent
     deployed on the account ([DEC-2830](../decisions/DEC-2830.md) item 2, DEC-2824 item 1). It
     fails safe (an unreadable record reads `true`), but it can print `no` where the executor
     requires independence. Until E9-6 lands, `mandate agent holds` prints that its independence
     line is advisory; folding the overlay across every agent deployed on the account is the later
     alternative.
  2. `requests()` reads a version-1 request that carries `subject` as that subject; journal spec
     §9.28 and [DEC-2838](../decisions/DEC-2838.md) read every version-1 request as `limit`. No
     writer emits that shape today.
  3. The differential property test in `mandate-executor`'s `acknowledge.rs` fixes `request` to
     `null` and draws only limit triggers and reconciliation holds, so it never exercises a
     restriction or the independence path.
  4. Without an agent, the command printed under an account-wide hold names the placeholder
     `AGENT_ID` (`ANY_AGENT` in `mandate-cli`'s `agent.rs`) and no line says to replace it with an
     agent deployed on the account.

  *Accepted when:* until E9-6 lands, `mandate agent holds` prints that its independence line is
  advisory (item 1's Must), and the later alternative, folding the overlay across every agent
  deployed on the account as the executor's check 4 does, replaces that line once it lands;
  `requests()` reads every version-1 request as `limit`, with a test of a version-1 request that
  carries `subject`; the differential test draws restrictions, requests and the independence
  overlay, and is shown to fail on a seeded bug in each; and an account-wide hold's command is
  followed by a line naming the agents deployed on the account, or saying to put one in place of
  `AGENT_ID`.
<!-- section: E7 Alpaca connector and recovery -->
