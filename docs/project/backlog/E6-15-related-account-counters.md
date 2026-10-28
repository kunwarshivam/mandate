- **E6-15-related-account-counters (Proposed; debt; lane X)** As an owner with more than one
  connected account in a workspace, I want the binding gate's §9.6 counters (opposite-side fills,
  daily participation, self-trade prevention) read across every account of the related-accounts
  group, so that an opening on a traded, flat account is answered there too. Today the gate answers
  it only while the workspace has exactly one live connection, and holds it otherwise
  ([DEC-2833](../decisions/DEC-2833.md) item 4). *Accepted when:* the counters fold every related
  account's stream, and a test shows an opposite-side fill on another account of the group denying
  an opening within its interval.
