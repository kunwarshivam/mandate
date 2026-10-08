# Task: E9-1 Sign in with passkey or OIDC SSO (lane L1, slices A1 and A2)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15).

## Story

- **Story:** E9-1 ([backlog](../06-backlog-v1.md)), claim [#758](https://github.com/kunwarshivam/mandate/issues/758)
- **Acceptance criteria (verbatim):** passkey sign-in requires user verification and OIDC sign-in
  checks signature, issuer, audience, expiry, nonce, and `email_verified` against the configured
  issuer only (spec §6.1), each refusal tested against an in-memory issuer and a software
  authenticator; sessions meet spec §6.2 (5-minute access tokens, uncached membership re-check,
  refresh rotation with reuse revoking the family, idle and absolute limits an org can only
  shorten); the risk-reduction path of §6.4 pauses and engages a kill switch with the identity
  provider unreachable (ID-10), while a refusal from the provider (`invalid_grant`, a disabled
  subject, a back-channel logout) ends the session with every permission and blocks the local
  passkey route (§6.4, §11.1); the host CLI commits only as its registered principal
  (`HostCliRegistered`, ID-1); and the log scan finds no canary token from any path (ID-9).
- **PRD / HLD / spec anchors:** PRD FR-1.1; HLD §8; [identity spec](../../specs/identity.md) §6.1,
  §6.2, §6.4, §11.1, ID-9, ID-10
- **Decisions that apply:** DEC-211, DEC-437, DEC-820 (the founder's acceptance of DEC-437 item 15:
  Supabase Auth is the managed issuer, ES256 only), DEC-650, DEC-651 (the ES-13 exception for A1)

## Scope

- **Slices in this brief:**
  - **A1**, OIDC token verification as a pure function (DEC-650): tests in two PRs (A1a: the
    crate, its stub API, the in-memory issuer, and the algorithm, key, signature, and issuer
    refusals; A1b: the claims, the token's shape, the key set, the two properties, and the ID-9
    canary), then the implementation.
  - **A2**, sessions (§6.2, §6.4 routes 1 and 2): tests, then the implementation.
  - The Postgres stores and the HTTP middleware (H1) wait for lane L2's server crate; passkey
    assertions and step-up are E9-4's.
- **Reference cases:** none; the identity spec has no reference-case file.
- **Invariants touched:** ID-9 (A1, A2), ID-5 and ID-10 (A2's reduction-only session).
- **Crates in scope:** `mandate-authn` (new: layer 1, pure, safety-critical).
- **Crates out of scope:** every other lane's crates (executor, mandate-robinhood, mandate-mcp,
  mandate-rh-sim, runner, mandate-cli, `web/`).
- **New dependencies allowed:** `ring` and `base64`, both already in `Cargo.lock` (DEC-650 item 8).
- **Safety-critical:** yes. DEC-77's sequence: tests PRs, then the implementation PR.
- **Size budget:** about 400 non-generated lines per PR.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-authn --run-ignored all
cargo xtask ci pending
cargo xtask ci mutants
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

Anything that loosens a rule, or that DEC-79 reserves for the founder, goes to the lead as
Proposed, and the work continues on the most conservative option.

## Definition of done

- [ ] Tests came first; each refusal has a named test, the properties' oracles are independent
      of the code, and each oracle was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (A2 names the session events of identity spec §12.1;
      the crate is pure and journals nothing itself).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green, with zero missed mutants.
- [ ] The PR description is complete (see the PR template).
