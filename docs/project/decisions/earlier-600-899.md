# earlier-600-899

DEC-600 to DEC-899 — identity, the workspace API, notifications, the audit and verification
machinery, and the live-run (R0) rehearsals. The decisions are verbatim below; the rules still
in force:

- Identity: `mandate-authn` verifies OIDC tokens as a pure function under a fixed allow-list and
  strict parsing (DEC-650); one OIDC issuer verified by asymmetric keys (DEC-820, applying
  DEC-437 item 15); sessions and step-up are pure records; a deprovision or failed refresh ends
  the session before a reused `next` is refused (DEC-652, DEC-658); a sensitive data API can
  demand the permission its context was authorized for (DEC-655).
- The workspace API is served by axum, its OpenAPI document generated from the code; strict wire
  types via serde plus `validate()`, no free-form fields; a spec may list a value before the code
  serves it, marked planned, under checks that bound it (DEC-680, DEC-689, DEC-683).
- The MCP client's contract hash, fund-movement test and drift halt: a later check that finds an
  allowlisted tool missing suspends the connection (DEC-674, DEC-839, DEC-843).
- Audit and verification: exports and downloads are journaled before any byte is served
  (DEC-766); verification takes its trusted start from a manifest or anchor in the journal,
  never from the request (DEC-767); a trusted start from an anchor verifies that anchor's TSA
  token (DEC-896).
- Notifications: the notice id is minted through an injected secure-random source (DEC-702);
  web-push endpoints only on the browsers' own push services (DEC-792); a relayed VAPID `sub` is
  a role or platform address, never a person's (DEC-727).
- Live run (R0): `--confirm-live` and core dumps off (DEC-937 sits in the next file); here, the
  connector's reads reach only the one agentic account and fail closed otherwise (DEC-875), an
  unreadable Robinhood cancel answer is `Unreadable` and places no opening (DEC-864), and the
  runner's rehearsal build reaches loopback only (DEC-922).
- The landing page is the app itself, live in the browser's tab (DEC-906); the web app reads and
  commands through the workspace API behind one server-side data-source switch (DEC-975).


<a id="dec-610"></a>

## DEC-610: CI skips draft pull requests, and only a run that executed counts toward a merge

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The CI builder (agent), under DEC-79: a reversible CI scheduling decision that weakens no gate; CI and the merge script are founder-owned in `CODEOWNERS`, so the founder may overturn it |
| **Story or claim** | CI throughput correction from the coordinator; amends ADR-0001 ES-12; on [DEC-175](../04-decision-log.md#decisions), [DEC-464](earlier-303-599.md#dec-464), [DEC-538](earlier-303-599.md#dec-538) |

### Decision

1. **`ci` runs nothing on a draft.** `.github/workflows/ci.yml` triggers on `pull_request` with the
   types `opened`, `synchronize`, `reopened` and `ready_for_review`. `fast`, `full-checks` and
   `mutants-plan` carry `if: ${{ !github.event.pull_request.draft }}`, the mutation matrix follows
   its plan, and `full` runs on `always() && !draft`. On a draft every job is skipped, so a draft
   push starts no runner. `workflow_dispatch` has no pull request and always runs.
2. **Marking a pull request ready starts the run that judges it**, and so does every later push to
   it. `concurrency` keeps one run per ref with `cancel-in-progress: true`, unchanged.
3. **A skipped run is never green.** The required checks `fast` and `full` read as skipped on a
   draft, and branch protection counts a skipped required check as passing. So
   `merge-approved.sh` no longer trusts the run's conclusion alone. It reads the jobs of the
   latest `ci` run for the head and merges only when `fast` and `full` both ran and concluded
   `success`. A run skipped on a draft, one with either job skipped, or one without the jobs is
   refused, naming the run. Without this, an approved pull request read in the moment after it
   left draft and before its new run existed would have merged on the skipped run.
4. **Builders prove changes locally first.** A builder runs `cargo xtask check`, keeps the pull
   request a draft, and marks it ready once its independent review passes, or earlier when it needs
   CI's runners (`docs/project/07-quality-and-release.md`, development workflow item 6).
5. **Nothing else changes.** The checks, their ten-minute limit (DEC-464), DEC-538's planned
   matrix, the required check names, and every other condition `merge-approved.sh` reads are as
   they were.

### Rationale

About 60 open pull requests had `ci` queued or pending, most of them drafts. Each draft push
started a full run, and most were superseded before they finished, so approved pull requests
waited behind them. Builders already run the whole set locally, and a draft is not yet a candidate
for merge, so its CI run bought nothing the review needs.

#### What holds it

- `the_merge_script_refuses_anything_short_of_an_approved_green_head` adds three refusals: a run
  whose every job was skipped on a draft, one with `full` skipped, and one without the required
  jobs. The `gh` stand-in answers the jobs query only for the latest `ci` run. Seeded bug: with the
  jobs comparison disabled, the script squash-merged the draft-skipped run, and the test failed.
- `ci_runs_nothing_on_a_draft` pins the trigger types, the per-ref cancellation, and each job's
  draft guard. Seeded bug: `fast` without its guard.

### Alternatives considered

- **Trigger on `ready_for_review` only, never on draft pushes.** The required checks would then
  report nothing on a draft, and a later push to a ready pull request needs `synchronize` anyway.
  Skipping by job keeps one trigger set and reports the required checks as skipped.
- **Require `pull_request.draft == false` on the run.** The runs API does not carry the pull
  request's draft state at trigger time; the jobs' own conclusions are what the API does carry.

<a id="dec-644"></a>

## DEC-644: An ES-13 exception for the mandate-identity stub API PR (I1 tests, part 1)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The lane L1 lead's ruling, under DEC-79 and DEC-537: an engineering size limit, not a safety rule. A reversible process decision that an agent may accept; the founder may reverse it |
| **Story or claim** | E9-2 (claim #757); I1 tests, part 1 (the PR that adds `crates/mandate-identity/`); precedent DEC-537 |

### Decision

1. **The exception.** The first tests PR of `mandate-identity` may exceed ES-13's 400-line split
   threshold (DEC-253 item 3). Counted over its diff against `main`:
   - 1828 lines in `crates/mandate-identity/`: 770 in `src/lib.rs` and `src/permission.rs`
     (717 and 53), 1043 in its in-crate tests (`src/tests/matrix.rs` 711, `rows.rs` 139,
     `mod.rs` 106, `wire.rs` 87), and 15 in its `Cargo.toml`;
   - about 1870 lines in all, with the wiring outside the crate (the workspace and lock files,
     `xtask/layers.toml`, CODEOWNERS, `docs/dependencies.md`, the feature map).
2. **This PR only.** Part 2 (the ID-13 fuzz and the last-owner and last-admin tests, stacked on
   part 1) and the implementation PR stay within 400 lines, or each records its own
   justification. DEC-77 holds: the implementation PR changes the tests only by deleting ignore
   lines, and each PR passes the mutation gate with 0 missed mutants. The lane lead's review
   ruling (#806, round 1) declined a split: no blocker rests on the size, and a split now would
   cost more review than it saves.
3. **The identity-side step-up evidence is `mandate-identity`'s** (#806 review, major 2).
   `AssertionId` wraps the journal's text form (`assertion_id: text`, journal §9.2), the shape of
   `mandate-approval`'s `AssertionId`. Both crates sit at layer 1, so neither depends on the
   other: `mandate-identity`'s `StepUpEvidence` serves step-up issuance and verification (E9-4),
   `mandate-approval`'s serves the runtime, the API converts between them, and E9-4 owes a test
   that pins the two shapes equal.

### Rationale

- **The size is the closed interface.** Most of `src/` is the public API other lanes code
  against (L2's `mandate-api` and `Authenticate`, L4's web, L5, and `mandate-authn` and
  `mandate-passkey` at layer 2). That API is:
  - one documented `Permission` variant for each of the 46 rows of identity spec §4.2 as #766
    and #811 leave it (four of them owed by those PRs until they merge);
  - the closed `Role`, `PrincipalKind`, `MembershipState`, `ClientScope` and `StepUpActionKind`
    enums, with their wire codes;
  - `TenantContext`, the sealed `Tenant` and `MembershipLookup` traits, `MembershipQuery`,
    `Session` with its per-scope roles snapshot, `Membership` with each role's effective-from
    instant, and principal scope, which the #766, #806, and #811 reviews asked for, with their
    `compile_fail` proofs.

  The only logic is two `Unimplemented` stubs and the `code()` tables. The bodies are not built
  yet.
- **The test is one oracle.** Most of the tests are the exhaustive ID-2 test. Its parser and
  grammar check only make sense next to the 46-row map they read. Splitting them would leave a
  parser with nothing to check.
- **Splitting the API would cost more than it saves.** Half an enum or half a matrix map cannot
  be reviewed on its own. Several lanes are waiting on this one interface, and each extra
  stacked PR delays them by a review cycle.

### Alternatives considered

- **Split part 1 to 400 lines.** Rejected. It would cut the closed `Permission` enum or the ID-2
  map in the middle, away from the test that reads them.
- **Leave the overage unrecorded.** Rejected. ES-13 is enforced by review, so an exception the
  review cannot see is not an exception.

<a id="dec-647"></a>

## DEC-647: The membership writer refuses a full stream (`StreamFull`) and never retries a store failure

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L1 agent (E9-7), under DEC-79 and DEC-176: each item only narrows what may commit a membership record and adds no risk |
| **Story or claim** | E9-7, claim [#760](https://github.com/kunwarshivam/mandate/issues/760); amends [DEC-646](DEC-646.md) item 3; PR #1108 |

### Decision

DEC-646 item 3 lists the writer's refusals. This decision adds two, and DEC-646 item 3 reads with
them:

1. **`WriteError::StreamFull`** when the stream's head is already `u64::MAX`, so no `seq` follows
   it ([DEC-659](DEC-659.md) item 3: no `seq` is above `u64::MAX`). The writer computes the next
   `seq` with checked arithmetic, answers `StreamFull` before it reads the clock, runs the order
   check, or appends, and writes nothing. It never wraps or saturates the `seq`.
2. **`WriteError::Store` is not retried.** A failed read or append is passed through at once; only
   a moved head (`HeadMoved`) re-runs the attempt (DEC-646 item 2).

As with the other refusals, the caller retries later and nothing is granted meanwhile.

### Rationale

No existing variant names a full stream: `Refused(OutOfOrder)` describes a record checked against
a last membership record, which a full stream need not have, and `Contended` names a moved head. A
wrapped `seq` would duplicate a committed one, and a saturated one would reuse it, so refusing is
the only reading that keeps every `seq` unique. Retrying a store failure could only add commit
attempts against a store already failing; passing it through is the safe default (`AGENTS.md`
rule 3).

### Alternatives considered

- **Map a full stream to `Store` or `Contended`.** Rejected: `Store` needs the store's own error,
  and `Contended` would invite a retry that can never succeed.
- **Amend DEC-646 in place.** Rejected: the decisions README lets a PR change an accepted decision
  only through a new one.

<a id="dec-650"></a>

## DEC-650: `mandate-authn` verifies OIDC tokens as a pure function, under a fixed allow-list and strict parsing

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The lane L1 builder (Claude Code), under DEC-79 and DEC-176: every item tightens a check the identity spec names or closes a gap by the reading that adds no risk. The per-issuer algorithm list in item 2 is the lane lead's ruling on the #791 review (a tightening) |
| **Story or claim** | E9-1 slice A1; claim [#758](https://github.com/kunwarshivam/mandate/issues/758); [task brief](../tasks/E9-1-authn.md) |

### Decision

1. **The crate.** `mandate-authn` is a layer-1, pure, safety-critical crate (superseded in part by
   DEC-652 item 9, which moves it to layer 2). It verifies a token
   and (slice A2) keeps sessions as pure functions: the caller fetches the issuer's key set and
   reads the clock, and hands both in. It never fetches a key, reads a clock, or draws randomness,
   so the same inputs always give the same answer.
2. **What a token must be.** Of the identity spec §6.1 checks:
   - `alg` is one of the algorithms configured for this issuer, drawn from ES256, RS256, and
     EdDSA and compared case-sensitively. The managed Supabase issuer (DEC-820, the founder's
     acceptance of DEC-437 item 15) is configured for ES256 only; another SSO issuer for whichever
     of the three it signs with. `none`, every HMAC algorithm, and anything else are refused
     before a key is looked up, whatever the `kid`, so a public key is never an HMAC secret.
   - A header that carries a key or a pointer to one (`jwk`, `jku`, `x5u`, `x5c`, `x5t`,
     `x5t#S256`) is refused: only the key set the caller supplied is ever used.
   - `kid` is required, even when the key set holds one key, and must name a usable key of the
     algorithm's type. A key that names its own `alg` must name the header's.
   - A header with `crit` is refused: this verifier understands no extension (RFC 7515 §4.1.11).
   - An ES256 signature is the raw 64-byte `r || s` (RFC 7518 §3.4). A DER signature, or raw bytes
     of any other length, is a bad signature. So is an empty signature segment: it decodes to no
     bytes, which never verify.
   - `iss` equals the configured issuer byte for byte: no trailing-slash or case folding, so
     another tenant of the same provider, signing with the same keys, is refused.
   - `aud` names a configured audience. When it names several, `azp` must be present and be a
     configured audience (OIDC Core §3.1.3.7 items 4 and 5).
   - `sub` is present and non-empty; `exp` is present; `nbf` is checked when present.
   - An ID token's `nonce` is present, non-empty, and equal to the one the sign-in issued. An
     access token is not checked for a nonce. The comparison is not constant-time: it runs only
     after the signature verifies, so an attacker cannot choose what is compared, and ring's
     helper for it is deprecated in 0.17.14.
3. **Parsing is strict.** A token over 16 KiB (`MAX_TOKEN_BYTES`) is refused before decoding.
   Each segment is base64url without padding and with zero trailing bits, so one token has
   exactly one text. The header and the claims must be JSON objects; a duplicated member, or a
   claim of another JSON type (a fractional `exp`, a numeric `iss` or `nonce`), is refused. No
   claim is read before the signature verifies: a token with a bad signature is refused as such
   whatever its claims say.
4. **Time.** The skew is a constant 60 s (`CLOCK_SKEW_S`) on each side, not a setting, so no
   configuration can widen it. A token is expired from `exp + 60 s` and not yet valid before
   `nbf − 60 s`. Both are computed with checked arithmetic. An `exp` that `UtcNanos` cannot hold
   (negative, past 9999-12-31, or `i64::MAX`), an `exp` or `nbf` the skew would overflow, and an
   integer beyond `i64` are a malformed payload, not an expiry. `UtcNanos` starts at the Unix
   epoch, so no negative `exp` is representable; an `exp` of 0 is representable and expired (the
   lane lead's ruling on the #810 review). The same holds for `nbf`: one before the epoch or past
   9999-12-31 is a malformed payload, and an `nbf` of 0 verifies. The skew is applied to `now`,
   never to `exp` or `nbf`, so a value inside the range never overflows: an `exp` at the last
   representable second (253402300799) verifies, and `expires_at` is that second. An
   unrepresentable `nbf` is refused as malformed before `Expired` is checked.
5. **The key set.** A JWKS key is kept only when it has a non-empty `kid`, its `use` (if any) is `sig`,
   and it is EC on P-256, RSA with a modulus of 2048 to 8192 bits (the range ring verifies), or
   OKP on Ed25519. Every other key (P-384, X25519, an RSA key outside 2048 to 8192 bits, a
   symmetric `oct` key, a key without a `kid`) is left out and can never verify a token, and a
   key left out never collides with a kept key's `kid`. A kept key with a missing or wrongly
   sized member fails the whole set, and so do two kept keys with one `kid` and a member the
   document or a key repeats: an ambiguous set verifies nothing.
6. **Invitations.** Matching an invitation by address (§6.1) needs `email_verified` to be the JSON
   value `true` (a string `"true"` is not), and the address to equal the invited one byte for
   byte. Case-folding the domain would admit more; refusing a differently cased address only asks
   the admin to re-invite. Supabase Auth's access tokens carry no top-level `email_verified`, so
   they never match an invitation by address for now. When the identity spec's reading of DEC-820
   lands (builder B1, DEC-640 block), the claims Supabase actually issues are mapped to it:
   `user_metadata.email_verified`, or `email_confirmed_at` read through the session.
7. **Nothing printed.** A refusal carries static text only. A `TokenKind`'s `Debug` never shows
   the nonce, a key set's shows its key ids only, and a verified subject's shows the issuer and
   the expiry, never the subject or the address, which belong in the personal-data
   vault (identity spec §3.1). The tests check this with canary values (ID-9).
8. **Dependencies.** `ring` 0.17.14 verifies the signatures, and `base64` 0.22.1, the
   workspace's one pin, shared with `mandate-passkey` (DEC-660), decodes the segments; both were
   already in `Cargo.lock` (through `rustls` and `sqlx`). `serde` and `serde_json`, already
   registered, parse the header and claims. The tests' RS256 key is one
   fixed PKCS#1 file made with `openssl genpkey`, because ring cannot generate RSA keys; it signs
   nothing outside the tests, and `.gitleaks.toml` allowlists no path for it. Every other test key
   is made in the test process (identity spec §1.3).

### Rationale

Each item refuses something the spec's list would otherwise leave open: algorithm confusion, key
ambiguity, a same-provider tenant, an ID token issued to another client, a replayed sign-in, a
malleable token text, and a configurable skew. None of them can block risk reduction: a refused
sign-in leaves the routes of identity spec §6.4, which do not depend on the identity provider.

### Alternatives considered

- **Fetch the JWKS in the crate.** Rejected: it would put the network in a pure core and make
  verification depend on a reachable provider.
- **`jsonwebtoken` or another JWT crate.** Rejected: a larger surface, with HMAC support that has
  to be configured away rather than being absent.
- **`aws-lc-rs`.** Rejected for the same reason DEC-88 gave for TLS: C code with the OpenSSL
  license in its set.
- **Accept a token with no `kid` when the set holds one key.** Rejected: a rotation that adds a
  second key would change which tokens verify.
- **Case-insensitive address matching.** Rejected for item 6's reason.

<a id="dec-652"></a>

## DEC-652: Sessions in `mandate-authn`: pure records, the outage and refusal split, and the reduction-only session

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The lane L1 builder (Claude Code), under DEC-79 and DEC-176. Each item tightens or closes a gap by the reading that adds no risk. Item 3 follows the lane lead's ruling on the 408 and 429 question |
| **Story or claim** | E9-1 slice A2; claim [#758](https://github.com/kunwarshivam/mandate/issues/758); [task brief](../tasks/E9-1-authn.md) |

### Decision

1. **A session is a pure record.** `mandate-authn::SessionRecord` takes the time and the
   caller's fresh refresh secrets as inputs, and returns what the caller journals
   (`SessionRevoked.reason` codes from identity spec §12.1). It keeps only the SHA-256 digest of
   each refresh token, never the token itself (ID-9), and exposes them (`refresh_digest`,
   `rotated_digests`) so a test can check that against its own SHA-256. The membership
   re-check, the stores, and §6.2's cap on concurrent sessions are the caller's (slice H1).
2. **Limits.** Access tokens last 300 s for every org. The idle and absolute limits default to
   24 h and 7 days for an individual org, and 1 h and 12 h for a business org. An org's setting
   must be positive and no longer than the default; an equal setting is accepted, and a longer
   one is refused, not clamped, so the org learns it was refused. After shortening, an idle
   timeout longer than the absolute lifetime is refused too.
3. **Outage, failed refresh, or deprovision** (§6.4 route 1, §11.1), on the lane lead's final
   ruling of 2026-10-08, which superseded an earlier one:
   - **Outage.** No answer (a timeout, or a connection, TLS, or name-resolution failure) or a 5xx
     status leaves the session pause and the kill switch (item 4).
   - **Deprovision.** A deprovision signal (`invalid_grant`, a disabled or revoked subject, or a
     back-channel logout) ends the session with every permission (`deprovisioned`). It also
     closes route 2 for the subject until its next successful sign-in (item 7).
   - **Failed refresh.** Every other answer that grants nothing (any other 4xx, a 408 and a 429
     included, a 3xx, or a 2xx without new tokens) also ends the session with every permission,
     as the spec's "only a transport-level failure or a 5xx keeps anything" says. It is not a
     deprovision signal, so route 2 stays open: the member can still pause or engage a kill
     switch with a local passkey.
   - **Journaling.** A failed refresh is journaled as `SessionRevoked` with the reason
     `refresh_failed`, one of the seven reasons §12.1 lists (`sign_out`, `deactivated`,
     `deprovisioned`, `refresh_reuse`, `refresh_failed`, `expired`, `admin`). The event's schema
     comes with E9-7.
   - **Why a tightening.** An earlier draft of this item treated every non-5xx answer as a
     deprovision. That also closed route 2 on a rate limit, which went past §11.1's list. This
     reading ends the same sessions, and closes route 2 only on the signals §11.1 names.
4. **In an outage**, the session reaches pause and the kill switch until its absolute lifetime,
   with no idle timeout, since nothing else is reachable through it (§6.4 route 1). Its pauses
   and kill switches count as activity. A later granted refresh with the same refresh token,
   which an outage does not consume, restores the full session only while the idle timer has not
   lapsed since the last admitted request; otherwise the grant ends the session, journaled as
   `SessionRevoked` with the reason `expired` (item 5). It does not leave the session reduced.
   That is DEC-816 item 8, §6.4 route 1, and the lane lead's ruling on the A2 review, the reading
   that only tightens (DEC-176). No step-up is presented through an outage session.
5. **Activity and the clock.** A granted refresh and an admitted request reset the idle timer,
   which never runs past the absolute lifetime, so the end of the clock's range never refuses a
   pause or a kill switch (rule 13). A refusal is no activity.
   - **Expiry ends the session.** A session ended by its idle or absolute limit is journaled as
     `SessionRevoked` with the reason `expired` (DEC-816 item 8, §12.1). The first request or
     refresh at or after a full session's idle timeout, or any session's absolute lifetime
     (route 2's 900 s included), ends it, and so does a granted refresh to an outage session whose
     idle timeout lapsed (item 4). Every later call is refused `Ended` with that reason, and the
     caller journals it once, when `ended` first turns `Some`. An expired access token is not an
     end: a refresh renews it.
   - **A clock gone backwards** is a `now` earlier than the opening or the last activity. A
     refresh, or a request other than pause and the kill switch, is refused `ClockBehind`
     whatever the session's reach (an ended session still answers `Ended`), and ends nothing. A
     pause or a kill switch is never refused for it (ID-10, rule 13): it is judged at the last
     activity instead of `now`. Every other rule (an ended session, the reach, the
     absolute lifetime, the access token) applies at that instant, and the admission moves no
     timer. Every activity came before the absolute lifetime, with a live access token or in an
     outage, so a live session admits them there and an ended one still refuses them. An earlier
     draft refused them `ClockBehind`; removing that refusal only admits risk reduction, which
     rule 13 requires, and widens nothing else, so the builder takes it under DEC-176.
6. **Refresh rotation.** A family is one session's chain of refresh tokens. A rotated token
   presented again ends the session with `refresh_reuse`. A token that is neither current nor
   rotated is refused and revokes nothing, so a guess cannot end someone's session. A `next`
   secret equal to the presented one, or to one already rotated away, is refused
   (`RefreshSecretReused`) and revokes nothing.
7. **The reduction-only session** (§6.4 route 2) lasts 900 s from its opening, has no refresh
   token and no idle timeout, reaches pause and the kill switch only, and never presents step-up.
   It is refused when the last deprovision signal for the subject is at or after its last
   successful sign-in: a tie counts as "since". A signal with no sign-in on record refuses it
   too. The caller records every deprovision signal with `SubjectStanding::saw_deprovision`,
   whatever state any session of the subject is in (signed out, lapsed, revoked) or with none
   open, so a departed member disabled at the IdP never keeps route 2 (§13). The latest signal
   is kept. A failed refresh records nothing. The caller (H1) opens the session only after
   verifying a fresh passkey assertion locally, and builds its roles snapshot from every
   workspace where that credential has an unsuspended row. The typed proof of the verification
   comes with E9-4.
8. **In a full session**, pause and the kill switch need a live access token like every other
   request. When the refresh that would renew it finds the provider unreachable, item 4 keeps
   them.
9. **The identity session.** `mandate-authn` moves to layer 2 and depends on `mandate-identity`
   and `mandate-identity-seal`, whose `allowed_dependents` list names it (DEC-642 item 7, agreed
   with the identity builder). `SessionRecord::admit` runs the record's checks for the request at
   `now` and only then builds `mandate_identity::Session` with the reference the caller's store
   keeps for the record, the record's kind, and the roles snapshot. H1 builds the snapshot (item
   7 for route 2); `admit` passes it through whole and in order, cooling roles included, for
   `authorize` to judge at `now`. A full reach is `SessionKind::Full`; an outage (route 1) and a
   reduction-only session (route 2) are `SessionKind::ReductionOnly`. A refusal builds nothing.
   The pure record is named `SessionRecord`, so it is not confused with the identity session.
   This move supersedes the layer-1 statements of DEC-650 item 1 and DEC-653 ("`mandate-authn`
   1"); `xtask/layers.toml` is the source.

### Rationale

Each reading keeps the spec's two promises: a stolen or deprovisioned session loses everything
at once, and an unreachable provider never takes away pause or the kill switch. Keeping route 2
open after a failed refresh that is not a deprovision signal keeps the local passkey path to
pause and the kill switch (ID-10, rule 13) without widening any session.

### Alternatives considered

- **Clamp an org's longer setting to the default.** Rejected: the org would believe a limit is
  in force that is not.
- **Revoke on an unknown refresh token.** Rejected: anyone could end a session by sending
  garbage.
- **Count a 408 or a 429 as an outage**, keeping pause and the kill switch in the session.
  Rejected on the lead's final ruling: it loosens the spec's text, and route 2 already covers
  that case.
- **Count every non-5xx answer as a deprovision.** Rejected: it closes route 2 on signals that
  §11.1 does not name.
- **Refuse pause and the kill switch on a backwards clock.** Rejected: rule 13 lets nothing but
  agent mode, an `Unknown` order, or the broker hold risk reduction, and a clock fault is none.
- **Leave a lapsed outage session reduced, journaling nothing.** Rejected: DEC-816 item 8 ends it
  as `expired`, so the record says why it ended.
- **Keep the idle timeout in an outage.** Rejected: §6.4 route 1 keeps pause and the kill switch
  "until its absolute lifetime ends".

<a id="dec-653"></a>

## DEC-653: E9-1 slice A3 builds route 2's reduction-session gate as a pure module of mandate-authn

| | |
|---|---|
| **Status** | Accepted. Item 3's failure-limit reading is the founder's, accepted 2026-10-09 |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The lane L1 lead (claude-code), under DEC-79's delegation: engineering sequencing that only implements the founder's DEC-833 and DEC-834 and adds no risk. The founder (DEC-79), 2026-10-09, accepted item 3's reading that the failure limit only reports |
| **Story or claim** | E9-1 (claim #758); [identity spec](../../specs/identity.md) §6.4 route 2, ID-10 |

### Decision

1. **Slice A3** builds the gate in front of `POST /v1/reduction-sessions/challenges` and
   `POST /v1/reduction-sessions` as `mandate-authn`'s `reduction` module: `ReductionGate`,
   `ClientKey`, `ReductionLimits`, `Challenge`, and the one refusal `Unauthenticated`. It is pure
   (DEC-650): the caller hands in the time, the 32 random challenge bytes, and the passkey verifier
   as a closure, so the layer-1 crate needs neither a clock, randomness, nor `mandate-passkey`
   (layer 2).
   (Superseded in part by DEC-652 item 9: `mandate-authn` is now layer 2; it still does not
   depend on `mandate-passkey`.)
2. **Left to others.** H1's middleware owns the 401's status, body, headers and timing; A2's
   reduction-only session is what a verified assertion opens; the verifier closure looks up the
   credential by the client-supplied ID (DEC-833).
3. **Readings taken where the spec is silent**, each the one that refuses no valid key:
   - A challenge may be presented from another address or device than the one that asked for it
     (a phone changing networks), and is consumed either way; its slot is the issuer's and frees
     when it is consumed or expires; a failure counts against the presenter.
   - Every refused presentation is a failed assertion, including one over a challenge that was
     never issued, used, or expired; a failure counts for `CHALLENGE_LIFETIME_S`, 300 s, after it.
   - **Accepted by the founder, 2026-10-09:** no answer depends on the failure limit. Every
     failure is already the one 401, and a verified assertion is never refused (DEC-834), so
     `failure_limit_reached` only reports, for H1 to record the abuse signal. It never refuses
     new challenges. The challenge limit and first-use consumption bound the verification work.
   - A challenge is outstanding while `now < issued + 300 s`; at exactly 300 s it is expired.
   - Issuing bytes equal to an outstanding challenge's is refused.
   - The challenge route's refusals (over its limit, or bytes equal to an outstanding
     challenge's) are the same `Unauthenticated` as the assertion route's, so H1 answers both
     routes with one 401 and no refusal tells a limit from anything else.

### Rationale

Single use and counting are a pure state machine's rules, testable exhaustively; a verifier
closure keeps the layer order (`mandate-authn` 1, `mandate-passkey` 2) and lets tests count calls.

### Alternatives considered

- **Bind a challenge to the client that asked for it.** Tighter, but refuses a valid key whose
  address changed between the two requests, against DEC-834's "never refused".
- **Refuse new challenges while the failure limit is reached.** Gives the failure limit teeth, but
  lets a flood of cheap failures lock the member out, widening the residual DEC-834 accepted.

<a id="dec-655"></a>

## DEC-655: A sensitive data API can demand the permission its context was authorized for

| | |
|---|---|
| **Status** | Accepted (the lane L1 lead, under DEC-79's delegation, 2026-10-09). An engineering reading that tightens ID-8 (DEC-176); it adds to `mandate-identity`'s public API, which lane L5 depends on, and the merge coordinator agreed it on 2026-10-09 |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The lane L1 lead (claude-code), at the merge coordinator's request, under DEC-79's delegation |
| **Story or claim** | E9-8 (claim #761), E9-2 (claim #757); [identity spec](../../specs/identity.md) ID-8, §4.5; L5's E12-6 audit reads |

### Context

`authorize` builds a `TenantContext` for exactly one `Permission`, and `TenantContext::permission()`
returns it. Data APIs take `&impl Tenant`, and the sealed `Tenant` trait exposes the workspace, the
organization, the principal and its kind, but no permission. So a read that only `ReadRecords`
should reach, such as L5's audit reads, accepts a context authorized for any permission. A
`ViewAgents` context passes as well as a `ReadRecords` one. ID-8 makes another workspace's data
unreachable. It does not yet make one workspace's data unreachable to a request authorized for a
different row of §4.2.

### Decision

1. **A witness type per demanded permission.** `mandate-identity` adds a sealed witness,
   `demand::Permitted<'a, P>`, where `P` is a zero-sized marker type for one `Permission`. The first
   marker is `ReadRecords`; another is added only when a data API needs it. `Permitted` borrows
   the context and implements `Tenant`. It has private fields, no `Clone`, no `Default` and no
   public constructor. It is named `Permitted`, not `Authorized`, because
   `mandate_identity::Authorized` already names what `authorize` returns.
2. **The only way to obtain one** is `TenantContext::require::<P>(&self) -> Result<Permitted<'_, P>,
   Refusal>`. It returns `Ok` exactly when the context's permission is `P`'s, and otherwise
   `Err(Refusal::Forbidden)`.
3. **A sensitive API demands it in its signature.** For example, `fn read_records(ctx:
   &Permitted<'_, ReadRecords>, ...)` instead of `ctx: &impl Tenant`. Passing a context authorized
   for another row is then a type error at the call site, or a `forbidden` refusal one step earlier
   at `require`. It never becomes a read.
4. **Nothing else changes.** `Tenant` keeps its methods, and every existing `&impl Tenant` API keeps
   working. `OrgContext` and `PrincipalContext` get no `require`. Lane E9-8's `SystemContext`
   produces a witness only for the permissions its registration lists, and the deployment's
   background processes list none of the sensitive ones until a story needs them.
5. **Tests owed** (in the PR that adds the witness):
   - `a_context_for_another_permission_cannot_read_records`, live: a `ViewAgents` context's
     `require::<ReadRecords>()` is refused `forbidden`;
   - `permitted_cannot_be_constructed_outside_require`, a `compile_fail` doctest;
   - `require_returns_the_context_it_was_called_on`: the witness's workspace, org, principal and
     kind equal the context's.

### Rationale

The check sits where the permission is already known, in `mandate-identity`. A data API cannot
forget it, because the type demands it. The marker types cover only what is demanded, so the
§4.2 matrix is not duplicated as types. A runtime check inside each read would work too, but a
read that forgot it would compile.

### Alternatives considered

- **`fn permits(&self, Permission) -> bool` on `Tenant`.** Smallest change, but every sensitive
  read must remember to call it. Rejected as the primary mechanism; it may be added as well.
- **`TenantContext<P>` generic over every permission.** Compile-time like the proposal, but every
  existing `&impl Tenant` API and `authorize` itself would change shape, and it needs a type per
  row of §4.2. Rejected: too wide for one demanded permission.
- **Leave it.** ID-8 still holds across workspaces, but a request authorized to view agents could
  read the records. Rejected.

<a id="dec-658"></a>

## DEC-658: A deprovision or failed refresh ends the session before a reused `next` is refused

| | |
|---|---|
| **Status** | Accepted (the lane L1 lead, under DEC-176, 2026-10-09; the merge coordinator agreed it on 2026-10-09) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The lane L1 lead (claude-code), under DEC-176: a reading that only tightens DEC-652 item 6 |
| **Story or claim** | E9-1 (claim #758); the follow-up from #832's review; identity spec §6.4 route 1, §11.1 |

### Context

DEC-652 item 6 says a `next` secret equal to the presented one, or to one already rotated away, is
refused (`RefreshSecretReused`) and revokes nothing. #832's `SessionRecord::refresh` checks that
before it reads the provider's answer. So a deprovision signal (§11.1) or a failed refresh (§6.4)
that arrives with such a `next` is refused `RefreshSecretReused`, and the session stays open until
a later refresh hears the same answer again. Nothing in the spec asks for that order.

### Decision

1. **An ending answer comes first.** `refresh`, after its existing checks of the session (ended,
   clock behind, lapse) and of the presented token (unknown, reused), reads the answer: a
   `Deprovision` ends the session `deprovisioned`, and a status that is not an outage ends it
   `refresh_failed`, whatever `next` is.
2. **Only then is `next` checked.** A grant or an outage with a `next` used before is still
   refused `RefreshSecretReused` and changes nothing, as DEC-652 item 6 says: a grant must not
   rotate to it, and an outage consumes nothing either way.
3. **Tests owed:** `a_deprovision_or_failed_refresh_ends_the_session_even_with_a_reused_next` in
   `crates/mandate-authn/tests/session.rs`, pending with a `xtask/behaviour-only` row until the
   implementation PR moves the check and deletes both.

### Rationale

Risk reduction is never denied, and ambiguity resolves to the safe default (AGENTS.md rules 3 and
13). The provider has said the subject may no longer act; a malformed or replayed `next` is the
caller's bookkeeping and must not keep that subject's session open. A reused `next` does not end
the family on its own later: the record stays as it was, so the session would end only when a
later refresh hears the provider again. Item 2 keeps every other outcome of DEC-652 item 6.

### Alternatives considered

- **Leave the order.** Spec-consistent, but a deprovisioned subject's session survives one
  refresh. Rejected: it loosens nothing to end it, and keeping it open adds risk.
- **End the session on any reused `next`.** Stricter, but a caller's bad randomness would sign a
  user out, and DEC-652 item 6 decided a reused `next` revokes nothing. Rejected.

<a id="dec-682"></a>

## DEC-682: How the workspace API's envelope and command schemas read spec §3 and §5

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176, on the coordinator's ruling of 2026-10-08 that JSON Schemas under `schemas/workspace-api/` are the API's source and on the conventions the L2 and L4 leads agreed. Each item tightens the spec or settles a gap by the reading that adds no risk |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); [workspace API spec](../../specs/workspace-api.md) §3.2, §3.3, §3.5, §5; DEC-436, DEC-681, DEC-740 |

### Decision

`schemas/workspace-api/envelope.schema.json` and `schemas/workspace-api/commands/*.schema.json`
are written as follows. `mandate-api`'s Rust types are checked equal to them in A1b.

1. **The envelope** (the lead's final shape).
   - `Envelope` is `{api_version: "v1", build: ContentRef, served_at: Timestamp, as_of: [Watermark]}`.
     `build` is §3.2's server build digest. `served_at` is when the response was built;
     `Freshness.age_seconds` is measured to it.
   - The envelope is open. Every response is `allOf: [Envelope]` with its own members and
     `unevaluatedProperties: false`.
   - `as_of` may be empty only for a response that reads no stream (OAuth start). Every response
     that reads or commits on a stream tightens it to `AsOf` (at least one watermark). A
     command's `202` carries the committed control-stream watermark.
2. **Requests are closed** (`additionalProperties: false`), as DEC-681 item 6 reads. A body naming
   `requested_by` is refused, and no request has that member (API-6).
3. **The schemas mark a nullable member as `oneOf: [{$ref}, {type: null}]`**, never `type` beside
   `$ref`. Closed safety enums (§3.2) are `enum` or `const`.
4. **`Problem` (§3.5)** pins each `code` to its table status. `event_id` is present exactly when
   `effect` is `recorded` or `unknown`.
   - `code` and `effect` are closed (DEC-681 item 1).
   - `title` is free text in the schema; the server fixes it per code (API-10).
5. **`Actor` (§3.3 item 4)** is `{kind: user, id}` or `{kind: client, id, on_behalf_of}`. A client
   is never a user.
6. **`StepUpEvidence`** is `{assertion_id: EventId, authenticated_at: Timestamp, method}`.
   - It mirrors journal spec §9.2 and `mandate-identity`'s type: the assertion id is the
     challenge's ULID (26 Crockford digits, first `0` to `7`), and `method` is closed at `passkey`
     and `cli_confirm` (L1's answers of 2026-10-08).
7. **Kill switch (§5.4)** requires only `scope`. A missing optional member never refuses it (API-7).
   - `scope.kind` is `agent`, `connection`, or `workspace`; there is no org value (DEC-436 item 13).
   - A workspace scope's `id` is `null`; an agent or connection scope's is an `Id`.
   - `owner_exit` bids are `{asset_id, bid, bid_size, quoted_at, floor}`. Every price is a
     `Decimal` string.
8. **Pause, hold, and ending a delegation** take only an optional `record`, so `{}` is valid.
9. **Resume, lift-hold, and acknowledge** require a `step_up` member, which may be `null`. Missing
   evidence is forwarded and refused by the runtime (§5.4), never by the schema.
10. **Stop** requires `release`. `warning_shown` is a `ContentRef` exactly when `release` is
    `true`, and `null` otherwise (DEC-290 item 2).
11. **Approval response (§5.2).** All five members are required.
    - `verdict` is closed: `approved` or `skipped`.
    - `content_hash` is required (API-12). `record` is a non-null `Record`, as §5.2 types it.
    - `step_up` may be `null`. A missing step-up on `approved` is the server's
      `step_up_required` (401), not a schema refusal, and `skipped` is never refused for it.
    - A `skipped` carries `delegation: null`; the schema refuses a Skip naming a delegation, and
      the server drops it (item 27), so a Skip is never refused for it (API-7). An `approved` with
      a delegation needs step-up bound to the preview's `step_up_digest` (§5.3), else the server's
      401.
12. **Owner exit (§5.4)** requires only `instrument`, an `AssetId`. The bid members are `null` or
    absent in the regular session and for crypto.
13. **Delegation preview (§5.3).**
    - `until_close` takes no caps or expiry: those are derived.
    - The timed shapes require all three.
    - `max_orders` is 1 to 1,000.
    - The 30-day bound on `expires_at` is the server's, since it depends on the time of the call.
14. **Every `202` has `phase: const "recorded"`.** `step_up_status` (`bound`, `missing`, or
    `unbound`) is optional, and only the kill switch sends it.
15. **Command status (§5.5).**
    - `phase` is closed: `recorded`, `taken`, `applied`, `refused`, or `ended`. It is never
      `unknown`.
    - `recorded` has no steps; every other phase has at least one.
    - A step is `{stream_id, seq, event_type, recorded_at, reason}`.
    - `seq` starts at 1, as the journal's does. That is tighter than the draft's 0.
16. **Owner request (§4.6)** is `{instrument: AssetId, side: buy | sell, size: Decimal | null}`.
    It has no `requested_by` (API-6).
17. **OAuth start response.** DEC-821 fixes the redirect; the response's shape is this DEC's own.
    - It is `{authorization_url, expires_at}`, and the URL must be `https`.
    - `propertyNames` bars every secret-shaped member name, case-insensitively, written with
      explicit character classes (API-11).
18. **Revoke (§4.5, §5.6).** Every member is optional. A `compromised` that is absent or `false`
    is the ordinary revoke.
19. **CSRF, pinned and scoped (§3.3 item 1, the lead's rulings of 2026-10-08).**
    - The custom header is `X-Mandate-Request: 1`. `Origin` must equal the deployment's configured
      app origin (`https://app.owlhead.ai` for the demo).
    - Only calls the browser session cookie authenticates are checked. DPoP-authenticated calls
      (the CLI, clients, service accounts) send no `Origin` and are not checked.
    - Order: authenticate first (401, one body, API-1), then CSRF (403 `forbidden`, `effect: none`,
      `retryable: false`), before any other read or write. §3.5's `forbidden` row names it.
    - It applies to the API-7 operations. Failing it falls under API-7's existing "failed
      authentication": a cross-site request is not the owner's authenticated call. It is not a new
      refusal reason.
    - **Residual:** a misconfigured app origin refuses every browser call, Stop included. The CLI
      (DPoP, unchecked) and the host routes (identity spec §6.4) remain the fallback, and the
      deployment's own health check should exercise a browser-origin call.
20. **§3.4's derivation is in the spec text**, so a client implements it from the spec: DEC-681
    item 5's canonical object, hash, and Crockford encoding. The operation names become a closed
    table with the route table (A1c).
21. **`effect` is required in every problem** (it is in `Problem`'s `required`). A client treats a
    non-2xx response without a listed `effect` as `unknown`, never as `none`.
22. **`Violation` is one tagged type**, shared by the 422 problem and the validate read model (A4),
    with `oneOf` on `kind` (the lead's ruling of 2026-10-08).
    - `rule`: `{path, code, message}`, where `code` is a mandate V-rule id (`V-022`).
    - `policy`: `{path, key, level, value, ancestor_level, ancestor_value, message}`. This is
      mandate spec §4.3's report: the key, the violating level and its value, and the nearest
      ancestor whose value it breaks (FR-1.5).
      - `value` is not in the lead's list; §4.3 names it, so it is carried too.
      - `level` is `organization`, `workspace`, or `mandate`, and `ancestor_level` is `platform`,
        `organization`, or `workspace` (`PolicyLevel` is the four in order).
      - `PolicyValue` is closed: a `Decimal` string for every number (counts and seconds
        included), a boolean for a permission or requirement, or the sorted set of a set key.
    - `schema`: `{path, code, message}` for a body that fails its shape or canonical form (§3.5's
      `invalid`), where `code` is a lowercase wire code (`non_canonical`, `type`, ...). It is
      neither a V-rule nor a policy finding, so it is a third kind rather than a misuse of either.
    - `path` is a JSON pointer whose segments hold no control character.
23. **No Rust type of the API uses `#[serde(flatten)]`** (the lead's ruling of 2026-10-08). serde
    cannot refuse an unknown member of a struct that flattens another, so such a type accepts what
    `additionalProperties: false` and `unevaluatedProperties: false` refuse. A1b's drift test
    found it on the responses, which carry the envelope's four members inline. `mandate-api`'s
    `tests/schemas.rs` refuses the attribute in the crate's source.
24. **`outcome_unknown` (503)** is the code for an append whose outcome could not be confirmed
    (the lead's ruling of 2026-10-08). §3.5 had `effect: unknown` but no code to carry it.
    - It carries `effect: unknown`, the derived `event_id`, and `retryable: false`.
    - The client polls §5.5 with that `event_id` and never resends with a new key. A resend with
      the same key is still safe (API-4), so `retryable: false` is a rule for the UI.
    - It is listed `(planned: E10-10)` in §3.5 and `x-planned` in the schema until `mandate-api`
      serves it (DEC-683).
25. **API-6's text names the refusal.** A request body naming `requested_by`, or any member its
    schema does not name, is refused `invalid` and journals nothing. The invariant's test reads
    so (DEC-681 item 6).
26. **Every pattern ends at the string's end.** The shared checker matches with ECMA `$`, and each
    of this change's patterns has a refuse case with a trailing newline.

27. **API-7 operations are lenient about their bodies** (the lead's ruling of 2026-10-08, the
    #788 review's M4 and M5).
    - Only the hard members are strict: the path ids, the kill switch's `scope`, an owner exit's
      `instrument` (without it there is nothing to exit; this reading is the builder's), and a
      Skip's `verdict` and `content_hash`.
    - Every other member that is omitted, unknown, or fails to parse is dropped, not refused. The
      `202` (`command-accepted`, `approval-response-accepted`) lists each dropped member's JSON
      pointer in `dropped`.
    - An owner exit's bid confirmation is all or nothing: a partial or unparsable one is dropped
      whole, and equities wait for the session. A kill switch's malformed `owner_exit` is dropped
      whole, and the switch still stops.
    - The schemas keep the strict shapes as what a client should send and carry
      `x-api7-lenient: true` with prose naming the hard members. The server's own cases are in
      `schemas/workspace-api/examples/lenient/api7.json`, which A2's tests read.
    - Journaling the dropped list on the command event is a backlog row (a journal change), not
      done here.
    - This supersedes, for the six lenient operations only, DEC-681 item 6's refusal of unknown
      members: a body naming `requested_by` on a pause or hold is dropped, never read, and listed,
      and API-6's text and test are two-tier to match (the re-review's B1).
    - A pause or hold body that is not JSON is read as `{}` with `dropped: [""]`. This tightens the
      leniency toward never refusing a stop, and it adds no risk: nothing is read from such a body.
    - A workspace-scope kill switch with a non-null `scope.id` has the id dropped and listed.
    - Skip's `content_hash` stays strict on purpose: an answer binds what was shown (API-12).
    - API-4's "same body" is judged over the members kept, so a retry differing only in a dropped
      member resolves to the first outcome.
    - The six lenient schemas are exactly pause, hold, kill switch, owner exit, the approval
      response (Skip half), and ending a delegation. Away mode has no request schema yet, and a
      revoke's kill-switch half is the revoke's own strict body. `check_lenient.py` checks the set
      and the server's cases.

28. **Each problem code pins everything about itself** (the #788 review's M6 and M7).
    - Its `status`, `type` (`https://mandate.dev/problems/<code>`), the `effect`s it may carry,
      `retryable`, and a fixed `title` with no content (API-10).
    - `effect`: `none` for every code that never records; `none` or `recorded` for
      `step_up_required` and `control_stream_frozen` (§5.6's batch); `unknown` only for
      `outcome_unknown`.
    - `retryable`: true for `journal_unavailable`, `rate_limited` and `membership_unavailable`,
      false otherwise.
    - #766's six identity codes joined §3.5 on main before this merged, so `ProblemCode` lists
      them and each has its branch: `own_roles`, `owner_role_reserved` and `reduction_only` 403,
      `last_owner` and `last_admin` 409, `membership_unavailable` 503; `effect` `none` only; titles
      "Cannot change own roles", "Owner role reserved", "Owner must remain", "Admin must remain",
      "Risk reduction only" and "Membership unavailable", as #780's DEC-681 item 14 gives them.
    - `current_base` is present exactly for `stale_base`: the base now in force, a version hash or
      a draft id (API-19).
    - A problem is not wrapped in the envelope (§3.2).
29. **The delegation preview names its delegation, and ending one already gone has its own
    `200`.**
    - The preview's `delegation` carries the five entered members and `source_approval_id`; the
      rest of mandate spec §6.5's members are planned with E8-15.
    - `commands/end-delegation-already-ended` is `{outcome: "already_ended"}` with the envelope,
      committing nothing (§5.3).
30. **Quoted content's author** is `owner_selected` or `platform_authored` (`common.schema.json`,
    DEC-740 item 8): wire codes for the journal's two display labels, "Output of software you
    selected" and "Platform-authored". DEC-681 item 7 says the same.

31. **The schemas' examples kill every structural mutant** (the #788 review's test-coverage ask).
    - `schemas/workspace-api/mutate_schemas.py` changes one constraint at a time in
      `envelope.schema.json` and every `commands/` file: a required member, an enum value, a
      `const`, `additionalProperties` or `unevaluatedProperties`, a pattern, a bound, a `oneOf`
      branch, a `then` or `else`, an `allOf` entry. It runs the shared checker on each mutant.
    - Every mutant fails the checker but six equivalent ones, listed in the script with their
      reasons: `Problem`'s `code` requirement, its `type` and `title` lengths, its `status` enum,
      and the two `effect` enums that `Effect` already closes. Each is implied by a per-code pin
      or a shared definition.
    - `check_planned.py` checks DEC-683's markers. Both scripts, with `check_examples.py`, run from
      a `cargo xtask` job once one is wired (backlog row); until then reviewers run them.

32. **CORS for the cross-origin web app** (§3.3 item 5; the lead's ruling with L4 of 2026-10-08).
    - Only the exact configured app origin, with credentials and `Vary: Origin`. Other origins get
      no CORS header.
    - The preflight allows the five methods and four headers the API reads, `Max-Age` 600. It is
      unauthenticated and reads nothing, an API-1 exemption beside the liveness probe.
    - Exposed: `ETag`, `Location`, `Retry-After`. Error responses carry the CORS headers too.
    - The app origin must be same-site with the API (a subdomain of the same registrable domain);
      the app is served on `app.owlhead.ai`, and an origin on another site never works.
    - The session cookie is `__Host-`, `Secure`, `HttpOnly`, `SameSite=Strict`, `Path=/`.
    - A2 tests each: an origin not allowed gets no `Access-Control-Allow-Origin`, the preflight
      never authenticates or reads data, and the cookie's attributes are exact.
33. **The coordinator's conditions on API-7** (2026-10-08). CSRF never applies to identity spec
    §6.4 route 2 (the local passkey route) or to DPoP-authenticated calls, so the CLI and the host
    routes stay open as the kill-switch path (item 19). Stop (the kill switch and owner exit) and
    Skip never answer 422 for an optional member, whether omitted, unknown, or malformed (item
    27). A2 tests both; the server's cases are in `examples/lenient/api7.json`.

### Rationale

The spec gives each shape's members but not every optional, nullable, or conditional rule. Each
reading above follows the spec's own words, or picks the form that refuses more input without
refusing a risk-reducing call. The kill switch, pause, Skip, and owner exit stay acceptable with
nothing but their target.

L4's builder C drafted the first version of these schemas. The cases C listed are the fixture
cases in `schemas/workspace-api/examples/`.

### Alternatives considered

- **Requiring every member of every request, nullable.** It would refuse a kill switch from a
  client that sends only its scope, which API-7 forbids.
- **An open `phase` or `step_up_status`.** §3.2 closes every enum whose values change meaning.
- **Validating step-up presence in the schema.** It would turn a 401 into a 422 and refuse a Skip
  that carries none.

<a id="dec-683"></a>

## DEC-683: A spec or schema may list a value before the code serves it, marked planned, under checks that bound the excuse

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176, on the lead's rulings of 2026-10-08 (option (d), its second variant; the #788 review's B3). A reversible process rule that weakens no test: every value stays checked in both directions |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); ES-22; DEC-682; the #788 re-review's item 4a |

### Decision

1. **The hazard.** `mandate-api`'s drift tests compare its closed enums and route table with the
   spec's tables and the schema files, in both directions. ES-22 forbids changing a spec or schema
   in the same PR as code. So a new value would turn `main` red between the spec PR and the code
   PR, whichever lands first.
2. **The markers.**
   - A spec table row carries `(planned: <story>)`.
   - A schema enum's object carries `"x-planned": {"<value>": "<story>"}` beside `enum`. JSON
     Schema ignores unknown keywords, so validation is unchanged.
3. **The rule.** For each closed set, with `S` the values listed, `P` those marked planned, and `R`
   the Rust values:
   - every value of `R` is in `S`;
   - every value of `S` not in `P` is in `R`;
   - a value in both `R` and `P` passes only while its story is named in the drift tests' in-flight
     list (item 5).
4. **The spec and schema agree** (`schemas/workspace-api/check_planned.py`).
   - Every marker names a story the backlog lists.
   - Every `x-planned` value is in its enum.
   - For each spec table paired with a schema enum (today §3.5 and `ProblemCode`), the codes are
     equal, and the planned rows equal `x-planned`, story for story.
5. **The excuse is bounded.** "In Rust and planned" is the one state between the code PR and the
   marker-removal PR.
   - The code PR that adds a planned value names its own story in the drift tests' in-flight list.
     The list is code, reviewed in that PR, and a value planned under any other story still fails.
   - When the marker goes, the excuse goes with it, because the value is no longer planned.
   - The entry is removed by the next code PR on the crate; a later value planned under the same
     story is the same story's work.
   - A `cargo xtask` check stays owed: it must reject a planned marker whose story is done, using the
     backlog's story states. It is a founder-owned xtask change.
   - Adding `x-planned` (or a `(planned: ...)` marker) to a value the code already serves needs a
     reviewer's explicit sign-off, because it re-opens the excuse. The PR template's checklist asks
     for it.
6. **The order.**
   1. The spec or schema PR adds the value, marked planned; `main` stays green.
   2. The code PR adds the value and its story to the in-flight list; `main` stays green.
   3. A spec or schema PR removes the marker; `main` stays green.
7. **Scope.** The rule covers every lane's spec tables and schemas that a Rust drift test reads
   (L4's read models, L5's audit types).

### Rationale

Strictness in both directions is what lets the drift tests catch a renamed or dropped value. The
marker keeps that strictness for every unmarked value, and the in-flight list confines the one
excused state to a named story in a reviewed change. An environment variable or flag would let any
run excuse anything, so none is read.

### Alternatives considered

- **The lead's first variant**, excusing "in Rust and planned" only for the PR's own story: no test
  can reliably read which story a change belongs to.
- **An environment variable naming in-flight stories:** unbounded, since CI or a developer could set
  it for any run.
- **An allowlist the spec PR edits:** ES-22 keeps spec PRs out of code.
- **One-directional drift** (only Rust ⊆ spec live): a dropped variant would pass silently.
- **An ES-22 exception:** founder-reserved, and needed on every enum change.

<a id="dec-684"></a>

## DEC-684: How the workspace API's route table reads spec §4 where it leaves a choice

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176. Each item gives a route no wider access than the spec's nearest row, or settles a path the spec does not name |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); [workspace API spec](../../specs/workspace-api.md) §3.6 to §3.8, §4; identity spec §4.2; DEC-821 |

### Decision

A1c will define `mandate_api::routes::ROUTES`, listing every operation of §4, §3.1's change
notices, and §3.6's step-up challenge, with tests that check the table against the spec's text.
No Rust is in the PR that records this decision. Where the spec leaves a choice, the table will
read it as follows.

1. **Paths the spec does not give.**
   - Lifting a hold is `POST /agents/{agent_id}/hold/lift` (the lead's ruling), committing
     `OwnerCommandIssued` `lift_hold`. It is user-only, requires step-up, and is not API-7. §4.2
     names the command but no route; the schemas PR adds the row.
   - The OAuth redirect will be the absolute `GET /v1/oauth/alpaca/callback`, with the workspace
     bound in `state` (DEC-821), once [#769](https://github.com/kunwarshivam/mandate/pull/769)
     (pending) changes §4.5's `GET /connections/oauth/callback` to it. A1c's table lists the new
     path, and its drift test exempts that one path until #769 merges.
2. **Reads take the narrowest row that covers them.**
   - Drafts, versions, diffs, the confirmation screen, the replay, and backtests (run and report)
     need *Draft a mandate version*. §4.1 maps drafts there; backtests have no row of their own and
     precede a deploy.
   - Reading the workspace policy needs *Workspace policy: tighten* (workspace admin), the only
     policy row a viewer lacks.
   - Listing clients needs *Revoke a client* (workspace admin, operator).
   - Listing members needs *Invite, deactivate, remove a workspace member*.
   - Connections, approvals, notices, command status, and the change notices need *View agents,
     positions, decisions*. §3.7 maps the read models and the approvals inbox there.
3. **Body-dependent permissions are the server's reading** (API-5):
   - a confirm by its own §9.2 classification;
   - an approval response by its verdict (*approve* or *skip*);
   - the kill switch by its scope, plus the privileges row when the body asks for an owner exit;
   - a revoke on compromise as both the connection row and the kill switch's connection row;
   - a policy write as tighten or loosen by the server's comparison.
4. **A step-up challenge for an action of kind K is authorized exactly as that action** (L1's
   answer of 2026-10-08; identity spec §7.2 step 1, DEC-816). K maps to its permission row (`STEP_UP_PERMISSIONS`), and `authorize`
   runs for that row.
   - A principal who could not act cannot mint a challenge.
   - A client or a service account is refused by the blank cells of every step-up row (identity
     §6.6).
   - A reduction-only session reaches no step-up row and is refused `reduction_only`.
   - Verification is `mandate-passkey`'s.
5. **The signed-in user's own memberships** are `GET /v1/me/workspaces`, §4.5's row marked
   `(planned: E10-10)` until [#811](https://github.com/kunwarshivam/mandate/pull/811) (pending)
   adds the permission, outside every workspace path: *List one's own workspace memberships* at the principal's own scope (identity spec §4.5).
   It reads no workspace's data and returns only the caller's memberships (API-9).
6. **Step-up rules.** Each route is one of:
   - none;
   - required (refused `step_up_required`);
   - required only for the half of the body that adds risk;
   - forwarded for the stream owner to judge: resume, Stop, acknowledge, owner exit, and the kill
     switch's privilege (§5.4). Lifting a hold requires it outright (DEC-191: the owner's alone, with
     step-up).
   No API-7 operation requires step-up outright.
7. **Client scopes are the table's**, by §3.8's list. `authorize` checks roles only (L1's
   contract).
   - `read` is: list and read agents, positions, the plan, the daily brief, and the change notices.
   - `request` is the owner request.
   - `propose` is creating a draft.
   - `dry_run` is the dry run.
   - `hold` is holding new openings.
   No other route is a client's.
8. **§4.8's routes are served by the audit lane (L5).** The timeline is one of them.

### Rationale

The spec's §4 tables name every operation but leave a few paths and read permissions open. Each
reading takes the row that grants the fewest roles while keeping the screen usable. None widens
what a client can reach.

### Alternatives considered

- **Reads under *View agents* everywhere.** It would let a viewer read the workspace policy,
  members, and clients, which no §3.7 row gives them.
- **`POST /agents/{agent_id}/hold` with `{lift: true}` for lifting a hold.** One path for two
  permissions, the second of which needs step-up; a separate route keeps the matrix one row per
  route.

<a id="dec-685"></a>

## DEC-685: A live key that reports no permissions is refused as `permissions_unreadable`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L2 lead, under DEC-176, on the coordinator's ruling of 2026-10-09. It only tightens check 1 and loosens nothing |
| **Story or claim** | E7-12, claim [#764](https://github.com/kunwarshivam/mandate/issues/764); corrects the tests of [#825](https://github.com/kunwarshivam/mandate/pull/825); [connections spec](../../specs/connections.md) §8.1 check 1, CN-2; DEC-441 item 4 |

### Decision

1. **Live: refused.** At check 1 (scope), a live credential whose permissions were read and are
   empty (`Granted::KeyPermissions(Some(empty))`) is refused with reason `permissions_unreadable`.
   It cannot show that fund movement is absent (CN-2, DEC-441 item 4). It is treated as a key whose
   permissions could not be read.
2. **Paper: passes.** A paper or demo credential whose permissions were read and are empty still
   passes check 1. A paper key cannot move money, and DEC-441 item 4 keeps the record-and-disclose
   reading for paper and demo keys.

### Rationale

A live key that reports no permissions at all is suspicious. Every key that can trade has at least
one permission, so an empty set more likely means the read went wrong than that the key is narrow.
Live keys are reserved for the founder (DEC-79), so the refusal blocks nothing today. The reading
only tightens check 1 and adds no risk, so an agent may take it under DEC-176.

### Alternatives considered

- **Accept the empty live set, as #825's tests pinned.** Rejected: an empty answer does not show
  fund movement absent, and DEC-441 item 4 refuses a live credential that cannot show it.
- **Refuse an empty set on paper keys too.** Rejected: it blocks fixtures and demo connections, and
  a paper key cannot move money.

<a id="dec-686"></a>

## DEC-686: The workspace API reports identity's five step-up refusals, and `step_up_required` means no evidence at all

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176, on the L2 lead's slice of 2026-10-08. Every item refuses as before and only names the reason; none loosens a rule |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); [workspace API spec](../../specs/workspace-api.md) §3.5, §3.6; identity spec §7.2, §7.3 |

### Decision

1. **Six codes, one per reason.** Identity spec §7.2 step 6 names five step-up refusals:
   `step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`, and `step_up_mismatch`.
   Until now the workspace API's §3.5 listed only `step_up_required`, and `step_up_stale` appeared
   only in §9's prose. §3.5 now lists all six, each 401 with `retryable: false`.
2. **`step_up_required` means the call presents no evidence.** A call that needs step-up and
   carries no `step_up` member, or a null one, is refused `step_up_required`. That check comes
   before any other step-up check. Evidence that is present is judged in the order of identity spec
   §7.2 step 4, and the first failing check names one of the other five codes:
   1. the challenge exists: else `step_up_missing`;
   2. the method is allowed in this environment (§7.3, reported by step 6): else `step_up_method`.
      Step 4 does not list the method, so this decision places it right after the challenge is
      found, before any cryptographic work; every placement refuses the same calls and only names
      the reason;
   3. the challenge is unused: else `step_up_reused`;
   4. the challenge is not expired, and is not presented before its `issued_at`: else
      `step_up_stale`. So an expired challenge that was also used is `step_up_reused`;
   5. the challenge names this principal and workspace: else `step_up_mismatch`;
   6. the credential belongs to this principal and its enrolment cool-off has ended, the signature
      verifies, user verification shows in the flags, and the counter has not gone backwards: else
      `step_up_missing`;
   7. the submitted action's digest equals the challenge's: else `step_up_mismatch`.

   So `step_up_missing` never means "nothing was sent". It means something was sent that does not
   count, as mandate spec §6.1 counts evidence that does not count as missing. These codes are
   returned only to the authenticated requester, with no detail beyond the code.
3. **Effect.** Nothing the step-up guards is committed, so the effect is `none`. The one exception
   is a revoke on compromise (§5.6): its kill-switch half is an API-7 operation and is committed
   anyway, so the refusal of the revocation carries `effect: recorded`. The schema cannot tell that
   call apart, so each of the five allows both effects and says so in a `description`.
4. **Never for an API-7 operation.** None of the six is sent for pause, the kill switch, an owner
   exit, or Skip. There, failed step-up loses only the privilege it would have added (identity spec
   §7.3, ID-5).
5. **The challenge source.** `POST /v1/workspaces/{ws}/step-up/challenges` is the only source of
   the WebAuthn challenge the authenticator signs. That challenge is
   `mandate_passkey::stepup::ChallengeRecord::webauthn_challenge`, the SHA-256 of the canonical
   challenge record (identity spec §7.2 steps 1 and 2).
6. **The schema moves with the spec.** `envelope.schema.json`'s `ProblemCode` gains the five codes,
   each pinned like `step_up_required`: status 401, `retryable: false`, `effect` `none` or
   `recorded`, and its own `type` and generic `title`. DEC-683's check requires §3.5's codes and
   the enum to be equal, so the two change in one PR, stacked on
   [#788](https://github.com/kunwarshivam/mandate/pull/788), which adds the enum.
7. **The five are planned.** The crate does not serve them yet: `mandate-api`'s `ProblemCode` has
   no variant for them, and its contract test refuses a served code with no variant. So each of
   the five §3.5 rows is marked `(planned: E10-10)` and listed in the enum's `x-planned`, as
   `outcome_unknown` is. DEC-683's rule covers codes the code already serves; these are not served
   yet. The crate PR (A1b) adds the five `ProblemCode` variants and removes the markers in the
   same change.

### Rationale

A client needs to know whether to ask the user again (any of the six) and why: an expired
challenge, a reused one, or the wrong method each need a different message. Reporting identity's
own code keeps the API's reason and the journal's the same. Defining `step_up_required` as "no
evidence" gives the two overlapping names disjoint meanings, and every request still gets exactly
one code.

### Alternatives considered

- **`step_up_required` for every step-up failure.** One code would hide which check failed, and
  the client could not tell an expired challenge from the wrong method.
- **Dropping `step_up_required` for `step_up_missing`.** That would break the existing contract and
  the brief's copy for "step-up needed", and blur absent evidence with evidence that does not count.
- **403 for mismatch and reuse.** A new ceremony fixes all six, which is what 401 tells the client.

<a id="dec-688"></a>

## DEC-688: CI runs the workspace API schema checkers in `full` and their mutation sweep as its own job

| | |
|---|---|
| **Status** | Accepted; item 2's shard count superseded by [DEC-1004](earlier-900-1999.md#dec-1004) (six shards) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176: a reversible engineering decision that adds checks and removes none |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); claim [#835](https://github.com/kunwarshivam/mandate/issues/835) |

### Decision

1. **`cargo xtask ci schemas`** runs `check_examples.py`, `check_planned.py` and `check_lenient.py`
   from `schemas/workspace-api/`, in the environment `reference/mandate/requirements.txt` pins. It
   takes about a second and is part of `cargo xtask ci full`.
2. **`cargo xtask ci schema-mutants [K/N]`** runs `mutate_schemas.py [--shard K/N]`. Unsharded it
   took 7m17s locally with #941's schema changes applied, so CI runs it as its own `schema-mutants`
   job, a fixed matrix of three shards (`0/3`, `1/3`, `2/3`, `fail-fast: false`) of 206 mutants
   each, which took 2m25s, 2m35s and 2m54s locally. The required `full` verdict passes only when
   the matrix succeeded, which means every shard did. `full-checks` and each shard stay under ten
   minutes (DEC-464). `cargo xtask check` and the nightly run the sweep unsharded. Every run, each
   shard included, first runs the three checkers on the unmutated tree and runs no mutant unless
   all pass: the sweep counts a mutant caught when any checker fails, so a failing baseline would
   report every mutant caught.
3. **`docs/specs/workspace-api.md` is not documentation** to `.github/scripts/docs-only.sh`:
   `check_planned.py` reads it, so a change to it alone takes the full path.

### Rationale

Until now the checkers ran only by hand, so a schema edit could merge with an example, a planned
marker or the API-7 leniency out of step. The mutation sweep is what shows the examples pin every
constraint; inside `full-checks` it would push that job near the ten-minute cap.

### Alternatives considered

- **The sweep in the nightly only.** Cheaper, but a schema change could merge with a surviving
  mutant and fail main the next night.
- **The sweep inside `full-checks`.** One job fewer, but about seven minutes added to the job
  DEC-464 caps at ten.
- **One unsharded job.** About seven minutes of sweep plus 1.5 to 2.5 minutes of checkout, setup
  and build put a required check at the cap; DEC-464 says to add parallel shards.

<a id="dec-689"></a>

## DEC-689: How `mandate-api` types the workspace API's shapes: serde plus `validate()`, strict wire types, no flatten

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L2 builder (Claude Code), under DEC-79 and DEC-176: reversible engineering choices that add no trading rule. Items 1 to 4 confirmed by the coordinator on 2026-10-09; item 5 is the coordinator's ruling of the same day |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750), A1b-1; [workspace API spec](../../specs/workspace-api.md) §3, §5; DEC-682, DEC-683, DEC-686 |

### Decision

1. **Decode is serde plus `validate()`.** Members, closed enums, and tagged unions (`Actor` on
   `kind`) are serde's, with `deny_unknown_fields` wherever the schema is closed. What serde cannot
   express (a pattern on a plain string, a numeric bound, an array's length or uniqueness, one member
   conditioned on another) is `wire::Validate`, which `decode` requires of every shape and calls
   after the shape decodes. A shape with no such rule says so (`rules!(none: …)`).
2. **The API-7 shapes are strict.** The types say what a client should send; A2's
   `decode_lenient` drops what DEC-682 item 27 lets an API-7 operation drop, and lists it.
3. **Wire types are separate** from `mandate-identity`'s and the journal's (`StepUpEvidence`,
   `Actor`, `Watermark`). The server converts between them. No new dependency; the crate stays at
   layer 4.
4. **No `#[serde(flatten)]`** (DEC-682 item 23). Each response repeats the envelope's four members
   through one `response!` macro; `tests/schemas.rs` refuses the attribute in `src/`.
5. **DEC-686 item 7 is read as:** A1b adds the six `ProblemCode` variants (`outcome_unknown` and
   the five `step_up_*` codes) and keeps their `x-planned` and `(planned: E10-10)` markers. A
   schemas-only PR removes them once the implementation serves the codes, in DEC-683 item 6's order
   and under ES-22. This only tightens: no marker goes before the code serves its value.
6. **A1b is split** to fit 400 lines: A1b-1 (the envelope's shapes, `CommandAccepted`, the six
   variants, and the drift and example harness), A1b-2 (the plain requests, with
   `ApprovalResponseAccepted`, `ConfirmAccepted`, and `EndDelegationAlreadyEnded`), and A1b-3 (the
   conditional requests, with `DelegationPreview` and `CommandStatus`). `oauth-start-response` is
   E10-13's.

### Rationale

serde already refuses unknown members, wrong types, and closed-set misses with a located error;
one more trait holds the remaining rules in one place that `decode` cannot skip. Strict types keep
the schema's shape as the contract, and leniency lives where API-7 needs it. Separate wire types
keep the contract's spelling from moving with an internal type. Repetition through one macro keeps
each response closed.

### Alternatives considered

- **Newtypes with `try_from` for every constrained member.** It moves rules into Deserialize, but
  a cross-member rule (`CommandStatus`'s steps by phase) still needs a struct-level check.
- **`#[serde(flatten)]` of an `Envelope` struct.** serde cannot then refuse an unknown member.
- **Reusing `mandate-identity`'s types.** It ties the wire to an internal type and a lower crate.
- **Removing the markers in the code PR.** ES-22 forbids schema and code in one PR.

<a id="dec-691"></a>

## DEC-691: The `mandate-connections` crate and the shape of the Alpaca OAuth connect core

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), E10-13 slice D2, under DEC-79 and DEC-176: a crate boundary and types that only tighten the paths DEC-690 item 1 and DEC-821 item 3 allow. The L2 lead approved the layout |
| **Story or claim** | E10-13, claim #754; [connections spec](../../specs/connections.md) §5.2, §8.1; DEC-690 item 1; [DEC-821](DEC-821.md) items 2, 3 and 4 |

### Decision

1. **A new crate, `mandate-connections`**, layer 6 beside `mandate-executor`.
   - It is safety-critical (broker connections, OAuth scopes, credential handling), with the lint
     header and a CODEOWNERS line.
   - It is pure: its external dependencies are `thiserror` (D2a) and `secrecy` (from D2b, for
     the secret wrappers), so it has no async runtime, HTTP client, or database.
   - Every network call is a trait that an adapter implements: the HTTP routes in
     `mandate-api-server` (DEC-680), the token endpoint and vault write in the token-exchange
     process, and the paper-host requests in the executor.
2. **Which process may call what is a module boundary.**
   - `start`, the `state` and the authorization URL, is the API process's. It names no token type.
   - `exchange`, the code exchange, and `grant::check_scope` on its token response are the
     token-exchange process's (DEC-821 item 2; DEC-690 item 1). That process is not the
     executor: it holds no order client and no token lease, and it is the only process that
     sends a `LiveTokenRequest`. The scope is checked there, before the vault write, so an
     over-scoped token is never stored (CN-2).
   - The executor sends only `PaperRequest`s; its egress contains no live host (DEC-821
     item 4).
   - `ExchangedGrant` is what the permission checks (E7-12) receive, and it carries no secret.
3. **The live host is reachable by type, not by a runtime check** (DEC-821 item 2).
   - `hosts::LiveTokenRequest` has a private field and one crate-private constructor, so it cannot
     be built outside the crate (a `compile_fail` doctest proves it). It means
     `POST https://api.alpaca.markets/oauth/token` and nothing else.
   - Every other request is a `PaperRequest`, which can address only the paper host
     (DEC-821 item 4: every order, trading, and account call goes to the paper host only). The live host and its URL are crate-private constants; an adapter gets the
     one live URL only from `LiveTokenRequest::url`.
   - An adapter holding a raw URL passes it through `hosts::admit` and sends only the resulting
     `Outbound`.
   - `admit` matches the token URL exactly. A different case, port, query, fragment, user info, or
     dot segment is refused, which is stricter than URL equivalence.
4. **Paper only, enforced twice** (DEC-821 items 3 and 4). `start` refuses a live binding before issuing a
   `state`. `exchange` refuses a live connection before touching the vault or the endpoint. The
   authorization URL always carries `env=paper` and `scope=trading data`. The granted scope must
   be exactly those two names; a duplicate or a change of case is refused.
5. **Secrets stay in `SecretString` wrappers with redacting `Debug`** (DEC-821 item 4; rule 7). The wrappers
   are the code, the token, the client secret, and the PKCE verifier. The `Vault` trait has no
   method that returns a token. A refused exchange leaves no token and deletes the vault entry.
6. **A `state` is spent whenever it is found**: on success, on expiry, and on another user's
   attempt. So a leaked or guessed `state` can never be tried twice. It lives 600 seconds
   (connections spec §5.2, Proposed default). Its last valid second is the 599th after issue.
7. **The core reads no clock and draws no randomness.** The caller supplies the `state`, the PKCE
   verifier and challenge, and the time. Computing the S256 challenge is left to the adapter,
   because PKCE is never relied on (DEC-690 item 9).
8. **Shared crate.** Lane L2's δ adds the connection record and fingerprint (E7-11) and the
   permission checks (E7-12) as their own modules. This lane owns `lib.rs`, and δ adds `pub mod`
   lines.

### Rationale

The trust ladder's first rung: a live-host request that cannot be constructed cannot be sent by
mistake, by a future adapter, or by a careless refactor. Keeping the crate pure keeps every rule
testable without a network and keeps the HTTP adapter thin. Splitting `start` from `exchange` by
module makes DEC-821 item 2's process split visible in the code: the exchange runs in the
token-exchange process, never in the executor.

### Alternatives considered

- **Put the connect core in the API server crate.** Rejected: that crate is async and impure, and
  it would put the exchange code in the API process's binary.
- **A runtime allowlist of URLs.** Rejected as the only control. `admit` exists for adapters, but
  the type is the guarantee.
- **Accept URL-equivalent spellings of the token URL.** Rejected: refusing them costs nothing,
  because the adapter sends the constant.

<a id="dec-692"></a>

## DEC-692: The local vault (V1): two keys, per-process directories, and checks before every read

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), E10-13 slice V1, under DEC-79 and DEC-176: each item tightens how a credential is held, and none adds a custody path beyond DEC-690 item 1 and DEC-822 item 4. The L2 lead approved the layout and the requirements |
| **Story or claim** | E10-13, claim #754; [connections spec](../../specs/connections.md) §5.2; [infrastructure](../../design/infrastructure.md) §3.1, §5.2; DEC-690 item 1; DEC-822 item 4 (founder); deploy PR #795 |

### Decision

The demo host keeps broker credentials in a local vault, `mandate-vault-local`, until the full
vault of infrastructure §5 replaces it. It holds three kinds of secret, all encrypted at rest:
OAuth authorization codes, access tokens, and the platform's Alpaca OAuth client secret.

1. **Two Unix users, two directories.** The API process runs as `owlhead_api`, the connection
   executor as `owlhead_exec`. They are never in each other's group, and the executor never runs
   inside the API process. Under `MANDATE_VAULT_DIR` (default `/var/lib/owlhead/vault`), the
   layout is:

   | Directory | Owner and group | Mode | Contents |
   |---|---|---|---|
   | The vault itself | root | `0755` | — |
   | `pending/` | `owlhead_api:owlhead_exec` | `2770` (setgid) | Codes. Files `0640`, so the executor reads and deletes them |
   | `tokens/` | `owlhead_exec:owlhead_api` | `0730` | Tokens and the client secret. Files `0600`. The API can unlink a known name but cannot list or read |

   The filesystem, not code, enforces that the API cannot read a token.
2. **Two keys, as systemd credentials, never environment variables** (DEC-822 item 4). Together
   they are DEC-822's single "vault key":
   - The pending key (`vault-pending-key`) encrypts `pending/`.
   - The token key (`vault-token-key`) encrypts `tokens/`.

   Each is a root-only `0600` file loaded with `LoadCredential=` and read from
   `$CREDENTIALS_DIRECTORY`. The API unit loads only the pending key; the executor unit loads
   both. Without the token key, the API can neither read a token nor forge one.
3. **Startup refuses rather than repairs.** The vault refuses to start when:
   - any of `MANDATE_VAULT_KEY`, `MANDATE_VAULT_PENDING_KEY` or `MANDATE_VAULT_TOKEN_KEY` is set,
     whatever its value;
   - a key credential is missing, is not a regular file, or is not exactly 32 bytes;
   - the API process finds `vault-token-key` among its credentials;
   - any directory's owner, group, or exact mode (setgid included) differs from item 1, or the
     directory is a symlink.
4. **AEAD: ChaCha20-Poly1305 with a random nonce per entry.** The associated data binds the
   key_ref, the `connection_id`, the environment, and the entry kind (code, token, or client
   secret). A file renamed onto another connection's name, or moved between kinds, fails to
   decrypt.
5. **Every token or client-secret read checks the open file first.** The file is opened with
   `O_NOFOLLOW` and `fstat`'d. It must be a regular file owned by `owlhead_exec` with mode exactly
   `0600`, or it is refused before decryption. This stops a file the API planted through its
   group write on `tokens/`.
6. **The client secret lives in the vault** (DEC-822 item 4).
   - `mandate vault import alpaca-oauth-client-secret` reads it from stdin only, never argv, and
     runs as `owlhead_exec`.
   - It is stored under `tokens/` with key_ref `platform/alpaca-oauth-client-secret` and kind
     `client_secret`, and the import is journaled as `VaultOperationRecorded` (`put_client_secret`)
     without the value.
   - The connecting executor reads it only while `connecting`, through the single-use grant
     (connections §5.2), which is enforced in code on the demo host.
7. **Every write, read, and delete is journaled** as `VaultOperationRecorded` (E7-17's vault-events
   spec), with no value ever recorded.
8. **Backups** carry the vault's files with their owners and modes, and never `/etc/owlhead` or the
   credentials. After a restore without the keys, connections are re-authorized.

The tests come in two parts:
- **Part 1 (this PR):** startup, which covers items 2 and 3.
- **Part 2:** the AEAD binding, the fstat check, the import command, and the journal events,
  which cover items 4 to 7. It adds `chacha20poly1305`, `getrandom` and `libc`, each with a
  `docs/dependencies.md` row.

### Rationale

The API faces users and handles a code only in transit, so it should hold no key or file
permission that reaches a token. Splitting users, directories, and keys puts that boundary in the
kernel and the cipher rather than in code paths. Refusing to start on a wrong layout keeps a
misconfigured host from running with weaker custody than this decision describes.

### Alternatives considered

- **One service user and one key.** Rejected: the API could then read and forge tokens.
- **Keys in environment files.** Rejected by DEC-822 item 4. An environment leaks through
  `/proc`, crash reports, and child processes.
- **Repairing modes at startup.** Rejected: a wrong layout means a setup error worth surfacing,
  and a vault running as root to chmod would widen its own privilege.

<a id="dec-697"></a>

## DEC-697: A compromised revoke without valid step-up is the one plan that carries both an effect and a refusal

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), E10-13, from L2's assigned range DEC-670 to DEC-699, on the merge coordinator's ruling of 2026-10-09. A clarification under DEC-176: it states what [workspace API spec](../../specs/workspace-api.md) §5.6 and API-7 already require and loosens nothing |
| **Story or claim** | E10-13, claim #754; PR #1076 (the R1 tests); amends [DEC-694](DEC-694.md) item 1 |

### Decision

1. **DEC-694 item 1 is amended.** Its sentence "It never returns both" now reads: every plan is
   either an ordered list of effects or a refusal, never both, with exactly one exception (item 2).
2. **`RevokePlan::KillSwitchOnly` is that single exception.** A compromised revoke without valid
   step-up commits the connection-scoped kill switch and refuses only the revocation. The plan
   carries the kill-switch effect and the revocation's refusal together. This is what workspace
   API §5.6 and API-7 require, what [DEC-158](../04-decision-log.md#decisions) option (c) chose,
   and what AGENTS.md rule 13 demands: the kill switch is always available and never waits on
   step-up.
3. **No other plan, in `revoke` or in `manager`, may carry both.** A new case that needs both
   takes a new decision.
4. **The server maps `KillSwitchOnly` as DEC-694 item 2 says for effects**: it appends the kill
   switch's `OwnerCommandIssued` (`kill_switch`, connection scope) and reports the revocation's
   refusal, appending no `ConnectionRevoked`.

### Rationale

DEC-694 item 1 was written for the common case, and read literally it would force the compromised
path without step-up to refuse whole, dropping the kill switch. §5.6, API-7 and rule 13 forbid
that. PR #1076's tests pin `KillSwitchOnly`, and its review confirmed the reading follows the spec.
The exception is named and closed so that the server slice and later plans do not read "never both"
as permission to drop a protective effect, or the exception as permission to mix effects and
refusals elsewhere.

### Alternatives considered

- **Refuse the whole compromised revoke without step-up.** Rejected: it would deny a kill switch,
  against rule 13, §5.6, API-7 and DEC-158 (c).
- **Commit the revocation too without step-up.** Rejected: §5.6 requires step-up for the
  revocation, and DEC-158 (c) keeps it.
- **Edit DEC-694 in place.** Rejected: the decisions README (rule 4) changes an accepted decision
  only through a new one.

<a id="dec-704"></a>

## DEC-704: The dispatcher's step over the journal: its notice-stream writer, its first slice, and its readings of spec §5.1

| | |
|---|---|
| **Status** | Accepted; item 2 amended by [DEC-705](earlier-600-899.md#dec-705) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L3 builder agent (Claude Code) under DEC-79 (reversible engineering readings) and DEC-176 (each item only tightens what the spec allows the dispatcher to do, or resolves a gap by the reading that sends nothing more and drops nothing) |
| **Story or claim** | E8-10 slice D2, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [notifications spec](../../specs/notifications.md) §3.3, §3.4, §5.1, §5.5, NT-3, NT-8; [journal spec](../../specs/journal.md) §5.1, §9.15; DEC-701 item 3; DEC-702; DEC-725 item 7 |

### Decision

1. **The writer DEC-701 item 3 names.** `mandate_dispatcher::NoticeWriter` holds a `StreamId` and
   the writer epoch the dispatcher took for it, and `NoticeWriter::new` refuses every stream that
   is not `StreamType::Notice` (`DispatchError::NotANoticeStream`). The dispatcher reaches the
   journal only through its `Journal` trait, whose one append, `append_notices`, takes a
   `NoticeWriter` and no stream id, so no dispatcher code can name an agent, account, control, or
   scheduler stream to append to. The trait keeps the crate `pure = true`: the in-memory journal
   implements it in the tests, and the Postgres store does in the process slice.
2. **What one `step` does** (spec §5.1 steps 1 to 4, for the `OwnerAlertSent` causes). It reads
   its own notice stream and the committed rows of the subject streams it is given, in each
   stream's `seq` order, the subject streams in the order given. Each committed `OwnerAlertSent`
   whose notice key (its `owner_command` when it names one, its own event id otherwise, DEC-725
   item 7) has no `NoticeIssued` yet gets one, with a freshly minted notice id (DEC-702); the
   notice stream's `StreamOpened` comes first when the stream is empty. All of a step's new
   `NoticeIssued` are one batch, committed before any send. Then, for every notice on the stream
   whose kind has a text key, for every recipient it lists and each of that recipient's push
   channels with no `NoticeAttempted` yet, the step makes one send through the provider and
   journals its outcome as `NoticeAttempted`, attempt 1, before the next send: `accepted` as
   `delivered` with the provider's message id, `retryable` and `permanent` as `failed` with the
   reason (spec §5.5 has no other status for a send that did not go through).
3. **An append that does not commit stops the step before its next send.** `Fenced` is
   `DispatchError::Fenced { current_epoch }`; every other outcome but `Committed` is
   `DispatchError::NotCommitted` with the outcome's name and nothing from the drafts. A fenced
   dispatcher therefore sends nothing more, and a send whose record did not commit is made again
   by the next step, under the same idempotency key (spec §5.1: at least once, never lost).
4. **Recipients before roles.** Until roles exist (E9-2) and the identity spec's receive column is
   read (NT-10, a later slice), every notice goes to the configured audience, the founder's push
   addresses, as spec §3.3's interim says ("before roles exist every row resolves to the
   founder"). `NoticeIssued.recipients` is the audience's recipient ids, sorted and unique
   (journal spec rule 123), and a later step sends a notice only to recipients its own
   `NoticeIssued` lists, so an audience changed after issue adds no recipient to an earlier notice.
5. **The dispatcher's event ids** are the ULID text of 128 bits drawn from the same injected
   `SecureRandom` as the notice ids, each id its own draw. None is derived from a cause, a notice
   id, or a clock; journal spec §3 gives a ULID's time component no meaning.
6. **The draft's actor** is `{kind: system, id: dispatcher, version: <crate version>, build}`,
   with `build` the process's binary digest from its configuration.
7. **This slice leaves out**, each for its own later slice: `ApprovalRequested` causes, reminders,
   and the `not_pending` stop; retries after a `failed` attempt (`next_attempt`); quiet hours
   (`quiet_hours`); coalescing (`coalesce`); `channel_lost`, address marking, and the lost-address
   alert (§5.6); the receive column (NT-10); crash injection at every step and the journal-only
   oracle of NT-8; the process, its Postgres journal, its OS random source, and its channel
   adapters. Until the retry slice, an attempt that failed is not attempted again, which loses no
   record: the attempt and its reason are journaled, and the pull channels list every notice.

### Rationale

DEC-701 item 3 asks for a writer that accepts only an `ntf:` stream id and leaves its shape to
E8-10's tests. Taking the writer, not a stream id, in the only append the dispatcher has makes the
rule hold at rung 1 in the dispatcher's own code, while the journal crate keeps its general
`append`. Every other item restates spec §5.1's order (cause committed, `NoticeIssued` before the
first send, each outcome journaled) or picks the reading that sends nothing more than the spec
allows: no notice without a committed cause, no send after a refused append, no recipient added
after issue. The slice is the smallest one every later slice builds on: retries, quiet hours,
coalescing, approvals, and crash injection all start from a committed `NoticeIssued` and a
journaled attempt.

### Alternatives considered

- **A writer method on `mandate-journal`.** Rejected: the journal crate also builds control-stream
  drafts (DEC-701 item 3), and a restriction there would not stop the dispatcher calling the
  general `append`.
- **One batch for a notice's `NoticeIssued` and its attempts.** Rejected: an attempt's outcome is
  known only after the send, and spec §5.1 journals `NoticeIssued` before the first send.
- **Map a `retryable` result to no record until it is retried.** Rejected: NT-8 journals every
  attempt's outcome, and a crash would then lose the evidence of the attempt.
- **Make the process impure in this slice.** Rejected: the pure step is what every later slice
  tests; the process, its storage, and its random source are a separate, smaller change.

<a id="dec-705"></a>

## DEC-705: When alerts share a notice key, a control-stream alert is the cause, whatever the subject streams' order

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L3 builder agent (Claude Code) under DEC-79 and DEC-176. This reading only tightens DEC-704 toward spec §3.4: it changes which alert is named as the cause, and it sends nothing more and drops nothing |
| **Story or claim** | E8-10 slice D2, claim [#753](https://github.com/kunwarshivam/mandate/issues/753), PR [#1241](https://github.com/kunwarshivam/mandate/pull/1241); [notifications spec](../../specs/notifications.md) §3.4 and §7; [journal spec](../../specs/journal.md) §9.15 rule 124; amends [DEC-704](earlier-600-899.md#dec-704) item 2 |

### Decision

1. **Amends DEC-704 item 2.** One `step` may see several committed `OwnerAlertSent` events that
   share one notice key: their `owner_command` when they name one, their own event id otherwise
   (DEC-725 item 7). When it does, the new `NoticeIssued` names as its `cause` (and `cause_stream`)
   the alert on a control stream, if there is one (spec §3.4: the cause of a user's kill switch is
   the alert the API writes beside the command's `OwnerCommandIssued` on the control stream). This
   holds in whatever order the subject streams are given. Only when no alert in the group is on a
   control stream is the cause the first alert in DEC-704's reading order: subject streams in the
   order given, each in `seq` order.
2. **Nothing else in DEC-704 item 2 changes.** There is still one notice per key. A key that already
   has a `NoticeIssued` is still skipped, whichever alert that notice names. The new notice's place
   in the step's batch is still where its key was first read.

### Rationale

DEC-704 item 2 reads the subject streams "in the order given". With the control stream given first,
that agrees with spec §3.4. With an account or agent stream given first, it would name a stream
owner's alert as the kill switch's cause, which contradicts §3.4 and the §7 failure walk ("One
`NoticeIssued`, cause the `OwnerCommandIssued`", reached through the alert as journal spec rule 124
requires). A notice's cause should not depend on configuration order. This reading names the
§3.4 cause every time. Recipients, sends, and records are the same under either reading, so it adds
no risk and only tightens the spec's reading. The pending test
`the_control_streams_alert_is_the_kill_switch_cause_whatever_the_subject_order` pins it with the
subject streams in the order `[A1, A2, CTL]`.

### Alternatives considered

- **Keep "in the order given", and require the configuration to list the control stream first.**
  Rejected: the rule would then live in deployment configuration, which no test of the step can
  hold, and a misordered list would silently name the wrong cause.
- **Always sort the subject streams so that the control stream comes first.** Rejected: it changes
  the reading order of every cause, not only the tie inside one key, and §3.4 asks only for the
  cause.

<a id="dec-710"></a>

## DEC-710: The `mandate-notify` crate and its tightening readings of the notice payload

| | |
|---|---|
| **Status** | Accepted; item 8 amended by [DEC-712](earlier-600-899.md#dec-712) |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L3 builder agent under DEC-79 (a reversible engineering decision) and DEC-176 (items 3 to 5 only tighten what the notifications spec allows to leave the workspace) |
| **Story or claim** | E8-9 part a (slice S1 of the L3 lane, in two parts: S1a the notice, S1b the provider interface and the canary scan; claim [#753](https://github.com/kunwarshivam/mandate/issues/753)); [notifications spec](../../specs/notifications.md) §3, §4.2, §4.3, §5.2; DEC-438 items 1, 2, 10 |

### Decision

1. **A new crate, `mandate-notify`, at layer 1, pure and safety-critical,** over `mandate-canon`
   alone (lane layout per claim #753). It holds the closed notice: `NoticeId`, `TextKey`,
   `NoticeKind` and `Class`, `Notification`, the payload, the origin and the link, the provider
   interface of spec §5.2 with a fixture provider, and the captured-payload canary scan. Below the
   journal, it cannot reach the control-stream writer (NT-3 at rung 1). `mandate-approval`'s
   `Notification` and `notification_payload` stay as they are until a later E8-9 slice moves their
   callers onto this crate.
2. **A notice id is minted only from a `SecureRandom` source passed in,** whose failure mints
   nothing (`EntropyUnavailable`), as DEC-701 and DEC-702 lay out. It has no constructor from an
   event id; text parses only as exactly 32 lowercase hex digits (`NoticeId::parse`, to resolve a
   link or replay the dispatcher's own `NoticeIssued`, DEC-702 item 2), which no ULID is (NT-1,
   NT-4). `NoticeId::hex` writes it back in that form. The crate draws no randomness itself, so it
   stays pure; the dispatcher (E8-10) passes the operating system's source.

   *The rung-1 residual.* `parse` lets any caller turn 32 hex digits into a `NoticeId`, so code
   could hex-encode an event's 128 bits and parse them. That buys nothing: `parse` only resolves an
   id that was already minted and journaled. A parsed id that matches no `NoticeIssued` resolves as
   a missing notice (NT-10), and the dispatcher issues a new notice only through `mint` (its tests
   pin a minted id). There is no `Display`: a formatter cannot be a DEC-77 stub, so `hex` is the
   accessor, and every write of an id goes through one fallible call.
3. **The origin is `https://<lowercase host>[:<port>]` and nothing else:** the host is one or
   more dot-separated labels, each non-empty, of `[a-z0-9-]`, neither starting nor ending with `-`
   (so no empty label, trailing dot, or IP literal in brackets such as `https://[::1]`); a port is
   1 to 65535 in decimal without a leading zero or sign; and there is no path, trailing slash,
   query, fragment, credentials, whitespace, or upper case. Spec §4.3 asks for a fixed origin with
   no token, query, or tracking; this reading also refuses plain `http`, any path, and IPv6
   literals, so a misconfigured origin fails at load rather than leaking through a link. A hybrid
   or on-prem origin reachable only on the customer's network still fits it by host name or IPv4.
4. **A recipient is journaled and keyed as an opaque id** of 1 to 64 characters of `[a-z0-9_]`,
   starting with a letter (NT-2). An address handle is that id and a channel; the adapter
   dereferences it through the vault.
5. **A provider sees only a digest of the idempotency key:** the lowercase hex SHA-256 of the
   canonical `{"channel", "notice", "recipient"}`. Spec §5.1 keys a send by `(notice id, recipient,
   channel)` and gives the key to a provider that supports one; sending the triple itself would
   hand every provider a stable recipient id that links notices over time (spec §12 item 4). The
   digest is as unique and as stable across retries, and reveals nothing.
6. **The rendered message of this slice is the text, a newline, and the link.** Channel adapters
   (E8-11, E8-12, E8-14) add only fixed copy around it, such as the email's sentence and footer
   placeholder (spec §4.4).
7. **The canary scan ships in the crate** (`mandate_notify::canary`) so every channel slice reuses
   one oracle: fourteen canaries, one per kind of data NT-1 names, each lowercase, distinct, and
   holding `z` or `q` so no hex id, digest, or fixed text can contain one; the scan ignores ASCII
   case. It has its own tests, which plant a canary and show the scan finds it.
8. **The provider's `Reason` is spec §5.2's closed enum** as notifications spec v0.2 (#763)
   writes it: `timeout`, `rate_limited`, `provider_error`, `address_rejected`, `auth_failed`,
   `too_large`, `recipient_not_permitted`, `bounced`, `complained`, `unsubscribed`. The
   dispatcher's own `not_pending` and `retry_window_ended` (§5.3, §5.5) are not a provider's to
   return, so they are not in it.

### Rationale

The spec fixes what may leave the workspace; these readings only remove ways a mistake could add to
it. Each is held by the type (items 1, 2) or by a test in the crate: item 3 by S1a's tests, items 4 to 7
by S1b's.

### Alternatives considered

- **Put the notice in `mandate-approval`.** Rejected: that crate is the approval core, and alerts,
  account notices, and the brief are not approvals; a separate crate also lets the channel crates
  depend on the notice without the approval rules.
- **Send `(notice id, recipient, channel)` as the provider's key.** Rejected for item 5's reason.
- **Allow `http` origins for on-prem.** Rejected: an origin is fixed configuration, and plain HTTP
  would let a network observer read the link and the sign-in that follows. A customer who needs it
  can ask for a new decision.

<a id="dec-712"></a>

## DEC-712: The provider's `Reason` includes `address_missing`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the L3 lane lead) under DEC-176: it follows the newer spec text and adds no risk. Amends [DEC-710](earlier-600-899.md#dec-710) item 8 |
| **Story or claim** | E8-9 part a, slice S1b (the L3 lane, claim [#753](https://github.com/kunwarshivam/mandate/issues/753)); [notifications spec](../../specs/notifications.md) §5.1, §5.2, §5.6; [DEC-795](DEC-795.md) item 7 |

### Decision

DEC-710 item 8's provider `Reason` is spec §5.2's closed enum as it now reads (DEC-795 item 7):
`timeout`, `rate_limited`, `provider_error`, `address_rejected`, `auth_failed`, `too_large`,
`recipient_not_permitted`, `address_missing`, `bounced`, `complained`, `unsubscribed`. The
dispatcher's `not_pending` and `retry_window_ended` (§5.3) stay out, as DEC-710 item 8 says.

`address_missing` is `permanent` (§5.2): the fixture provider's tests pin that a handle with no
vault entry answers `permanent { address_missing }` and sends nothing.

The provider's `send` takes the origin, the closed `Notification`, the address handle, and the
idempotency key, and renders the message itself (DEC-710 item 6), where §5.2's table lists "the
rendered template for the channel". This is a tightening: no string from the caller reaches an
adapter (§5.2: adapters "take no string from the caller except the address handle"), and a channel
adapter still adds its fixed copy (§4.4) around `rendered`.

### Rationale

- §5.2's adapters take the address handle and dereference it through the vault client, so a
  missing entry shows in the adapter, which is platform code: as §5.2 says, it is the platform's
  finding, not a provider's verdict.
- Without `address_missing`, an adapter could answer only `address_rejected`, which marks the
  address `unreachable` (DEC-795 item 7 forbids that for a missing entry), or `provider_error`,
  which retries a send that can never succeed.
- `recipient_not_permitted`, the platform's own refusal, already sits in the same enum.
- It follows the newer spec text and adds no risk: it names a failure otherwise mislabelled.

### Alternatives considered

- **The dispatcher checks the vault before calling the adapter.** Rejected: §5.2 keeps the
  dereference in the adapter, and a dispatcher that reads raw addresses widens the surface
  `AGENTS.md` rule 7 guards.

<a id="dec-713"></a>

## DEC-713: `PushPlaintext::of(&Notification)` is a plaintext's one public source, so `mandate-webpush` sits above `mandate-notify`

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79's after-the-fact review). It edits `xtask/layers.toml`, a founder-owned file (CODEOWNERS), so the founder reviews it after the fact and can revert it, as with DEC-785 |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L3 builder agent (Claude Code) under DEC-79 (a reversible engineering decision) and DEC-176 (it keeps DEC-790 item 7's closed source on the type rung, adds no path to the journal, the control stream, or the order path, and loosens no invariant or non-negotiable) |
| **Story or claim** | E8-14, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); #1180's "Not done"; [notifications spec](../../specs/notifications.md) §4.2, §4.6, NT-1, NT-3, NT-4; DEC-710 item 2, DEC-790 items 1 and 7 |

### Context

DEC-790 item 7 makes `mandate_webpush::PushPlaintext` closed: its constructor, which takes 16 raw
bytes and a text key, is private to its crate, and S1's `mandate_notify::Notification` is to become
its only public source. Item 1 puts `mandate-webpush` at layer 0 with no workspace dependency. Now
that `mandate-notify` (layer 1) has merged, the two items cannot both hold: a constructor that takes
a `Notification` must name it, and a layer-0 crate cannot depend on a layer-1 one.

### Decision

1. **The bridge is `PushPlaintext::of(&Notification) -> Result<PushPlaintext, WebPushError>`, in
   `mandate-webpush`.** It writes exactly `{"notice":"<the id's 32 lowercase hex
   digits>","text":"<the text key>"}` (spec §4.2) and is the only public way to build a plaintext.
   The byte-level `PushPlaintext::new` stays private to the crate (DEC-790 item 7).
2. **`mandate-webpush` depends on `mandate-notify` and moves from layer 0 to layer 2,** one above
   it. Its `forbidden_internal` list is unchanged, and `mandate-notify` is pure, at layer 1, over
   `mandate-canon` and `mandate-time` only, so the push path still cannot reach the journal or the
   control stream (NT-3). DEC-790 item 1's "depends on no workspace crate" now reads "depends on no
   workspace crate but `mandate-notify`".
3. **Its dependents move up by one so the layering stays strict:** `mandate-push-relay` from 2 to
   3, and `mandate-dispatcher` from 3 to 4. Their flags and `forbidden_internal` lists are
   unchanged, and no other crate depends on either.
4. **`WebPushError` gains `InvalidNotice`** (`invalid_notice`), the bridge's only refusal: a
   notice id that cannot be written as its 32 digits, which no minted id is. It carries no input.

### Rationale

DEC-790 item 7's point is that nothing outside `mandate-webpush` can put domain text, or bytes that
were not minted, into a push. A notice id is 128 bits, as a journal event's ULID is, so a public
constructor from 16 bytes, or from 32 hex digits, would let a caller send an event id, whose
leading 48 bits are its creation time (NT-4, spec §4.2). `NoticeId` has no such constructor and is
minted only from a secure random source (DEC-710 item 2), and `Notification` holds a `NoticeId` and a
closed `TextKey` and nothing else. Taking a `Notification` keeps the rule on the type rung. Rust
cannot seal a constructor to one other crate, so the only placement that keeps it there is in
`mandate-webpush` itself, which then has to see `mandate-notify`.

Moving crates up a layer only admits dependencies; it removes none and adds no edge toward the
journal, the runtime, the executor, or a connector. Moving them back is a change to
`xtask/layers.toml` and one `Cargo.toml` line.

### Alternatives considered

- **A function in `mandate-dispatcher`.** Rejected: it would need a public byte- or hex-level
  constructor in `mandate-webpush`, which any of its dependents could fill with an event id's bits.
- **A public constructor from `[u8; 16]` or 32 hex digits, gated on the closed `PushText`.**
  Rejected for the same reason: the text key is closed but the id is not, so it would loosen
  DEC-790 item 7 and NT-4 from rung 1 to review.
- **The bridge in `mandate-notify`, over `mandate-webpush` at layer 0.** Rejected: `mandate-notify`
  could call the constructor only if it were public to every crate.
- **A new layer-0 crate holding only `PushPlaintext`, with `allowed_dependents` naming
  `mandate-notify` and `mandate-webpush`.** Rejected for now: it holds the rule at rung 2 rather
  than rung 1, and adds a crate, a CODEOWNERS line, and a feature for one constructor.
- **Replacing `PushText` with `mandate_notify::TextKey`.** Deferred: the bridge maps one to the other
  with an exhaustive `match`, and removing `PushText` is a refactor, not part of this decision.

<a id="dec-722"></a>

## DEC-722: A push allowlist entry names at least two labels, and no host label has an edge hyphen

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the L3 lane lead) under DEC-176: it only refuses more of what DEC-792 accepts, spends nothing, and adds no risk. The coordinator asked for it on 2026-10-09, after #896's review found that one deployment typo opens the sender to every host under a top-level domain |
| **Story or claim** | E8-14, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); tightens [DEC-792](DEC-792.md) items 1 and 2 as [#896](https://github.com/kunwarshivam/mandate/pull/896) implemented them; [notifications spec](../../specs/notifications.md) §4.6 |

### Decision

1. **An entry names at least two labels.** An exact entry is a host of at least two labels, and a
   wildcard entry is `*.` and a domain of at least two labels. `com`, `a`, `*.com`, and `*.a` are
   refused, with the rest of DEC-792 item 2's malformed entries, and refuse the whole list. The
   default list is unchanged and stays valid: `fcm.googleapis.com`,
   `updates.push.services.mozilla.com`, `*.push.apple.com`, and `*.notify.windows.com`.
2. **No label starts or ends with a hyphen** (RFC 1123's letter-digit-hyphen rule), in an entry or
   in an endpoint's host. An entry such as `-a.com`, `a-.com`, `*.-a.com`, or `*.a-.com` is refused.
   An endpoint such as `https://-a.push.apple.com/` or `https://a-.push.apple.com/` is refused with
   `invalid_endpoint` although `*.push.apple.com` would match it. A hyphen inside a label stays
   allowed (`a--b.com`, `wns2-bl2p`), and an `xn--` label stays refused (DEC-792 item 1).
3. **No public-suffix list.** A wildcard over a two-label public suffix, such as `*.co.uk`, passes
   item 1. It remains a configuration risk, which DEC-792 item 2 already meets: the default list is
   fixed in code, a deployment may only shorten it, and adding an entry is a reviewed configuration
   change.
4. **Where it applies.** Both rules sit in `mandate_webpush`'s one host check, which
   `PushAllowlist::parse` and `PushEndpoint::parse_allowed` share (DEC-792 item 3). The web
   client's mirror of the table ([#833](https://github.com/kunwarshivam/mandate/pull/833)) applies
   item 2 to an endpoint's host when it next changes; its list is the fixed default, so item 1 does
   not reach it.

Delivered as DEC-77 says: a tests PR with pending tests, then the implementation. The parser is
already live, so there is no stub for the pending tests to stop at; each is named in
`xtask/behaviour-only` until the implementation deletes its row with its `#[ignore]` line.

### Rationale

The allowlist is the one thing that keeps the push sender and the relay from being open forwarders
(DEC-792's rationale). As #896 shipped it, a deployment that writes `*.com` for `*.push.apple.com`,
or drops a label by mistake, allows every host under a top-level domain, and nothing refuses it. No
push service is a bare top-level domain or a single label, so item 1 refuses only typos. An edge
hyphen is not a valid host name label (RFC 1123 §2.1, RFC 952), so no real push service uses one,
and item 2 refuses only addresses a browser would never hand over.

### Alternatives considered

- **A public-suffix list** (refuse `*.co.uk` and the like). Rejected: it is a new dependency and
  data that must be kept current, for a risk the reviewed, fixed-in-code default already contains.
- **Leave DEC-792 as it is.** Rejected: one typo in a deployment's configuration opens the sender to
  any host under a top-level domain.

<a id="dec-738"></a>

## DEC-738: A sparkline drops its dashed limit rule when the limit is below the line's range (amends DEC-515 item 2)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | the agent (Claude Code, lane L4 lead), under DEC-79 (reversible product decisions) and DEC-176; routed by the merge coordinator |
| **Story or claim** | E11-9, claim [#746](https://github.com/kunwarshivam/mandate/issues/746); plan J item C-11 in `web/design/plan.md`, finding C-11 of `web/design/critique-2026-10-09.md`; PR #1093 |

### Decision

This amends [DEC-515](earlier-303-599.md#dec-515) item 2, "the limit is the pale region below a dashed line", for the
two cases where the daily loss limit lies outside the line's own range. DEC-515 item 2 otherwise
stands: the line scales to its own range, and the limit is a pale band.

1. **Limit below the line's range** (the limit is strictly below the lowest plotted value): the
   pale band stays, as the sliver at the foot that DEC-515 item 2 describes, and the dashed rule is
   left out. A rule drawn under the lowest point read as the agent touching its limit when it was
   far from it (C-11).
2. **Limit inside the range, at its minimum or at its maximum**: as DEC-515 item 2, the band with
   the dashed rule on its top edge.
3. **Limit above the line's range** (the whole plotted day is already past the limit): today's
   behaviour holds. The band fills the chart and the dashed rule sits on its top edge. A test pins
   this case so a later change cannot alter it silently.

### Rationale

DEC-515 item 2 is Accepted, so narrowing it needs a decision of its own (`AGENTS.md` rule 9). The
critique's "DEC: no" does not override a binding decision.

The band, not the rule, is what tells the owner that a limit applies. It stays in every case, so
nothing that marks the limit is lost. Only the rule's false reading of proximity goes.

The above-range case says something true: the line is entirely past the limit. It is not the false
proximity C-11 describes, so it keeps its drawing.

### Alternatives considered

- **Keep the rule and move it to the foot.** It still reads as "the line touches the limit" at the
  foot, which is the defect.
- **Drop the band too below the range.** That hides the fact that a limit applies at all, which
  DEC-515 item 2 deliberately kept as the sliver.
- **Change the above-range drawing in the same step.** No finding asks for it, and it is a true
  reading; left for a finding that does.

<a id="dec-740"></a>

## DEC-740: The workspace API's common value types (`schemas/workspace-api/common.schema.json`)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L4 builder, under DEC-79 and DEC-176: a reversible engineering reading. It writes down value forms the workspace API and journal specs already fix, adds no trading rule, and only narrows what a response may carry |
| **Story or claim** | E11-9 and E10-10 (the read models and the API's response contract); claim #746; lane L4 slice S0a, part 1. The envelope and problem types are lane L2's `envelope.schema.json` |

### Decision

1. **One file of shared value types.** `schemas/workspace-api/common.schema.json` is a JSON Schema
   2020-12 document of `$defs` only. Every later workspace API schema references these names:
   the read models (L4), the envelope and commands (L2), and audit (L5). It defines no envelope,
   problem, actor, step-up, or record type; those belong to `envelope.schema.json`.
   `schemas/workspace-api/examples/common.json` gives valid instances of each definition.
2. **`Decimal`** is journal spec §4.6's canonical decimal string. Its pattern refuses a JSON number,
   an exponent, a sign on zero, a leading zero, a trailing fractional zero, and more than 28
   fractional digits. The magnitude bound (below 7.9 × 10²⁸) is the parser's, since a pattern
   cannot hold it exactly.
3. **`Timestamp`** is journal spec §4.7: UTC, `Z`, nine fractional digits, years 1970 to 9999,
   hours 00 to 23. The pattern also refuses a day past its month's length: day 31 only in the
   31-day months, and February at most 29. Whether February 29 falls in a leap year is left to the
   parser, which refuses it (item 11).
4. **`ContentRef`** is `sha256:` plus 64 lowercase hex (§3.1).
5. **`Id`** is an opaque id over journal spec §2's identifier grammar, at most 64 characters (the mandate schema's `id`).
   **`EventId`** is a ULID in Crockford base32 (journal spec §3), with a first character of 0 to 7
   so the value fits 128 bits.
   **`AssetId`** is an instrument's asset id: the mandate schema's lowercase `uuid` (mandate spec
   §3, journal spec §9.3's `asset_id`), so commands and read models name instruments one way.
6. **`Watermark`** is `{stream_id, seq, hash, recorded_at}` (§6.1):
   - `stream_id` takes one of journal spec §2's five stream forms with their segment counts:
     `acct:{workspace_id}:{account_ref}` and `agent:{workspace_id}:{agent_id}` have two segments,
     while `ctl:`, `clock:`, and `ntf:` take the workspace id alone. Every segment is
     `[A-Za-z0-9_-]+`;
   - `seq` is an integer of 1 or more;
   - `hash` is the event's hash written as a `sha256:` ref, as §3.1 writes every hash the API
     returns. The journal stores, and API-15's pages serve, the bare 64 hex digits as `hash` and
     `prev_hash`; the API adds the prefix in watermarks;
   - **`AsOf`** is one or more watermarks.
7. **`Freshness`** is per-value staleness, `{observed_at, stale, age_seconds, limit_source}`:
   - `age_seconds` is whole seconds from `observed_at` to when the server built the response;
   - `stale` is the server's judgment against the freshness limit `limit_source` names (§6.1);
   - a stale value is still returned, marked (API-14).

   A client showing a cached body adds the time since it fetched it.
8. **`QuotedContent`** is API-18's quoted, attributed model output:
   `{author, text, model_id, model_version, produced_at, event_id, artifact}`, closed to other
   members, so neither an action nor a control can ride in it.

   **`author`** is a closed enum of the authorship labels the specs define:
   - `owner_selected`: journal spec §9's `ApprovalRequested` evidence label "Output of software
     you selected", and mandate spec §6.4;
   - `platform_authored`: the same label's other value, mandate spec §6.4 and §8.1, and workspace
     API §4.7's "platform-authored quoted content".

   Mandate spec §8.1's "user-authored later" is not in v1. Since `author` is a closed safety enum,
   adding it is a new major version (§3.2).
9. **Conventions** for L2's Rust drift check: closed enums as `enum`, `additionalProperties: false`
   on every object, a nullable member as `"type": [..., "null"]`, and no floats.
10. **One example checker for every workspace API schema:** `schemas/workspace-api/check_examples.py`,
    run as `python3 -I schemas/workspace-api/check_examples.py` in the reference environment. It
    uses the jsonschema 4.26.0 already pinned in `reference/mandate/requirements.txt`.
    - It checks each schema against the 2020-12 metaschema and validates each
      `examples/<name>.json`.
    - Every case in `examples/<name>.valid.json` must be accepted and every case in
      `examples/<name>.invalid.json` refused.
    - For `common.schema.json`, the cases pin:
      - every `required` member, with a case that removes it;
      - every member's type and definition, with a value of another type and a value of a
        neighbouring definition (an `Id` where an `EventId` belongs, a bare time where a
        `Timestamp` belongs);
      - each object's closure;
      - each numeric bound, with a value one past it;
      - each length or repetition bound in a pattern, one past each end;
      - each anchor, with a trailing newline;
      - each date and time field's range;
      - the character exclusions the specs name: uppercase and non-hex digits in hashes and
        asset ids (per group), Crockford's I, L, O, and U and a first character above 7 in event
        ids, a dot or colon in an `Id`, an unknown stream type and the wrong segment count,
        and a leading digit, dot, or uppercase letter in a model id.

      A mutation sweep over the schema (370 loosening mutations, none equivalent) is caught on
      every `required`, type, closure, `const`, numeric-bound, and quantifier mutation. The 209
      that survive are of two kinds the cases do not attempt:
      - a character class widened by one particular foreign character at one position, for
        example `[0-9]` gaining `:` in a Timestamp's minutes;
      - an enum gaining an arbitrary string. Each enum is tested with real-looking foreign
        labels (`user_authored`, `platform`, `owner`), not every string.

      Item 11 lists the refusals the parser holds.
    - `pattern` is matched with ECMA-262's `$` (only at the end), as JSON Schema and Rust's
      `regex` read it. Python's `$` also matches before a final newline, so the checker rewrites
      it.
    - The read-model and envelope lanes add their examples beside these and use the same script.
      Wiring it into `cargo xtask ci full` is a separate change, since xtask is code (ES-22).
11. **Refusals the parser holds, not the schema.** These are looser in the schema than in the
    spec, and the parser refuses them (journal spec §4.6, §4.7):
    - a `Decimal` whose magnitude is 7.9 × 10²⁸ or more;
    - a `Timestamp` on February 29 of a year that is not a leap year.

    A regular expression cannot hold either exactly.

### Rationale

The read models, the envelope, and the audit routes are written by three lanes, so the value types
they share have to be fixed once, first, and by name. Each definition restates a form the journal
and workspace API specs already require, so writing it down is a reversible engineering reading
(DEC-176). Closing `QuotedContent` and its `author` enum makes API-18 checkable by a schema test
rather than by review. A server-computed `age_seconds` beside `observed_at` lets a screen show the
age without a clock of its own, and still lets a client that caches the body correct it.

### Alternatives considered

- **No `age_seconds`; the client derives the age from `observed_at` alone.** It needs no server
  clock in the body, but every client has to trust its own clock, and the lead's contract asks for
  the server's figure. Both are kept: the server's age and the observation time it came from.
- **Watermark `hash` as bare hex,** as the journal stores it. Rejected: §3.1 says the API writes
  hashes as `sha256:` refs, and one form across the API is easier to check.
- **The fixtures' author labels** (`owner_selected`, `platform`). `platform_authored` is the
  specs' wording ("platform-authored"); the web app maps its fixture value when it leaves its
  fixtures.
- **A free-text `author`.** Rejected: an open label could read as advice or as the owner's own
  words. A closed set keeps the label one of the two the specs define.

<a id="dec-750"></a>

## DEC-750: Pending tests in web/ are `it.skip("pending <story>: …")` against an `Unimplemented: <story>` stub, checked by `npm run pending`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, lane L4 builder), under DEC-176: it carries the Rust pending-test rule (`cargo xtask ci pending`, AGENTS.md "The trust ladder" rung 2) to `web/` and only tightens it. A reversible engineering decision |
| **Story or claim** | E11-9, slice S1-core of lane L4, claim [#746](https://github.com/kunwarshivam/mandate/issues/746) |

### Decision

1. **The form.** A web test written before its story is implemented is
   `it.skip("pending <story>: <what it proves>", …)` (or `test.skip`), one test per skip. The story
   is a backlog ID such as `E11-9`. Titles are unique within a file. Every other way of not running a
   test is refused, because nothing can prove it fails at a stub: `describe.skip`, `.skip.each`, a
   skip without that title, `skipIf`, `runIf`, `.todo`, `.fails`, `.only`, `xit`, `xtest`,
   `xdescribe`, and a computed member such as `it["skip"]`.
2. **The stub.** The code under test exists as a stub whose body throws
   `new Error("Unimplemented: <story>")`, so each pending test, un-skipped, fails at the stub and
   for no other reason.
3. **The check.** `npm run pending` (`web/scripts/pending.mjs`) fails when anything in
   `web/src/**/*.test.{ts,tsx}` breaks item 1, or when a pending test, un-skipped, passes or fails
   without its stub's `Unimplemented: <story>` report. It un-skips the tests in temporary
   copies of their files, runs them with Vitest, and deletes the copies.
4. **Two PRs per slice.** The tests PR adds the pending tests and the stubs. The implementation PR
   replaces the stubs and changes the tests only by deleting `.skip`, so the tests a reviewer
   approved are the tests that pass. One exception: a test that pins a directory's exact modules
   and exports (for `web/src/api/`, `client.test.ts`'s export list) is edited by the
   implementation PR when it adds or removes a module or an export, and that list is the only other
   test edit it may make.
5. **CI.** `.github/workflows/web.yml` runs `npm run pending` after `npm test`, from the
   implementation PR of E11-9's first slice (approved by the lane lead and the coordinator); until
   it lands, the check runs locally before every web push.

### Rationale

Agents copy what the repository already contains (AGENTS.md, "The trust ladder"). The Rust side
already proves that a pending test checks something: it must fail at its own stub. Without the same
check in `web/`, a skipped test could pass unnoticed, assert nothing, or fail for an unrelated
reason, and the implementation PR could quietly rewrite the tests it is meant to satisfy.

### Alternatives considered

- **`it.todo`.** Has no body, so it cannot be shown to fail at the stub; refused.
- **`it.fails`.** Runs on every `npm test` and passes while the stub throws anything, including an
  unrelated error; the un-skip check is stricter.
- **No convention, tests and code in one PR.** Loses the reviewed-tests-first property the Rust
  crates keep.

<a id="dec-770"></a>

## DEC-770: How `mandate-audit` reads what §4.8.1 leaves open: workspace ownership, never-written streams, and the stream-list cursor

| | |
|---|---|
| **Status** | Accepted; item 4 amended 2026-10-09 to take DEC-655's witness |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The lane L5 builder (`claude-code`) under DEC-79 and DEC-176: readings of workspace API §4.8.1 that only tighten it and add no risk; the founder may reverse them |
| **Story or claim** | E12-6, slice A1; claim [#756](https://github.com/kunwarshivam/mandate/issues/756) |

### Decision

The bounds, the cursor, the head read with the page, `at_head`, the one 404, and the stream list's
order and paging are [DEC-760](DEC-760.md)'s and workspace API §4.8.1's; `mandate-audit`
implements them as written (`limit` 1 to 1,000, default 100; `after_seq` 0 to 2^53 − 1; both
refused out of range, never clamped, and checked before any id is resolved). This decision records
only the readings §4.8.1 leaves open:

1. **Ownership is the whole segment.** A stream is in a workspace when its second `:` segment
   (journal spec §2) equals the workspace's segment (item 4) byte for byte. A prefix never
   matches: a workspace does not own `acct:{its segment}0:…`. An event is in a workspace when its
   stream is.
2. **A never-written stream is absent.** A stream whose writer took ownership but wrote no event is
   not listed (as §4.8.1 says) and its page is the same 404 as a foreign or absent stream, not an
   empty `200` at head 0: §4.8.1's empty page past the head is for a stream that exists in the
   workspace, and one with no event has no head to report.
3. **The stream-list `after` is a cursor, not an id.** It is compared by bytes and never resolved,
   so any string is accepted, including another workspace's stream id, and the list holds the
   caller's streams strictly after it. This reveals nothing: the response holds only the caller's
   own streams, whatever the cursor.
4. **The workspace comes only from a `ReadRecords` witness.** Each read, and `TraceRead::trace`,
   takes `&mandate_identity::demand::Permitted<'_, ReadRecords>` ([DEC-655](earlier-600-899.md#dec-655)), never a
   bare workspace id (identity spec ID-8, §4.5), so a malformed workspace id is unrepresentable.
   Only `TenantContext::require::<ReadRecords>()` yields the witness, and only for a context
   `authorize` built for `ReadRecords` (identity spec §4.2), so the permission is checked by type:
   a context authorized for another row is refused `forbidden` at `require` and reaches no read. The
   stream segment is the witness's `Tenant::workspace` ULID text (`WorkspaceId::to_ulid_text`;
   journal spec §2). This supersedes the interim reading of 2026-10-08, under which the reads took
   `&impl Tenant`, which exposes no permission, and relied on the API layer to authorize
   `ReadRecords` before it called; the merge coordinator ruled on 2026-10-09 that the reads take
   DEC-655's witness instead.
5. **Notice streams wait for the journal.** Journal spec §2 defines the notice stream
   `ntf:{workspace_id}`, and §4.8.1 lists `notice` among the stream types, so `StreamType::Notice`
   exists. But `mandate_journal::StreamId::parse` does not parse `ntf:` yet
   (`crates/mandate-journal/src/lib.rs`), so no store can hold a notice stream and no read returns
   one: an `ntf:` id reads as absent. The reads list and page notice streams like any other once
   the journal parses them (backlog E12-6, follow-up).

### Rationale

- A prefix or substring comparison is the one way a scope check written by hand reads across
  tenants; the test fixture holds a workspace's segment and that segment with one more character,
  so a regression fails.
- Answering 404 for a never-written stream keeps it identical to a foreign one (API-9); answering
  `200` with head 0 would let a caller tell "provisioned here" from "elsewhere".
- Resolving the stream-list cursor as an id would add a 404 path to a list call and leak nothing
  more than byte order, which the caller already knows.

### Alternatives considered

- **`200` with an empty page and head 0 for a never-written stream.** Rejected for the reason above.
- **404 for a stream-list `after` outside the workspace.** Rejected: it would make the cursor an id
  lookup, and a client paging its own list never sends one.

<a id="dec-772"></a>

## DEC-772: How the causal trace reads what §4.8.1 leaves open: hop statuses, check order, link names, multi-target rows, `as_of`, and quoted items

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L5 lane lead (`claude-code`) under DEC-79 and DEC-176: readings of workspace API §4.8.1 that only tighten it and add no risk; the founder may reverse them |
| **Story or claim** | E12-1 ([#745](https://github.com/kunwarshivam/mandate/issues/745)), E12-6 ([#756](https://github.com/kunwarshivam/mandate/issues/756)), slice A2; builds on [DEC-761](DEC-761.md) and [DEC-762](DEC-762.md) |

### Decision

The link table, the bounds (16 hops deep, 256 events, 1,024 hops), and the statuses are
§4.8.1's and DEC-761's and DEC-762's. `mandate-audit` reads what they leave open as follows.

1. **Statuses.** A hop's status is one of `already_shown`, `beyond_bound`, `not_recorded`,
   `unexpected_type` (with the found `event_type`), and `shown`, the last for a link followed to an
   event the trace did not yet hold. `to` is the target's `event_id` for `already_shown`,
   `unexpected_type`, and `shown`, and `null` for `not_recorded` and `beyond_bound`.
2. **Check order, per target.** `already_shown` when the target is a node; else `beyond_bound`
   when the from node is at depth 16 or the trace holds 256 events, which sets `truncated`; else
   `not_recorded`, `unexpected_type`, or `shown`. A link that closes a cycle at a bound is
   `already_shown`, and a bound never hides that a link returns into the trace.
3. **The hop bound truncates.** Once 1,024 hops are recorded, a further link is not recorded and
   sets `truncated: true`. A trace with exactly 1,024 hops and no further link is not truncated.
4. **Link names.** `link` is the member as §4.8.1's table writes it: `causation_id`,
   `payload.client_order_id`, `payload.intent_id`, `payload.approval`,
   `payload.content.evidence.outputs[].event_id`, `payload.outputs_used[]`, `payload.evidence[]`,
   `payload.thesis_id`. A list gives one hop per entry, a repeated entry included.
5. **Rows with several targets.** A member's targets are taken in the row's order, each named type's
   matches in `seq` order, on the stream the row names (built with the path's workspace). A target
   the row names with no match, singular ("the `IntentReceived`") or not ("every `GateDecided`"),
   is one `not_recorded` hop, as §4.8.1 reads any target that cannot be shown. An empty list and a
   `null` member name no target. An event found by its id on a stream other than the row names is
   `not_recorded`. The from event can be its own target (an `IntentReceived` matches its own
   `intent_id`), and reads `already_shown`.
6. **`as_of`** is one watermark per stream the walk read, in ascending `stream_id` bytes: each
   node's stream, and each stream a row names for a lookup (such as `agent:{ws}:{agent_id}`) that
   exists in the workspace, whether or not it held a match. A lookup by id alone that shows no
   node, and a link a bound kept from being looked up, add none.
7. **Quoted items.** A model-authored member that is absent or `null` holds no output and gives no
   item. Items follow §4.8.1's order per type (`ModelOutputRecorded`: `invalidation`, then
   `thesis_ref`). Their `author` is [DEC-773](DEC-773.md)'s (Proposed, for the founder).
8. **An `unexpected_type` target is a node** at the next depth, counted toward the 256, with no
   hops of its own.
9. **Malformed links.** The envelope and the closed payloads type every link member as a ULID, so
   the append protocol cannot record a malformed one; a malformed start id is the 404, and a
   malformed link value a store did hold reads `not_recorded`.

### Rationale

- Items 1 and 2 give every link exactly one outcome, and a `null` `to` for every target the walk
  did not show, so `beyond_bound` says no more about a target's existence than `not_recorded`.
- Item 3: a client must never take a cut trace for a whole one.
- Item 5 keeps a missing intent, gate decision, order, or approval visible as "not recorded"
  (backlog E12-5's wording): no target the table names is ever silent.
- Item 6 is §4.8.1's "per stream the walk read": a client checks each stream the walk relied on.

### Alternatives considered

- **`beyond_bound` before `already_shown`.** Rejected: a cycle at the depth bound would read as
  truncation of a trace that is whole.
- **Stop silently at 1,024 hops.** Rejected by item 3.
- **No hop for an "every" target with no match.** Rejected: an intent whose `GateDecided` is
  missing would read as a whole trace.

<a id="dec-775"></a>

## DEC-775: How the causal trace reads a version-1 submission, an `intended` protection, a mistyped by-id target, and a thesis

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L5 lane lead (`claude-code`) under DEC-79 and DEC-176: readings of workspace API §4.8.1 that keep every link visible and add no risk; the founder may reverse them |
| **Story or claim** | E12-1 ([#745](https://github.com/kunwarshivam/mandate/issues/745)), slice A2b ([#1028](https://github.com/kunwarshivam/mandate/pull/1028)); builds on [DEC-761](DEC-761.md) and [DEC-772](earlier-600-899.md#dec-772) |

### Decision

1. **A version-1 `OrderSubmitted`.** [DEC-761](DEC-761.md) item 3 holds: a version-1 submission
   has no link and reads `not_recorded`. Workspace API §4.8.1's version-1 row, "the walk ends here
   unless a `causation_id` is recorded", means that no further node follows from it, not that it
   records no hop. After its `causation_id`, when one is recorded, it gives exactly one hop
   `{link: "payload.intent_id", to: null, status: "not_recorded"}`: the intent that version 2 names
   through its companion is not recorded. A null `intent_id` on an `OrderRequestRecorded`, as on a
   protective order or a flatten, is still no link and gives no hop (DEC-761 item 3).
2. **An `intended` `ProtectionChanged`.** Journal spec rule 44 records its `intent_id` with a null
   `agent_id`. §4.8.1's intent row takes the `agent_id` from "the `ProtectionChanged`'s own", so its
   `IntentProposed` is one `not_recorded` hop, as DEC-772 item 5 reads any target that cannot be
   shown. The walk does not fall back to the `IntentReceived`'s `agent_id`. The `IntentReceived`, if
   shown, still finds the `IntentProposed` through its own row.
3. **A by-id target of the wrong type.** A row that names an event by its id and names that event's
   type (`payload.approval`'s `ApprovalRequested`, `payload.content.evidence.outputs[].event_id`'s and
   `payload.outputs_used[]`'s `ModelOutputRecorded`, and the `intent_id`'s `IntentProposed`) reads
   an event with that id, on the stream the row names, of another type as `unexpected_type` with the
   found `event_type`: it is shown and not followed (DEC-772 item 8). That is how A2 already reads
   the `IntentProposed` lookup. Such an event on another stream is still `not_recorded`
   (DEC-772 item 5).
4. **A thesis.** `ModelOutputRecorded`'s `payload.thesis_id` names one target, "the `ThesisProposed`
   or `ThesisRevised`" with that `thesis_id` on the output's agent stream. Its matches of either type
   are taken in `seq` order. With none, it is one `not_recorded` hop, not one per type.

### Rationale

- Item 1 follows the accepted decision and keeps the missing intent visible. A version-1
  submission that silently ended the trace would read as a whole chain, which is what DEC-772
  item 5 exists to prevent. Reading §4.8.1 as "no further node" makes the two texts agree, so
  neither needs to change.
- Item 2 adds no guess: an agent stream the record does not name is never read on its behalf. The
  chain stays reachable through the `IntentReceived`, so nothing is hidden.
- Item 3 tells "recorded, but not what the row expects" apart from "absent", as §4.8.1 does for
  typed causes. It shows the found type rather than hiding an event the workspace holds.
- Item 4 matches the row's own wording ("the … or …"). Two `not_recorded` hops would claim that two
  separate records are missing.

### Alternatives considered

- **Item 1, no hop for a version-1 submission (§4.8.1 read alone).** Refused: it contradicts
  DEC-761 item 3 and hides a missing intent. **A hop for a null `intent_id` too.** Refused:
  DEC-761 item 3 and DEC-772 item 5 make a null member no link.
- **Item 2, fall back to the `IntentReceived`'s `agent_id`.** Refused: the walk would build a stream
  id the from event does not record, and the result would depend on which record the walk met first.
- **Item 3, `not_recorded` for a mistyped by-id target.** Refused: it would make an event the
  workspace holds read as absent, unlike the A2 `intent_id` lookup.
- **Item 4, one `not_recorded` hop per type.** Refused: see the rationale.

<a id="dec-777"></a>

## DEC-777: How a timeline page merges, stops, and reports its cursors

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L5 lane lead (`claude-code`) under DEC-79 and DEC-176: readings that close an unclosed read-path contract by the reading that adds no risk; the founder may reverse them |
| **Story or claim** | E12-2 ([#747](https://github.com/kunwarshivam/mandate/issues/747)), slice T1; builds on [DEC-760](DEC-760.md) and [DEC-764](DEC-764.md); workspace API spec §4.8.1 "Timeline" |

### Decision

Workspace API §4.8.1 and DEC-764 fix the timeline's streams, its k-way merge, one cursor per
stream, filters that still consume the events they exclude, `limit`, and the 10,000-event cap. These
items pin what they leave open.

1. **The merge runs over the raw stream heads.** At each step the next event is the head with the
   least `(recorded_at, stream_id)` over every timeline stream's next unconsumed event, whether or
   not a filter excludes it and whether or not it is the agent's under DEC-764 item 2 (DEC-764
   item 3). Such a head takes its turn and is consumed (DEC-764 item 4), and it counts toward the
   per-stream cap of item 3 below. An account-stream mark is one example. So the order in which
   served events appear never depends on the filters, even when a host clock steps back.
2. **A page stops right after its `limit`-th served event.** It consumes no further event, not even
   an excluded one that follows, so every stream's cursor in `next` is just past the last event the
   page consumed.
3. **Reaching the cap stops the whole page.** When one stream has had 10,000 events consumed on
   this page, the page ends there for every stream, and `more` is set by item 4. The next page
   resumes the same merge from `next`, so the merge order across pages is the order of one
   unbounded walk.
4. **`more` is true** exactly when, in the snapshot the page was read from, some timeline stream
   still has an event past its `next` cursor.
5. **`next` names every timeline stream**, in ascending `stream_id`, as `<stream_id>:<seq>`. A
   stream the walk has not reached yet is named at `0`, so the cursor list alone describes the
   timeline.
6. **Membership lookups read the page's own streams and snapshot.** These are the lookups through
   which an account-stream event is the agent's: its `intent_id`'s `IntentReceived`, its
   `client_order_id`'s `OrderRequestRecorded`, and the event its `causation_id` names. They read
   only the timeline's streams, in the same snapshot as the page. A record appended after the
   snapshot never changes which events an earlier page served (AU-5).

### Rationale

Each item fixes one choice that the page's correctness tests (AU-5's merge-oracle fuzz, the limit,
the cap, and membership) need pinned to assert exactly. None of them changes which events exist or
what any record says. Each picks the reading under which a page is a deterministic prefix of one
unbounded merge, and under which no event the agent's history holds is hidden.

DEC-764 item 2's membership list stays as it is. An agent-scope `KillSwitchActivated` on an account
stream is not on that list. Adding it would amend DEC-764, so it waits for its own decision, made
once the journal closes that payload.

### Alternatives considered

- **Merging over filtered heads only.** Rejected: a filter would change the display order of the
  events it keeps when a clock steps back.
- **Consuming trailing excluded events after the `limit`-th served one.** Rejected: the cursors
  would depend on events the page did not show, for no gain in completeness.
- **A per-stream stop at the cap.** Rejected: the other streams would run ahead of the capped one,
  and the merge order across pages would differ from one walk.

<a id="dec-778"></a>

## DEC-778: How a timeline finds the `OrderRequestRecorded` behind a `client_order_id`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L5 lane lead (`claude-code`) under DEC-79 and DEC-176: closes an unclosed read-path contract by the only reading the records carry, which adds no risk; the founder may reverse it |
| **Story or claim** | E12-2 ([#747](https://github.com/kunwarshivam/mandate/issues/747)), slice T1c; reads [DEC-764](DEC-764.md) item 2 with [DEC-777](earlier-600-899.md#dec-777) item 6; journal spec §9.5 and rule 45 |

### Decision

1. **The link.** DEC-764 item 2 makes an account-stream event the agent's when "its
   `client_order_id` is an order whose `OrderRequestRecorded` names the agent". Journal spec §9.5
   does not give `OrderRequestRecorded` a `client_order_id`: that member is on the `OrderSubmitted`,
   and a version-2 `OrderSubmitted`'s `causation_id` names its `OrderRequestRecorded` (rule 45).
   So a `client_order_id` is the agent's exactly when some version-2 `OrderSubmitted` with that
   `client_order_id` has a `causation_id` naming an `OrderRequestRecorded` whose `agent_id` is the
   agent.
2. **Where it is found.** Both records are found as DEC-777 item 6 says: on the timeline's own
   streams, in the page's snapshot.
3. **Version 1.** A version-1 `OrderSubmitted` has no companion, so no record ties its order to
   an agent. Its `client_order_id` makes nothing the agent's, though the event can still be the
   agent's by another rule of DEC-764 item 2, such as its `intent_id`.

### Rationale

This is the only path the records carry from an order's broker name to the agent that requested
it. DEC-764 names the `OrderRequestRecorded` as the record that decides, and §9.5 and rule 45 give
the one way to reach it. Attributing a version-1 order without such a record would be a guess. The
timeline is a read view, so this changes nothing any record says or any order does.

### Alternatives considered

- **Reading the agent from the `OrderSubmitted`.** Rejected: it names no agent.
- **Following the order's `intent_id` instead.** Not needed for this rule. DEC-764 item 2 already
  has its own `intent_id` rule, and a record that carries an `intent_id` is judged by that rule as
  well.

<a id="dec-779"></a>

## DEC-779: The timeline leaves out DEC-764's account-wide clause until the journal can record its events

| | |
|---|---|
| **Status** | Accepted; lapsed for `AccountRestrictionChanged`, which the journal records since spec v0.88 ([DEC-2137](DEC-2137.md)), so the timeline serves it on the agent's accounts. It holds for the connection- and workspace-scope `KillSwitchActivated`, which the account stream still refuses (v0.90, [DEC-2139](DEC-2139.md)); the tripwire now offers only those |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The L5 lane lead (`claude-code`) under DEC-79 and DEC-176: a reading that changes nothing any recordable journal can show and adds no risk; the founder may reverse it |
| **Story or claim** | E12-2 ([#747](https://github.com/kunwarshivam/mandate/issues/747)), slice T1; reads [DEC-764](DEC-764.md) item 2; follows the reasoning of DEC-776 |

### Decision

1. **The clause waits.** DEC-764 item 2 makes `AccountRestrictionChanged`, and a connection- or
   workspace-scope `KillSwitchActivated`, on an account stream the agent's. The journal registers
   neither payload today: `append` refuses both with `UnknownSchema` at `payload`. So no journal
   holds such an event. The timeline's implementation leaves that clause out, and every
   recordable journal reads the same as it would with the clause in.
2. **Why no code.** A clause that no recordable event can reach has no test that fails without it.
   In `mandate-audit` its mutants would survive (six did, in the first T1 implementation), and
   exclusions are the founder's. DEC-776 settled the same question for the causal trace.
3. **A tripwire ends it.** The live test
   `the_account_wide_records_the_timeline_omits_are_not_yet_recordable` offers both records to
   `append`, including all three `KillSwitchActivated` scopes, each at schema version 1 and 2, and
   asserts the refusal. The change that registers either payload makes that test fail. That
   change's tests PR then adds the timeline cases, served and not served, and the clause follows
   them in DEC-77 order. This decision lapses then.
4. **Lapsed for the restriction, narrowed for the switch (2026-10-26).** The journal records
   `AccountRestrictionChanged` since spec v0.88 and the account stream's `KillSwitchActivated` at
   agent and account scope since v0.90, so the tripwire failed. The timeline now serves the
   restriction, and the tripwire, renamed
   `the_account_wide_kill_switches_the_timeline_omits_are_not_yet_recordable`, offers a switch
   that is well formed but for its scope: accepted at `account` scope, refused at `payload.scope`
   for `connection` and `workspace`. Whether the recorded agent- and account-scope switches are the
   agent's is backlog E12-2a.

### Rationale

Leaving the clause out hides no event that can exist, and the tripwire turns "remember to add it"
into a failing test the registering change cannot miss. The agent-scope kill switch is also covered
by the tripwire. Whether it is the agent's waits for its own decision (DEC-777, Rationale).

### Alternatives considered

- **Keeping the clause with no test.** Rejected: six surviving non-equivalent mutants in a
  safety-critical crate, and code no test pins.
- **A test-only raw insert, or a stub store, to hold an unregistered event.** Rejected, as in
  DEC-776: it widens the journal or audit read layer only to exercise a state `append` forbids.

<a id="dec-784"></a>

## DEC-784: A requested `stream_id` with a missing or empty workspace segment is refused, even for genesis

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, lane L5's builder) under DEC-176: the reading only tightens the rule, refusing a request the spec does not cover, and adds no risk |
| **Story or claim** | E12-3; claim [#748](https://github.com/kunwarshivam/mandate/issues/748) |

### Context

Journal spec §9.14 ("What a verifier reads") says a range is entered from genesis, seq 1 with 64
zeros, and that every start is looked up among the workspace's own control-stream records only, so
an absent record and one of another workspace give the same refusal (workspace API §4.8.1,
DEC-767). It is silent on a requested `stream_id` whose `{workspace_id}` segment is missing
(`acct`) or empty (`acct::A1`). Such an id names no workspace, so no record can be its own; for a
manifest or an anchor start that already means a refusal. For genesis no record is read, so the
spec admits either outcome.

### Decision

1. **`resolve_trusted_start` (`crates/mandate-journal/src/start.rs`) refuses any request whose
   `stream_id` has no `{workspace_id}` segment, or an empty one, before it reads a record, and the
   refusal is the same `Refused` as an absent or foreign record (DEC-767).** This holds for a
   genesis request at `from_seq` 1 too.
2. **The test row is in `an_unreadable_record_a_malformed_stream_or_another_type_is_refused`**
   (`crates/mandate-journal/tests/trusted_start.rs`): "no workspace segment" and "an empty
   workspace", each a genesis request at seq 1, and the empty workspace's segment and anchor rows.

### Rationale

A stream that belongs to no workspace has no range a verifier could check, and §9.14 ties every
start to the workspace's own records. Refusing it the same way as an absent record keeps one
refusal for every unusable start, so a malformed id tells a caller nothing more than a foreign one.
Refusing a verification adds no risk: no state changes, and the caller can retry with a well-formed
id.

### Alternatives considered

- **Answer genesis for any `stream_id` at seq 1.** Rejected: it would report a start for a stream
  that cannot exist, and treat genesis differently from the other two starts for the same malformed
  input.
- **A distinct error for a malformed id.** Rejected: DEC-767 gives absent and foreign the same
  refusal, and a separate one would add a surface the spec does not name.

<a id="dec-785"></a>

## DEC-785: `mandate-audit` moves from layer 3 to layer 4, so E12-3's verification run can use `mandate-journal-cold`

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79's after-the-fact review; coordinator-approved 2026-10-09). The change edits `xtask/layers.toml`, a founder-owned file (CODEOWNERS), so the founder reviews it after the fact and can revert it |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, lane L5's builder) under DEC-79: a reversible engineering decision that adds no dependency edge to any crate on the order path and weakens no safety rule |
| **Story or claim** | E12-3; claim [#748](https://github.com/kunwarshivam/mandate/issues/748) |

### Context

E12-3 runs chain verification for an auditor. That run belongs in `mandate-audit`, the
workspace-scoped, read-only audit reads over the journal (E12-6), which already enforces tenant
isolation. The run must apply the cold store's per-range checks: `verify_tsa` (the
`tsa_token_invalid` check) and the `SegmentManifest` checks, both in `mandate-journal-cold`.

`xtask/layers.toml` put both crates on layer 3. A crate may depend only on workspace crates in
strictly lower layers (`cargo xtask layers`), so `mandate-audit` could not depend on
`mandate-journal-cold`.

### Decision

1. **`mandate-audit` moves from layer 3 to layer 4** in `xtask/layers.toml`, one above
   `mandate-journal-cold` (layer 3). That is the smallest raise that admits the dependency. Its other
   flags are unchanged: `safety_critical = true`, `pure = true`, `allowed_external = ["thiserror"]`.
2. **The planned-layers comment in `layers.toml` lists `mandate-audit` on layer 4** beside
   `mandate-risk` and `mandate-api`, and names E12-3's verification run and this decision.
3. **This change adds no dependency.** The E12-3 PR that needs `mandate-journal-cold` adds the
   edge to `crates/mandate-audit/Cargo.toml`. `mandate-journal-cold` is pure and safety-critical,
   and its only external dependency is `thiserror`, so `mandate-audit` stays pure and its
   `allowed_external` list does not change.

### Rationale

All of `mandate-audit`'s dependencies stay strictly below its new layer: `mandate-canon` (0),
`mandate-time` (0), `mandate-identity` (1) and `mandate-journal` (2). Its dev-dependencies
(`mandate-identity-testkit`, layer 11, and `proptest`) are not bound by layer order.

Nothing depends on `mandate-audit`, so raising it cannot break any crate's dependency edge.
The evidence, on `origin/main` at `8f2bebef`:

- Of every `Cargo.toml` in the workspace, only two name `mandate-audit`: the root manifest's
  `[workspace.dependencies]` entry and the crate's own manifest. No member has it as a normal,
  dev or build dependency.
- No `.rs` file outside `crates/mandate-audit/` names `mandate_audit`.
- No `forbidden_internal` or `allowed_dependents` list in `xtask/layers.toml` names it.
- So no crate on layer 4 (`mandate-risk`, `mandate-api`) or below needs it. `mandate-api` holds
  L5's audit wire types without depending on `mandate-audit`, and the conversion into them still
  sits above both, as the layers comment said before (DEC-681).

The raise loosens nothing that guards the order path. Layer 4 is still below `mandate-builder`,
the executor, the runtime and every connector, and nothing on those layers depends on
`mandate-audit`. The crate stays pure and safety-critical, and it can still reach only what it
reached before, plus `mandate-journal-cold` once E12-3 adds the edge.

### Alternatives considered

- **Reimplement `verify_tsa` and the segment-manifest checks in `mandate-audit`.** Rejected: it
  duplicates safety-critical verification code. Two copies of the cold store's per-range checks
  could drift, so an auditor's run could pass a range that the cold store's own checks refuse.
- **Lower `mandate-journal-cold` to layer 2.** Rejected: it depends on `mandate-journal`, which is on
  layer 2, so layer 3 is the lowest layer it can have.
- **Put the verification run in a new crate on layer 4.** Rejected: it would need the same
  tenant-scoped reads `mandate-audit` already enforces, so it would either duplicate them or depend
  on `mandate-audit`. That adds a crate with no gain.

### Consequences

- E12-3 may add `mandate-journal-cold` to `mandate-audit`'s dependencies and build the
  verification run on `verify_tsa` and `SegmentManifest`.
- `mandate-risk`, `mandate-api` and anything on layers 3 and below can no longer depend on
  `mandate-audit`. None does today. A future crate that needs both the audit reads and the
  workspace API contract sits on layer 5 or above, as the conversion already had to.
- The founder reviews this change after it lands (DEC-79) and can revert it. Reverting moves the
  crate back to layer 3, and it would then have to drop its dependency on `mandate-journal-cold`.

<a id="dec-790"></a>

## DEC-790: Web push encryption and VAPID readings (E8-14, slice S8a)

| | |
|---|---|
| **Status** | Accepted; item 1's layer and "depends on no workspace crate" amended by [DEC-713](earlier-600-899.md#dec-713) |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L3 builder agent under DEC-79 (reversible engineering readings) and DEC-176 (each item only tightens a rule or sends less, and loosens no invariant or non-negotiable). The value of the VAPID contact (item 4) is left to the founder |
| **Story or claim** | E8-14, slice S8a, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [notifications spec](../../specs/notifications.md) §4.2, §4.6; DEC-438 items 1 and 13; DEC-700 item 3 |

### Decision

1. **A pure crate below everything.** `mandate-webpush` sits at layer 0, depends on no workspace
   crate, and its `forbidden_internal` list keeps the journal crates, the runtime, the executor,
   the shell, the connectors, and the CLI out of its graph for good, so no push path can write a
   control-stream event (NT-3). It holds no key, clock, or randomness: the application server key
   is behind `VapidSigner`, the ephemeral key and salt come from `SecureRandom` (drawn key first,
   then salt), and the time is an argument.
2. **Every body is the same size.** The plaintext is padded (RFC 8188 §2: the delimiter `0x02`,
   then zeros) to one record of 128 octets, so every web push body is 230 octets whatever the text
   key. Spec §4.2 lets a provider learn whether a notice is an approval, an alert, an account change,
   or the brief; with fixed-size bodies the push service and the relay do not learn even that. A
   230-octet body keeps every push under the relay's 512-octet cap with 282 octets to spare, and the
   cap is still checked before any request is built. This only sends less (DEC-176).
3. **One record, record size 4096**, as RFC 8291 §4 requires of web push; the `keyid` is the
   ephemeral public key. A key draw that is not a valid P-256 scalar is refused, never retried
   with a derived value.
4. **VAPID** (RFC 8292). `aud` is the push endpoint's origin, `https://<host>[:port]`. `exp` is 12
   hours after the time given, inside the RFC's 24. `sub` is one contact URI per deployment, from
   its configuration, the same on every notice; it must be a `mailto:` or `https:` URI with no
   space, quote, or backslash. The crate fixes no address. The founder chose the deployment value,
   `mailto:push@notify.owlhead.ai` (2026-10-08, DEC-79), and it is set in deployment configuration;
   tests use the placeholder `mailto:push@example.invalid`.
5. **The envelope** follows DEC-700 item 3: `action` `high` and 3 600 s, `safety` `high` and
   86 400 s, `info` `normal` and 21 600 s, from the class alone.
6. **The endpoint is an address** (NT-2). It must be `https` with a host and no user information,
   and its `Debug` prints no part of it. Its layout is `{url, origin_len}`; the live test
   `an_endpoint_never_prints_its_address` builds one and pins both.
7. **The plaintext is closed.** `PushPlaintext` is `{"notice":"<32 lowercase hex>","text":"<key>"}`
   over the four text keys of spec §4.2, and its constructor is private to the crate. Slice S1's
   `mandate_notify::Notification` becomes its only public source when it merges.

### Rationale

Items 1, 6 and 7 put NT-1, NT-2 and NT-3 on the type and crate rungs rather than in review. Item 2
removes the one fact a ciphertext's length would still give away. Items 3 to 5 are the RFCs'
requirements, with the stricter choice wherever they leave room.

### Alternatives considered

- **No padding.** Rejected: the four text keys differ in length, so the body's length would name
  the text key.
- **`exp` at 24 hours.** Rejected: a clock skew at the push service would push it over the limit,
  and a token that lives shorter can be replayed for less time.
- **A fixed `sub` constant in the crate.** Rejected: it would choose the platform's contact address,
  which is the founder's decision, and could not differ for a customer-run deployment.
- **`ring` for the cryptography.** Rejected: its ECDH takes no injected ephemeral key, so the RFC
  8291 vector could not be reproduced byte-exact.

<a id="dec-793"></a>

## DEC-793: The browser side of web push (E8-14, slice S8d)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The L3 builder agent under DEC-79 (reversible engineering readings) and DEC-176 (each item only shows, keeps, or sends less) |
| **Story or claim** | E8-14, slice S8d, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [notifications spec](../../specs/notifications.md) §4.2, §4.3, §4.6, NT-1, NT-2, NT-4; P5 in [09-product-experience](../../product/09-product-experience.md) |

### Decision

1. **The worker shows four sentences and nothing else.** `web/public/push-sw.js` accepts a push
   only when its text is exactly the canonical bytes `{"notice":"<32 lowercase hex>","text":"<key>"}`
   with one of the four keys of spec §4.2: a duplicate key, whitespace, a byte order mark, another
   member order, or anything longer is refused. It shows that key's fixed sentence under the
   title "Owlhead". Anything else is dropped and nothing is shown. The notice id is the
   notification's tag, so a re-sent notice replaces itself rather than stacking. A browser that
   requires every push to be visible may then show its own generic message; that text is the
   browser's and carries nothing of the push.
2. **A tap opens only `<this origin>/n/<notice>`** (spec §4.3, NT-4). The link is built in the
   worker from its own origin and the validated id, never taken from the payload.
3. **The worker keeps nothing** (P5). It has no fetch handler, so it serves nothing and caches
   nothing, and it uses no storage. Its scope is the whole app so the link it opens is inside it.
4. **The subscription is an address** (NT-2). The client checks the endpoint against the
   push-service allowlist (DEC-792 in [#827](https://github.com/kunwarshivam/mandate/pull/827), the same table the server parses, notifications spec §4.6 as
   [#827](https://github.com/kunwarshivam/mandate/pull/827) states it) and hands only
   `{endpoint, keys}` to an injected sender, never to a log, the console, storage, or the page. The
   route that receives it is specified in workspace API spec §4.10 (#827, DEC-795) and is not served
   yet; until it is, the UI uses a labelled fixture sender (DEC-200 item 6).
5. **Permission is asked only on a gesture**, and a dismissed prompt counts as a refusal.
6. **The worker script is public.** `/push-sw.js` is outside the proxy's matcher and on the
   signed-out public file list, so a browser's update check, which may carry no session, still
   reaches it. It serves only the worker's own code.

### Rationale

Each item puts a spec sentence on the browser side where the worker could otherwise break it: an
unexpected payload is the one way content could reach a lock screen, and a payload-chosen URL the
one way a tap could leave the app.

### Alternatives considered

- **Showing a generic sentence for an unrecognized payload.** Rejected: it would tell the person
  something happened that the platform did not send.
- **A worker built from TypeScript.** Rejected for now: it adds a build step for about fifty lines,
  and the test runs the shipped file itself.

### Tests first in `web/`

The tests PR marks each pending test `it.skip("pending E8-14: …")` against stubs that throw
`new Error("Unimplemented: E8-14")`, the convention DEC-750 sets for `web/`. `cargo xtask ci pending`
does not run Vitest; the rule that each pending test fails at its stub is automated for `web/` by
`npm run pending` (`web/scripts/pending.mjs`), which un-skips each pending test in a temporary copy and
checks that it fails with `Unimplemented: E8-14`.

<a id="dec-794"></a>

## DEC-794: CodeQL ignores only the web push crate's published RFC test vectors

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The founder, 2026-10-08, in the coordinator's session (DEC-79: narrowing a security scan is the founder's; DEC-500's configuration file), recorded as DEC-824; written here by the L3 builder agent |
| **Story or claim** | E8-14, slice S8a, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); CodeQL's two `rust/hard-coded-cryptographic-value` alerts on [#800](https://github.com/kunwarshivam/mandate/pull/800); precedent [DEC-500](earlier-303-599.md#dec-500) |

### Decision

1. **The vectors live in one test-only directory**, `crates/mandate-webpush/tests/vectors/`, which
   holds only `mod.rs`. It contains RFC 8291 §5 and Appendix A's keys, salt and message, RFC 8188
   §3.1's input keying material and message, and every fixture that carries them: the scripted
   random source, the test signer, and the receiver-side decrypt. They are published test vectors,
   used only by tests.
2. **CodeQL skips exactly that directory** (`paths-ignore` in `.github/codeql/codeql-config.yml`).
   Everything in it is skipped by every query. Every query, the hard-coded cryptographic value query
   included, still runs on every other path: the crate's `src/`, its tests module, and the rest of
   the repository. No alert is dismissed and no query is excluded.
3. **No product path may read them.** Only `src/tests.rs`, compiled under `#[cfg(test)]`, includes
   the directory. Its live test `no_product_code_reaches_the_rfc_vectors` reads every `.rs` file
   under `src/` except `tests.rs`, at any depth, and fails if one names the vectors. It also fails
   if the tests module stops being test-only, or if the directory holds anything but `mod.rs`.
   Cargo builds no target from a subdirectory of `tests/`.

### Rationale

The two alerts were false positives: the published RFC vectors, which byte-exact tests must use,
in a test-only module. The query is right that a fixed key in product code is a defect, so it keeps
running everywhere a product key could appear. The one place it skips is pinned by a test to stay
test-only and out of the product.

### Alternatives considered

- **Dismissing the alerts as "used in tests".** Not chosen by the founder. It is invisible in the
  code, and the next copy of the pattern would need its own dismissal.
- **Excluding the query.** Rejected: DEC-500 item 2 excludes a query only on its own evidence, and
  this one is right about product code.
- **Generating the keys at test time.** Rejected: the RFC vectors are reproducible only with the
  RFC's own keys.

<a id="dec-802"></a>

## DEC-802: `mandate connection record` plays the connection manager's part, and refuses what it cannot tell apart

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 |
| **Decided by** | Lane L2 builder (`claude-code`), under DEC-79 and DEC-176: every item refuses a record or binds one to evidence. None applies to an exit, a protective order, or a kill switch |
| **Story or claim** | E7-11's first slice (the first live trade brief's K1b), claim #764 |

DEC-529 item 3 accepts a `live` mandate only on a Robinhood connection the founder recorded
through the CLI, "the first slice of E7-11's connection record". DEC-800 item 4a gives the connect
sequence to the connection manager and the control services. There is no connection manager and
no vault in Phase 1 (the brief's scope), so the CLI command does their part. These are the
readings it takes.

### Decision

1. **The command reaches no broker and reads no credential.** It records a connection whose
   account an executor already reached. `--checked` names the `ConnectionChecked` (occasion
   `connect`, or `reconnect` for a revoked connection) that the executor journaled on
   `acct:{workspace}:{account_ref}`. The command confirms that check:
   - it is for the same `connection_id`, in the owner's environment;
   - every result passed, and scope, environment and account are listed, plus contract for
     Robinhood's MCP connection;
   - `account_pii_ref` is not `null`.

   It then commits `ConnectionEstablished` version 2, with that check as its `causation_id`
   (journal spec §9.8 rule 63). Agents never run the live connect: the founder's own build
   journals the check (DEC-529 item 1).
2. **Rule 64 and the binding are checked before the append**, against the control stream, as
   DEC-800 item 9 asks of the stream's owner:
   - an id established and not revoked since is `connection_exists`;
   - a revoked id is re-established only with its first establishment's broker, environment and
     `account_ref`, and never after a version-1 establishment (`reconnect_mismatch`);
   - an `account_ref` that another id's establishment names is `account_ref_bound`.
3. **Without the fingerprint, the command refuses what it cannot tell apart.** Check 4 compares
   account fingerprints, which only the vault computes (connections spec §3.1). Without one, any
   other connection established with the same broker and environment, revoked or not, may be the
   same account. So the command refuses it as `account_maybe_connected`. This refuses more than
   check 4 would; E7-14's connection manager replaces it with the fingerprint comparison.
4. **What each broker may record in this slice:**
   - Alpaca only in `paper` (DEC-441 item 3).
   - Robinhood only in `live`: it has no paper environment, and DEC-124's simulated stage runs
     the same live build against the simulator.
   - Kraken is not accepted: no crypto in this slice.
   - `backtest` is never accepted.
5. **The no-margin attestation** (DEC-529 item 11) is journaled, as `ConnectionEstablished`
   version 2's `margin_attestation` (journal spec §9.8 rule 64, #786):
   - `--margin-attestation` takes `cash_account` or `margin_disabled`;
   - it is required for `live` (`attestation_missing`), refused for any other value
     (`attestation_invalid`), and refused for `paper` (`attestation_not_live`).

   The record's `cli_confirm` step-up confirms it, since the code binds the whole record. The
   deployment that accepts `live` reads it from the journal, never from a flag (the D2 builder's
   half (b)). Rule 64 holds for every writer of a version-2 establishment, not only this command.
6. **Shown, then confirmed** (DEC-155 item 3):
   - Without `--code`, the command prints the record, its cause, and the code, and commits
     nothing.
   - The code is the first eight hex digits of the canonical record, the cause, and the
     attestation, so a code shown for one record confirms no other.
   - A re-run with the same code, whose control stream already ends with that record, answers
     the committed event and mints no assertion (DEC-290).
7. **What it prints** names the connection, the broker, the environment, the `account_ref`, and
   the scopes. It never names the personal-data reference, the account number, or a fingerprint
   (`AGENTS.md` rule 6).
8. **What the implementation needs from outside this module.** These are named here so the
   implementation PR does not widen silently:
   - the control stream's draft must carry a `causation_id` and schema version 2;
   - a step-up whose `authenticated_at` is a timestamp, as §9.8 types it;
   - `mandate-journal` must register version 2 (E7-17);
   - a production source of assertion ids for `run`.

   The first two are an extension to `control.rs`, which the lead holds.

<a id="dec-823"></a>

## DEC-823: The web app moves to Cloudflare Workers Paid through OpenNext (supersedes DEC-465 items 1 and 3)

| | |
|---|---|
| **Status** | Accepted (founder-reserved, DEC-79: spending). The founder chose "Cloudflare Workers $5/mo (Recommended)" on 2026-10-08 in the coordinator's session. Item 4 would be superseded in part, and item 3's removal of `vercel.json` deferred, by [DEC-1005](DEC-1005.md) (Proposed, for the founder) |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The founder, in the coordinator's session on 2026-10-08; recorded by an agent (Claude Code, the coordinator) |
| **Story or claim** | Platform lane L4 (web on live data, slice S11); [DEC-465](earlier-303-599.md#dec-465); [DEC-820](DEC-820.md) |

### Context

`web/` deploys to Vercel today, under DEC-465. The demo serves it at `app.owlhead.ai` (DEC-820). The
L4 lane checked the alternatives with a local build only: nothing was deployed and no credentials
were used.

- **Cloudflare Pages doesn't fit.** Pages serves only static exports, and the app renders on the
  server: it reads cookies, runs a `proxy.ts` auth gate and an auth callback, and has an API route.
- **The free Workers plan doesn't fit either.** Its 10 ms CPU limit per request is below the
  measured server render time of every page, which ranged from 19 ms to 218 ms.
- **Vercel's free Hobby plan is for non-commercial use.** Its paid plan costs about $20 a month.

### Decision

1. **Plan and adapter.** `app.owlhead.ai` runs on Cloudflare Workers Paid, at about $5 a month,
   through the OpenNext adapter (`@opennextjs/cloudflare`).
2. **This supersedes only part of DEC-465.**
   - DEC-465 item 1 (Vercel deploys only `main`) and item 3 (the policy lives in `vercel.json`)
     fall away when slice S11 lands.
   - Item 2 stays: GitHub's `web` workflow remains the pull-request gate.
   - DEC-465's purpose carries over. Cloudflare's non-production branch builds and preview URLs
     are turned off, so only `main` deploys.
3. **The L4 lane's slice S11** makes the repository changes:
   - the adapter and the `wrangler` configuration;
   - unoptimised images, or the Images binding;
   - the Next.js patch bump;
   - removal of `vercel.json`, its test, and the `vercel.json` trigger path in
     `.github/workflows/web.yml`;
   - the beta-request store moved to Supabase, as item 5 says;
   - edge `middleware.ts` in place of `proxy.ts`.

   That last change moves an authentication boundary, which AGENTS.md lists as safety-critical.
   It therefore goes tests-first (DEC-77), with an independent review. `@opennextjs/cloudflare` and
   `wrangler` each get a row in `docs/dependencies.md`.
4. **No Cloudflare credential exists outside the founder's account.**
   - The founder does every dashboard step themselves after S11 merges: the plan, the repository
     import, the build settings, the variables and the custom domain.
   - Builds run on Cloudflare's side from the GitHub connection.
   - No `CLOUDFLARE_API_TOKEN` or other Cloudflare secret goes in a workflow, a repository secret,
     the `wrangler` configuration or a script, and no workflow runs `wrangler deploy` or
     `wrangler login`. CI only builds (ES-14).
   - The founder protects the Cloudflare account with a hardware key or MFA. The account now
     controls DNS, the API's tunnel (DEC-822) and the web app.
5. **Only public values reach the web app.**
   - The variables are the Supabase URL, the publishable key and the API base URL.
   - The beta-request store works through the publishable key, with row-level security that
     allows anonymous inserts only. Those inserts are rate-limited, so the table can't be used for
     spam or to run up cost.
   - No Supabase service-role key or other secret is ever in `web/`, its build, or the Worker.
6. **Session cookies are host-only to `app.owlhead.ai`.** They carry no `Domain=` attribute, so
   they never reach `api.owlhead.ai` or any other subdomain unless the API's own scheme sets them.
7. **Merging to `main` is the deploy.** Agents merge their own changes after CI and an independent
   review (DEC-79), and Cloudflare builds `main`, so agents control what runs at `app.owlhead.ai`
   without holding a credential. That is accepted for the paper demo. The founder can roll back
   from the Cloudflare dashboard, or by reverting on `main`.

### Rationale

It is the lowest-spend host that runs the app as it is built, and it keeps the web app on the
founder's existing Cloudflare account next to the API's tunnel (DEC-822).

### Alternatives considered

- **Keep Vercel Hobby.** No spending, but its terms are for non-commercial use.
- **Vercel Pro**, at about $20 a month.
- **A client-rendered rewrite on Cloudflare Pages.** Free, but weeks of rework.

<a id="dec-824"></a>

## DEC-824: The founder's notification, CodeQL and suspended-connection choices of 2026-10-08

| | |
|---|---|
| **Status** | Accepted (founder-reserved, DEC-79). The founder answered the coordinator's questions on 2026-10-08 in the coordinator's session |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The founder, in the coordinator's session on 2026-10-08; recorded by an agent (Claude Code, the coordinator) |
| **Story or claim** | Platform lanes L3 (E8-9 to E8-14) and L2 (E7-17); [DEC-438](DEC-438.md) items 19, 20, 22 and 25; DEC-794 ([#800](https://github.com/kunwarshivam/mandate/pull/800)); DEC-800 item 5 ([#786](https://github.com/kunwarshivam/mandate/pull/786)); [DEC-500](earlier-303-599.md#dec-500); [DEC-820](DEC-820.md) |

### Context

DEC-794 and DEC-800 are lane decisions in [#800](https://github.com/kunwarshivam/mandate/pull/800)
and [#786](https://github.com/kunwarshivam/mandate/pull/786), not yet on `main`.

The coordinator put four questions to the founder, each with a recommendation:
1. DEC-438 items 19, 20 and 22 (notification channels), with the web push contact.
2. DEC-438 item 25 (an owner nobody can reach).
3. DEC-794 (CodeQL and the web push crate's RFC test vectors).
4. DEC-800 item 5 (how a suspended connection recovers).

### Decision

1. **Channels (DEC-438 item 19): accepted, without a chat channel.**
   - v1 notifies by email (DEC-820 item 3), by `web_inbox` with the web app, and by web push with
     the relay.
   - SMS and phone stay under E8-7.
   - HLD §12 item 6 is decided on that basis.
2. **Chat channel (DEC-438 item 20): not taken.** The recommendation was Slack through an incoming
   webhook. The founder has no Slack for this organisation, so v1 has no chat channel. Choosing one,
   Telegram at M10 included, is a new decision. The backlog keeps the chat channel as an open item.
3. **Web push and relay hosting (DEC-438 item 22): accepted.** There is no vendor. The VAPID `sub`
   contact is `mailto:push@notify.owlhead.ai`, on the sending domain DEC-820 item 1 names.
4. **An owner nobody can reach (DEC-438 item 25): accepted.**
   - **The rule.** A live agent whose owners have had no reachable push channel for 24 hours holds
     new openings until an owner re-verifies a channel. Exits and protection are untouched (rule
     13).
   - **Scope.** Paper agents are unaffected.
   - **Delivery.** The rule needs a mandate spec change. It is scheduled as its own story after the
     demo, and is not built before then.
5. **CodeQL and the RFC test vectors (DEC-794): accepted.** CodeQL skips exactly
   `crates/mandate-webpush/tests/vectors/`, which holds the published RFC 8291 and RFC 8188 keys,
   salt and messages. Within DEC-500:
   - no query is disabled and no alert is dismissed;
   - every query still runs on every other path;
   - a live test fails if product code names that directory.
6. **A suspended connection recovers by re-authorization (DEC-800 item 5): accepted.** Replacing
   the credential of a connection that is not revoked is `ConnectionCredentialRotated`. It is held
   to what a reconnect is held to:
   - it names a `reauthorize` `ConnectionChecked` in which every §8.1 check that can refuse passed;
   - it reaches the same account by fingerprint;
   - the control services accept it;
   - the owner acknowledges it.

   A second `ConnectionEstablished` remains only for a revoked connection.

### Rationale

Each choice is the recommended one, except item 20, where the founder lacks the recommended tool.
Items 4 and 6 keep every check a reconnect has, and item 4 only tightens. Item 5 keeps the query
running wherever a product key could appear.

### Alternatives considered

- A different push contact address.
- Keeping revoke-and-reconnect as the only way out of `suspended`, which means stopping agents and
  exiting positions first.
- Leaving the two CodeQL alerts open.

<a id="dec-830"></a>

## DEC-830: The lifecycle harness reads E8-3's answer records as journal spec §9.7 writes them, and the superseded `decided_by_now` until E8-3 lands

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the E8-3 harness builder) under DEC-79 and DEC-176: a tests correction that reads the binding spec and widens no runtime behaviour |
| **Story or claim** | E8-3; DEC-533 items 3 and 4; the tests correction for [#782](https://github.com/kunwarshivam/mandate/pull/782) |

### Context

E8-3's implementation ([#782](https://github.com/kunwarshivam/mandate/pull/782)) makes the runtime
write two records as journal spec §9.7 closes them (DEC-533 items 3 and 4):

- every `ApprovalResponded` carries `quorum`, `separation_of_duties` and `delegation`, `null` where
  nothing was judged;
- `ApprovalRevalidated.decided_by_now` is the re-classification's `ask` label, or `null` when it is
  not an `ask` or its label is empty.

Family E's lifecycle harness (`crates/mandate-refcases/src/mandate/escalation/lifecycle.rs`) fails
#782 on both:

- it refuses any payload member a case does not state, and the `ApprovalResponded` steps written
  before §9.7 state none of the three members;
- it derives `decided_by_now` as the re-classification's `by` label whatever the decision, or `""`
  with none, which is what the runtime on `main` writes and §9.7 no longer allows. MC-E19 (a `deny`
  by `rule:no_more`) and the `auto` edits of MC-E01 in `now_reaches_the_runtime_through_its_ports`
  reach it.

DEC-77 lets #782 change tests only by deleting `#[ignore]` lines, so the harness is corrected in a
tests-correction PR that merges first and passes on `main`'s runtime as well as with #782.

### Decision

1. **The three answer members.** Where a case's `ApprovalResponded` step does not state `quorum`,
   `separation_of_duties` or `delegation`, the harness accepts that member only absent or `null`,
   never a value. A step whose grant check 7 judged (rule 48: `approved`, and `admitted` or
   `counted`, or refused `duplicate_approver` or `not_independent`) must state its `quorum`, or the
   case fails. Every such step in `mandate.yaml` (MC-E01, E06, E17, E19 to E24, E29) states it, so no
   reference case changes. This is §9.7's own reading and needs no choice; it is recorded here with
   item 2 so the follow-up in item 3 has one place to cite.
2. **`decided_by_now`, transitional.** The harness expects §9.7's value. Until #782 merges it also
   accepts the one superseded value `main`'s runtime writes, the `by` label whatever the decision or
   `""` with none, and no third value. This is the one choice here: the harness on `main` today
   requires the superseded value, so accepting §9.7's as well loosens nothing it checks, and
   `mandate-runtime`'s `decided_by_now_is_null_unless_the_reclassification_asks` (pending E8-3)
   pins the runtime to §9.7 alone once #782 lands.
3. **Follow-up.** After #782 merges, a tests tightening removes item 2's superseded arm and item 1's
   acceptance of an absent member, so the harness then accepts §9.7's records only.

### Rationale

The tests correction must pass on two runtimes that write different values, and only an oracle that
accepts exactly those two values does, without hiding any of the harness's coverage behind pending
markers. Each accepted value is named and bounded: no third value passes, and the unit tests in
`lifecycle/tests.rs` show each reading fails on a seeded bug.

### Alternatives considered

- **A spec PR stating the three members on every pre-§9.7 step** (the dispatch's option (b)).
  Rejected: the cases would then fail on `main`'s runtime, which omits the members, so the
  lifecycle tests and the reference cases they run would have to go pending until #782 merges, and
  it would not touch `decided_by_now`, which no case states.
- **A strict §9.7 harness with the failing tests marked `pending E8-3`.** Rejected: it hides MC-E19
  and every whole-case lifecycle test on `main` until #782 merges, and #782 would then have to
  delete those markers and their `BEHAVIOUR_ONLY_TESTS` rows.
- **Comparing `decided_by_now` only through check 10's result.** Rejected: it drops a recorded
  value from the comparison for good, where item 2 drops nothing and is removed by item 3.

<a id="dec-831"></a>

## DEC-831: The MCP transport reads an explicit `null` member as present, so a response with `null` beside its result or error is malformed

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the E7-16 tests builder) under DEC-79 and DEC-176: a reading that only refuses more answers and adds no risk |
| **Story or claim** | E7-16; connections spec §6.2 rule 1; [#719](https://github.com/kunwarshivam/mandate/pull/719) |

### Context

JSON-RPC 2.0 §5 says of a response object: `result` "MUST NOT exist if there was an error", `error`
"MUST NOT exist if there was no error", and "both members MUST NOT be included". It does not say
whether a member whose value is `null` exists. A reader built on a typical deserializer, where a
missing member and a `null` one both become "none", would take
`{"jsonrpc":"2.0","id":1,"result":{},"error":null}` as a success and
`{"jsonrpc":"2.0","id":1,"error":{...},"result":null}` as an error, so one body would carry two
answers and the reader would pick one. `method` and `params` belong only to requests and
notifications (§4), so a response carrying either, `null` included, is not a response.

Connections spec §6.2 already treats an unparsable or unexpected answer to the place as `Unknown`,
never rejected and never re-sent, so how the transport reads these bodies decides whether an
ambiguous answer becomes a result or that conservative state.

### Decision

1. **A member is present when its name is in the object**, whatever its value, `null` included.
2. **A response is well formed only when it has exactly one of `result` and `error`** and neither
   `method` nor `params`. Any other shape, an explicit `null` counting as present, is
   `McpError::Malformed` (code `malformed`).
3. The transport's pending test
   `crates/mandate-mcp/src/tests/bounds.rs::an_empty_event_is_skipped_and_an_explicit_null_is_present`
   pins the four shapes: `"error":null` beside a result, `"result":null` beside an error, and a
   `null` `method` or `params` beside a result. The same test pins a plain `"result":{}` as
   accepted, so a reader that refuses every answer fails it.

### Rationale

The reading only refuses answers the other reading would accept, and a refused answer becomes
`Malformed`, which the connector reads as an outcome it does not know: an order is `Unknown` and
reconciled, never re-sent (§6.2, trading spec §5.3). It loosens no rule and weakens no safety
invariant, so DEC-176 lets an agent take it. Reading `null` as absent would let a server, or a
proxy that rewrites bodies, turn an error into a success by adding `"result":null` or the reverse,
and the transport would choose between two answers it was given.

### Alternatives considered

- **`null` is absent.** Accepts the bodies above as one answer each; rejected because the body
  then holds two contradictory answers and the transport picks one.
- **Accept `null` only for `error` beside a result.** Some servers emit `"error":null` on success.
  Rejected for now: the broker's server is not known to do so, and a carve-out is a loosening that
  can be proposed with evidence if a pinned server needs it.

<a id="dec-838"></a>

## DEC-838: How protection reads a capability profile

| | |
|---|---|
| **Status** | Accepted (agent, with the coordinator's ruling on item 5: each item only tightens what trading-domain spec §5.1, §5.2 and §5.4 allow, DEC-176; reversible engineering under DEC-79 and AGENTS.md rule 9) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the builder of E7-23 B2a's tests) |
| **Story or claim** | E7-23 B2a (claim #857); [first live trade brief](../tasks/first-live-trade.md) LT-2, LT-14; DEC-531 item 2; DEC-630 item 1 |

### Decision

1. **The order of strength for a protective order placed on a position that exists is OCO, then
   one resting stop-limit.** §5.1 puts "OCO or bracket legs" first and the stop-limit only "on a
   profile with neither". A bracket is not a candidate: its legs wait for an entry that is still
   working (§5.4), so it belongs to the entry. An OCO needs a take-profit price. The stop-limit
   needs the mandate's `stop_limit_offset` (DEC-539), and its take-profit is the runtime's (§5.4).
   With no form offered `protective_shape` returns `None`, and `re_place` routes it into the existing
   `unprotectable(...)` and `unprotected_end` path with `uncovered = true` (a journal record and an
   owner alert), never into an error and never into a silent skip. Deployment already refuses a
   mandate whose protection a profile cannot meet (§5.2, §5.4), so this is the defensive case. Today
   the arm is `Err(Unimplemented { story: "E7-4" })` (`protection.rs`, `re_place`).
2. **A form is offered only when the cell for the order's own type lists it and also lists `gtc`.**
   The cell of an OCO is `limit`'s, and of the stop-limit `stop_limit`'s (DEC-630 item 1). §5.1
   makes every protective order GTC, so a profile that lists the form without `gtc` offers none.
3. **The quantity form is whole when the quantity is a whole number and fractional otherwise.** A
   profile therefore lists a crypto stop-limit in both its whole and its fractional cell if it
   accepts both, as Alpaca's crypto row does; an unlisted cell offers nothing (DEC-630 item 9).
4. **Protection reads the protective order's own session, never the clock's.** A US equity reads
   its regular-session row and crypto its crypto row, because a GTC protective order rests and
   triggers in the regular session (§5.4). Re-placement runs at `TradingDayStarted`, in pre-market
   and after exits; reading the clock's session would find no cell on Alpaca's regular-only equity
   row and leave the position without the protection it has today (`AGENTS.md` rule 13, LT-14).
   `protective_shape` takes the clock's session only so that a test can show it is not used.
5. **How the profile reaches `re_place`, with no existing test edited (LT-14).** `ExecutorState`
   carries the profile (`with_profile`), set by the shell after the fold at every start from
   `connector.profile()`; `Ports` and its literals do not change. `ExecutorState::new` keeps its
   signature and starts with a transitional default equal to §5.2's Alpaca table (equities'
   regular row, crypto's row with `stop_limit`, `gtc` and the `StopLimit` form in both the whole
   and fractional cells), so an unset profile never drops crypto protection and existing fixtures
   are unchanged. The default is recorded as backlog debt and goes once every constructor passes a
   profile. The coordinator accepted this plumbing on these conditions:
   - The implementation PR adds a test that the shell calls `with_profile(connector.profile())` at
     every start.
   - The default is deleted by the story that first lands a non-Alpaca executor path (B3), before
     that path merges.
   - Until then, an `ExecutorState` built without a profile is Alpaca-only by contract.

   The reason: an Alpaca shape sent to a broker that refuses it would leave a position
   unprotected, which breaks `AGENTS.md` rules 3 and 13.
6. **Out of scope for B2a:** `crypto_add` (`protection.rs`) still reads `AssetClass::Crypto` for
   E7-4's add rule, as do `is_protected`, `ticked` and the reads in `kill.rs`, `intent.rs` and
   `session.rs`.

### Rationale

Each item is the tighter reading where the spec is silent: no form is placed that a cell does not
list for the order sent. Alpaca's outcomes do not change (LT-14): an equity OCO, and crypto's one
stop-limit, now through its profile. Alpaca's crypto row is added with the implementation.

### Alternatives considered

- **Strength from the form's position in the profile's set:** a set has no order; the spec does.
- **Quantity form from the instrument's share increment:** couples protection to instrument data
  for a rule the profile already states.

<a id="dec-839"></a>

## DEC-839: The MCP client's contract hash, fund-movement test, and drift halt are defined as follows

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the E7-16 M2 tests builder) under DEC-79 and DEC-176: each reading only refuses more, or leaves exits as rule 13 already requires |
| **Story or claim** | E7-16 (M2); connections spec §6.2 rules 2 and 3, §8.1 checks 1 and 7, CN-2, CN-9; DEC-441 item 8; claim [#859](https://github.com/kunwarshivam/mandate/issues/859) |

### Context

Spec §6.2 rule 3 says the connector hashes "the canonical form of the allowlisted tools' names and
input and output schemas", and CN-2 refuses a tool list containing "a transfer tool". It defines
neither the canonical form nor what makes a tool a transfer tool, and `mandate-canon` cannot hash
a JSON Schema (its keys must match `^[a-z][a-z0-9_]{0,63}$`; schemas use `additionalProperties`).

### Decision

1. **The hash** is SHA-256 of the UTF-8 bytes of a JSON array of the nine allowlisted tools sorted
   by name, each `{"inputSchema":S,"name":N,"outputSchema":O}`, with `O` the JSON `null` when the
   tool has none. `S` and `O` are written compact with object keys sorted by byte value, at every
   depth. Descriptions, titles, annotations, other tools, and the order of the list are not in it.
   A number that is not an integer, a duplicate tool name, a tool without a string name, or an
   allowlisted tool without `inputSchema` makes the list `Malformed`, which refuses the connection.
2. **The list is read to its last page** (`nextCursor`), so a tool cannot hide on page two.
3. **A fund-movement tool** is any listed tool, allowlisted or not, whose name has a token in:
   `transfer`, `transfers`, `withdraw`, `withdrawal`, `withdrawals`, `wire`, `ach`, `send`,
   `payout`, `disburse`, `deposit`, `deposits`, `fund`, `funds`, `funding`. The name is split at
   every non-alphanumeric character, at each lower-to-upper case change, after a digit before a
   capital (`v2Transfer` is `v2|transfer`), and, within a run of
   capitals, before its last capital when a lowercase letter follows (`ACHDebit` is `ach|debit`,
   `URLParser` is `url|parser`), then lower-cased. A name with any non-ASCII character is a fund-movement tool, which closes homoglyph tricks
   (a Cyrillic `і` in `wіre`, fullwidth forms, zero-width characters); every allowlisted name is
   ASCII, so nothing legitimate is lost. Tokens are whole words: `get_fundamentals` is a
   different word and is accepted, which is not a loosening. Only the name decides; descriptions
   are never read for meaning (CN-9). A read the tokens over-refuse, such as `get_deposits`, follows
   item 4: refused at the first connect, a halted client on reconnect, exits allowed. The list is
   the one the shared `mandate_domain::fund_movement` check uses (E7-12). A fund-movement tool is
   `FundMovementTool`, reported before a missing tool.
4. **Drift halts openings, and nothing lifts the halt but a new connection with a new pin.** A
   hash that differs from the pin, a fund-movement tool, or a missing allowlisted tool sets the
   halt. Only an `Ordinary` `place_equity_order` is halted; reads, `review_equity_order`,
   `cancel_equity_order`, and every `RiskReducing` call go on (`AGENTS.md` rule 13). A
   `RiskReducing` call to a missing tool is sent and fails at the server like any call.
5. **A connect with no stored pin** (the first establishment, spec §8.1) returns no client for a
   fund-movement tool or a missing allowlisted tool. **A connect with a stored pin** (every
   reconnect or session start, §6.2 rule 3) never returns no client for these causes: it returns a
   halted client for all three, since rule 13 lets only agent mode, an `Unknown` order, or the
   broker hold an exit. The allowlist still refuses calling a listed fund tool itself.
6. **A call outside the allowlist** (exact match, no trimming or case folding) is refused before
   anything is sent or any budget is drawn.

### Rationale

Each point only refuses more than the open reading would, or keeps what rule 13 already requires.
A name test can over-refuse (a read tool called `get_transfers`); a refused connection is the safe side.

### Alternatives considered

- **Hash the whole tool object.** Rejected: a reworded description would halt openings, and the
  spec names names and schemas only.
- **Halt every `place_equity_order`.** Rejected: it would deny a protective sell (rule 13).
- **Un-halt when the contract reverts.** Rejected: a flipping server should not reopen on the good answer.

<a id="dec-841"></a>

## DEC-841: An unclassified instrument is protected as a US equity; only a missing profile or cell leaves a re-placement uncovered

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176, with the coordinator's ruling on item 1; reversible engineering under DEC-79) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the builder of E7-23 B2a), with the coordinator's ruling |
| **Story or claim** | E7-23 B2a (claim #857); DEC-838 items 1, 4 and 5; [first live trade brief](../tasks/first-live-trade.md) LT-2, LT-14 |

### Decision

1. **An instrument the instrument snapshot does not classify is protected as a US equity.**
   `re_place` reads the profile's US-equity regular-session row for it, as the executor did before
   B2a, which read such an instrument as "not crypto". A position that had a protective order
   keeps one (`AGENTS.md` rule 13, LT-14).
2. **Only a profile that offers no usable cell, or an executor whose transitional profile failed
   to build, leaves a re-placement without a shape.** That case takes DEC-838 item 1's path:
   `unprotectable(...)` (an `expiry_unreplaceable` record and the `protection_expiring` alert)
   and an `unprotected_end` marked `uncovered`, whose interval stays open and bounded. It is never
   an error and never a silent skip. The transitional table is a constant that
   `CapabilityProfile::new` accepts, so its failure is the defensive case.

### Coordinator's ruling

The first draft of this decision routed an unclassified instrument to the uncovered path too. The
coordinator overruled that for rule 13: leaving the position bare, with a journal record and an
alert, removes protection the executor placed before B2a, so it is not a pure tightening. Reading
the instrument as an equity, from the regular row, keeps it.

### Rationale

The equity reading is the one the executor and `session.rs` already take for an instrument the
snapshot does not classify (#400 round 1, minor 3), so it changes no outcome (LT-14). The shape it
reads still comes from the profile, never from a broker's name (LT-2).

### Alternatives considered

- **Route an unclassified instrument to `uncovered`:** the first draft; overruled (rule 13).
- **Return an error:** DEC-838 item 1 rules out an error for a missing shape.

<a id="dec-843"></a>

## DEC-843: The MCP client halts openings on an unreadable health-check list, and hashes the listed allowlisted tools when one is missing

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the E7-16 M2 implementation builder) under DEC-79 and DEC-176: item 1 only halts more, and item 2 changes no outcome |
| **Story or claim** | E7-16 (M2); connections spec §6.2 rule 3, §8.1 check 7; DEC-839 items 1 and 4; claim [#859](https://github.com/kunwarshivam/mandate/issues/859) |

### Context

DEC-839 says a `Malformed` tool list refuses the connection, and that drift, a fund-movement tool,
or a missing tool halts openings. It does not say what a health check does with a list it cannot
read exactly, nor what hash a list missing an allowlisted tool has, since item 1's form names nine
tools.

### Decision

1. **A health check whose list is `Malformed` halts openings**, as a drift would, and returns
   `Malformed`. A check whose exchange fails (timeout, network, HTTP status, JSON-RPC error,
   throttled) returns that error and halts nothing: no list was read, and the next check reads
   again. As with every halt, exits, cancels, reviews and reads go on (`AGENTS.md` rule 13).
2. **With an allowlisted tool missing, the hash is DEC-839 item 1's form over the allowlisted tools
   that are listed.** It can never equal a nine-tool pin, and `ContractMissingTool` is reported
   ahead of drift, so the hash is informational only; `McpClient::contract` returns it.

### Rationale

A list that cannot be read cannot be shown to match the pin, so the safe side is to stop openings
until a new connection re-establishes the pin. A failed exchange says nothing about the contract,
and halting on it would make a transient outage sticky, which DEC-839 item 4 reserves for evidence
of a change.

### Alternatives considered

- **Halt on any failed check.** Rejected: a single timeout would stop openings until a reconnect.
- **Leave `Malformed` at a health check unhalted.** Rejected: a server could hide a change behind
  an unreadable list.

<a id="dec-847"></a>

## DEC-847: The founder's OAuth login dials only pinned hosts, checks identifiers exactly, and registers a public client (O1a)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-24 lane, under DEC-176: each item only tightens the published flow, and none adds risk |
| **Story or claim** | E7-24 (the first live trade brief's O1a); claim #859; the O1a tests PRs |
| **Amended** | O1a tests part 2, from #929's review: item 2's empty default list and credentials or fragment in an `authorization_servers` entry; item 5's detail. O1a implementation: item 6, the coordinator's readings of #934's review minors. O1a tests correction: item 7, the challenge URL's form. O1a tests correction 2: item 8, the issuer's query, the registration answer's other members, and the challenge parameter's boundary |

### Decision

The login follows the MCP authorization flow of the transport's revision (2025-06-18) and the RFCs
it cites, read the strict way where they leave a choice:

1. **Pinned hosts.** Protected-resource metadata (RFC 9728) is read only from the pinned MCP host.
   Its URL is the `resource_metadata` of the `401` challenge to an unauthenticated `initialize`;
   without one, the endpoint's RFC 9728 well-known URL. No other fallback is tried.
2. **One authorization server.** Only the first of `authorization_servers` is used, and its host
   must be on the connector's compiled-in authorization hosts; its RFC 8414 metadata, and every
   endpoint it names, must be `https` on those hosts, with no credentials or fragment. A host off
   the list is refused before anything is sent to it. No `3xx` is followed, and no refusal
   carries server text (CN-9). The authorization-host list is empty in the default build; only
   the runner's `live` feature adds hosts to it, as it adds the MCP host. This extends CN-3's
   build rule and ES-23 to the authorization hosts, so no default, CI or release build can log in
   anywhere. An `authorization_servers` entry carrying credentials or a fragment is refused.
3. **Exact identifiers.** `resource` must equal the endpoint and `issuer` the issuer it was read
   for, character for character (RFC 9728 §3.3, RFC 8414 §3.3).
4. **A public client only.** `S256` must be listed in `code_challenge_methods_supported`, as the
   later MCP revision requires; `code` must be a response type; a registration endpoint must exist,
   since there is no pre-registered client.
5. **Registration (part 2).** RFC 7591, as a public client: `authorization_code` only, response type
   `code`, `token_endpoint_auth_method` `none`, and the one loopback redirect
   `http://127.0.0.1:<port>/callback`, and no other member. An answer that issues a client secret,
   even an empty one, or names an auth method other than `none`, is refused, and the secret never
   surfaces (the brief's stop condition). An answer listing any redirect set other than that one is
   refused. A `client_id` must be non-empty visible ASCII.
6. **Registration answers, read strictly (implementation).** Where item 5 leaves a choice, the
   answer is read the way that holds less: a `client_secret` member is refused as
   `client_secret_issued` whatever its value, `null` included, and its value is never read; a
   `redirect_uris` that is not exactly a one-element list of the redirect sent, such as a bare
   string or a list repeating it, is refused as `redirect_changed`; only HTTP `201 Created` is a
   registration, so any other status, `200` included, is `http_status`; and a `client_id` is
   non-empty and every byte is visible ASCII, `0x21` to `0x7e`, so a space, a control character,
   DEL, or any non-ASCII character is `malformed`.
7. **The challenge URL's form (tests correction).** The `resource_metadata` parameter is read
   only as a quoted string, since a URL holds characters an RFC 9110 token cannot; an unquoted
   value is not read, and discovery falls back to the endpoint's well-known URL on the pinned
   host, as with no parameter at all. A `resource_metadata` URL carrying a query is refused as
   `endpoint_shape` before it is dialed, as the pinned endpoint itself is. Both only narrow what
   is dialed.
8. **Issuer, registration answer, and challenge parameter, read strictly (tests correction 2).**
   An `authorization_servers` entry, the issuer, that carries a query, even an empty one, is
   refused as `endpoint_shape` before its metadata is dialed (RFC 8414 §2: the issuer identifier
   "has no query or fragment components"). A registration answer without
   `token_endpoint_auth_method` is refused as `malformed`: RFC 7591 §3.2.1 has the server "return
   all registered metadata about the client", and §2 makes an omitted method mean
   `client_secret_basic`, a confidential client. A `grant_types` or `response_types` member that
   is present and not exactly `["authorization_code"]` or `["code"]` is refused as `malformed`;
   either may be omitted, since §2's defaults are those values. `resource_metadata` is matched only
   where an auth-param starts (at the start of the header, or after a space or a comma) and never
   inside a quoted value; quoted pairs are not read, so a challenge that escapes a quote can only
   fall back to the well-known URL or name a URL on the pinned host. Each only narrows what is
   accepted or dialed. A tab is not a parameter boundary, so a parameter after a tab is not
   read; that is stricter than RFC 9110's whitespace and only narrows what is accepted. If
   Robinhood's real server omits `token_endpoint_auth_method` from its `201`, the founder's login
   at R0 refuses; that is a finding for a new decision, never a workaround.

### Rationale

Every host the process dials is one a build names (LT-1, connections spec §6.2 rule 1), so no
document can send the login elsewhere; the exact identifiers are the RFCs' mix-up defences; and
each refusal ends the login before a token exists (rule 3). Residual, disclosed: if Robinhood's
server deviates from the flow, the founder's login at R0 step 4 refuses, and the fix is a new
decision, never a workaround.

### Alternatives considered

Any `https` authorization host (the host would come from server text); the root well-known and
OpenID fallbacks (not taken until a real answer needs them); a confidential client (the brief's
stop condition forbids a client secret).

<a id="dec-850"></a>

## DEC-850: every mutant builds incrementally on a tree that already holds its tested packages' graph

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79: a reversible change to the gate's configuration that judges more mutants and lowers, raises, or skips nothing; CI and xtask are founder-owned in `CODEOWNERS`, so the founder may overturn it) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The DEC-850 builder (agent), under DEC-79 |
| **Story or claim** | [DEC-519](earlier-303-599.md#dec-519) item 7's open case, met on [#931](https://github.com/kunwarshivam/mandate/pull/931) (CI run [37885302040](https://github.com/kunwarshivam/mandate/actions/runs/37885302040)); DEC-464's ten-minute limit; DEC-498's shard budget |

### Decision

1. **The pre-flight builds every package the run tests.** Its `cargo nextest list` names every
   package with a mutant, whose live tests it counts as before (DEC-139), and every other package
   in the run's `--test-package` set, `mandate-refcases` included (`preflight_args`). A nextest listing
   builds the test binaries it lists, so this is one build of the tested packages' whole graph,
   with the job's profile (DEC-519 item 2), in the repository's own `target/`.
2. **`cargo mutants` runs `--in-place`** (`MUTANTS_IN_PLACE`), in that same tree, rather than in a
   scratch copy whose `target/` starts empty. The unmutated baseline and each mutant therefore
   rebuild only what their own edit changes, and the 60-second build cap judges that, never the
   rest of the graph.
3. **`--jobs` is dropped.** cargo-mutants 27.1.0 refuses `--jobs` beside `--in-place`, and one
   tree holds one mutant, so the run stays one mutant at a time, which is DEC-498 item 2 unchanged.
4. **A run that leaves a mutant behind is restored and refused.** cargo-mutants reverts each mutant
   as it finishes it. xtask also reads every source file of the diff before the run and compares
   after it, whatever the exit status. A file that differs is written back and the run fails,
   naming it (`UnmutatedSource`). So an in-place run cannot leave mutated source in a developer's
   checkout or pass a verdict over a tree it did not restore.
5. **Nothing else moves.** The 60-second build cap, the 180-second test cap that also bounds the
   baseline's tests, one mutant a shard, the `min(192, n)` matrix (DEC-538), the database
   (DEC-519 item 1), no debuginfo (DEC-519 item 2), the external oracles (DEC-497), the
   `CARGO_TARGET_DIR` removal (#419), and the zero-survivor rule all stand. No cap is lowered or
   raised, no mutant is skipped, no crate is excluded, and a `TIMEOUT` still makes `cargo mutants`
   exit 3, which `mutants_outcome` refuses.
6. **Three xtask tests hold it.**
   `the_preflight_builds_every_tested_package_before_the_run_mutates_in_place` requires every
   mutated and every tested package in the listing, each once.
   `mutation_arguments_add_a_shard_once_and_leave_local_runs_complete` now requires `--in-place`
   once and no `--jobs`. `an_in_place_run_that_leaves_a_mutant_behind_is_restored_and_refused`
   plants a mutated file after the read. Each failed on a seeded bug (below). The fixture-repository
   tests that run the whole job (`the_job_refuses_a_crate_whose_mutants_no_live_test_would_judge`,
   `a_callers_cargo_target_dir_cannot_flip_the_mutants_verdict`) pass in place, the second still
   with no build tree in the caller's directory.

### Rationale

#### The failure

cargo-mutants 27.1.0 builds its baseline over the packages the run's own mutants are in
(`Lab::run_baseline`, `all_mutated_packages`), not over `--test-package` (DEC-498, "The baseline's
packages come from the shard's own mutants"). Each mutant then builds and tests the whole
`--test-package` set. In a scratch copy, whose `target/` is empty, the mutant's first build
compiles everything the baseline did not. #931's tested packages are `mandate-alpaca`,
`mandate-executor`, `mandate-shell` and `mandate-refcases`. Run 37885302040, before this change:

| Shard, attempt | Mutant | Pre-flight (3 packages) | Baseline | Mutant | Job |
|---|---|---|---|---|---|
| 7/16, 1 | `shape.rs:62:55` `&&` → `\|\|` (`mandate-executor`) | 66s | 23s build + 118s test | **TIMEOUT, 60s build** | 309s |
| 7/16, 2 | the same | 23s | 22s build + 119s test | **TIMEOUT, 60s build** | 267s |
| 1/16, 1 | `protection.rs:1866:5` `uncovered` → `Ok(())` (`mandate-executor`) | 62s | 22s build + 115s test | **TIMEOUT, 60s build** | 300s |
| 1/16, 2 | the same | 17s | 17s build + 82s test | caught, 53s build and test together | 208s |
| 13/16, 1 | `adapters.rs:1891:9` `use_profile` → `Ok(())` (`mandate-shell`) | 62s | 51s build + 4s test | TIMEOUT, 42s build + **180s test** | 379s |
| 13/16, 2 | the same | 15s | 33s build + 3s test | caught, 139s build and test together | 230s |

A `mandate-executor` baseline builds `mandate-executor` alone. The mutant then compiles
`mandate-alpaca`, `mandate-shell` and `mandate-refcases` on top, from nothing, and that took more
than 60 seconds on three of four attempts. How much more is not known, because the cap cut it off.
The nearest measure is item 1's own cold build of the same four packages' graph, 66 to 72 seconds,
below. Shard 13's attempt-1 timeout is in the test phase, not the build, and is a separate matter
(below).

#### The measurement, with this change

A throwaway branch, `claude/mutants-build-cap-measure`: #931's head `12bee907`, this change, and one
measurement-only commit that adds `--caught` so cargo-mutants prints a caught mutant's phase
times. It ran by `workflow_dispatch` (run
[37888679198](https://github.com/kunwarshivam/mandate/actions/runs/37888679198)), whose base is
the merge base with `main`, so its diff and its sixteen mutants are #931's. Every job passed,
`fast`, `full-checks`, all sixteen shards and `full`.

| Shard | Pre-flight (4 packages, cold) | Baseline | Mutant | Job |
|---|---|---|---|---|
| 7/16 | 72s | 17s build + 118s test | caught, **24s build** + 33s test | 308s |
| 1/16 | 69s | 16s build + 120s test | caught, **22s build** + 4s test | 273s |
| 13/16 | 66s | 21s build + 3s test | caught, **6s build** + 173s test | 318s |

The sixteen shards took 131 to 318 seconds. So the mutant builds that ran out of sixty seconds now
take 22 to 24, and the cold build they used to carry, 66 to 72 seconds for the whole graph, has
moved to the pre-flight, which no cap reaches and which already built three of the four packages
before. The baseline's build stays partly incremental (16 to 21 seconds), because it selects only
the mutated package, and cargo builds that package set separately from the four-package one.

#### The budget

DEC-498's equation is unchanged, with `k` the mutants in a slice:

```
k × (60 + 180) + 180 ≤ 600 − remainder      k = 1, so the remainder has 180 seconds
```

The remainder is the job less its three capped phases (the baseline's tests, the mutant's build,
the mutant's tests):

| Shard | Before (run 37885302040, attempts 1 and 2) | After (run 37888679198) |
|---|---|---|
| 7/16 | 309 − 118 − 60 = **131**; 267 − 119 − 60 = **88** | 308 − 118 − 24 − 33 = **133** |
| 1/16 | 300 − 115 − 60 = **125**; 208 − 82 − 53 = **73** | 273 − 120 − 22 − 4 = **127** |
| 13/16 | 379 − 4 − 42 − 180 = **153**; 230 − 3 − 139 = **88** | 318 − 3 − 6 − 173 = **136** |

Before, the widest was 153, of which 51 was a scratch baseline build. After, 127 to 136, against
the 180 reserved. The remainder now holds the whole cold build of the tested packages' graph. That
term is diff-dependent: a diff whose tested graph is larger pays more of it there, with no cap.
DEC-498 item 9 asks that this be named with its reserve and its failure mode. The reserve is the
44 to 53 seconds left over the widest measured here. The failure mode is the job's ten-minute
`timeout-minutes`, which fails the shard loudly and never counts a mutant caught. That term used
to be cut off at sixty seconds inside a mutant's build, where it turned an honest PR red. Now it
is paid once a shard, in the term DEC-498 item 6 already named. If it ever exhausts the reserve,
the remedy needs its own decision, against that diff, and DEC-498 item 8 already says the matrix
is not it.

#### What this does not settle: shard 13's test phase

The `use_profile` → `Ok(())` mutant in `mandate-shell` is caught at 173 seconds of tests here and
timed out at 180 on run 37885302040's first attempt. That is the 180-second test cap, not the build
cap. This decision does not change it. The margin is a property of #931's tests: the test that
catches this mutant takes most of three minutes to do it. A faster-catching test would fix it, not
a larger cap.

#### Seeded bugs

Each was planted alone in `xtask/src/main.rs`, and the named test failed, then it was reverted:

| Seeded bug | Test that failed |
|---|---|
| `MUTANTS_IN_PLACE` removed from `mutants_args` | `mutation_arguments_add_a_shard_once_and_leave_local_runs_complete` |
| `preflight_args` lists only the mutated packages | `the_preflight_builds_every_tested_package_before_the_run_mutates_in_place` |
| `preflight_args` lists only the tested set, not the mutated packages | the same |
| `UnmutatedSource::restored` does not write the file back | `an_in_place_run_that_leaves_a_mutant_behind_is_restored_and_refused` |
| `UnmutatedSource::restored` never refuses | the same |

### Alternatives considered

- **Raise the build cap.** It would leave the cold graph inside a mutant's build, now 66 to 72
  seconds and growing with the diff, and spend the budget's capped seconds on it. DEC-498 item 5
  chose the three caps together. Refused.
- **Make the baseline test every tested package** (`--cargo-arg=--package=…`, which reaches every
  phase). It would build the graph in the baseline, but in the scratch copy, so the pre-flight's
  build of the same graph is thrown away and paid twice. It would also add the full tested suite to
  the baseline's test phase, up to another 180 seconds a shard.
- **`--copy-target=true`** with the pre-flight's `target/`. cargo-mutants copies with new mtimes,
  in walk order, so whether cargo finds the copied workspace artifacts fresh depends on the order
  `ignore`'s walk reaches `crates/` and `target/`. A copy of `target/` also costs every shard its
  size in I/O.
- **In place on CI only, scratch copies locally.** It would make `cargo xtask check` build mutants
  differently from the gate it reproduces, which DEC-519 item 2 refused for debuginfo. Item 4's
  restore check makes in place safe for a local checkout instead.
- **Narrow `--test-package` to the shard's mutated packages.** It would drop the external oracles
  (DEC-497) and the other touched crates' tests from judging a mutant. A weakening. Refused.

<a id="dec-851"></a>

## DEC-851: X1 refuses a command line run from an expansion, and reads a variable-built command

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The coordinator, as a reading that only tightens DEC-529 item 3 (DEC-176) |
| **Story or claim** | E7-26 (X1), claim #857; X1 tests corrections 9 and 10, step 3b', step 3c |

### Context

Before step 3c, the reviewer found a bypass of DEC-529 item 3 that steps 1, 3a and 3b all let
through. A whole command is built in a variable across several assignments and then run:

```sh
C=ca; C+=rgo; C+=" build -"; C+="-feat"; C+="ures l"; C+=ive
eval "$C"
```

The same variable also gets through when run as a bare `$C`, as `"$C"`, as `sh -c "$C"`, and as
`bash <<< "$C"`. Rule 3 (a command word holding `$` may be cargo) needs a later expanding word
beside the command word, and a bare `$C` has none.

A first reading would refuse every command word that is solely an expansion. That reading would
refuse real lines:

- `deploy/lib.sh`'s `"$@"`;
- `deploy/set-secrets.sh`'s `"${VAULT_IMPORT[@]}"`;
- `nightly.yml`'s `outputs: run: ${{ … }}`;
- through 3b's re-reading of quoted words, about 140 other lines.

The coordinator ruled that no deploy script or workflow changes for this.

### Decision

1. **A command line run from an expansion is refused** (X1 tests correction 9). It is refused at
   top level, after a keyword, and inside a word 3b re-reads, in each of these forms:
   - `eval` with any argument holding `$` or a backtick;
   - `sh`, `bash` or `zsh`, named by any path, given `-c` (alone or in a short-flag cluster such as
     `-ec`) and an argument holding `$` or a backtick;
   - `sh`, `bash` or `zsh` given such a here-string (`<<<`).

   A wrapper before the command is skipped to find it (X1 tests correction 10, after #962's
   review), so `env bash -c "$C"` is refused like `bash -c "$C"`. The wrappers are:
   - `env`, with its options, `-u X` and `VAR=val` assignments;
   - `command`, `exec`, `nohup`, and `xargs` and `sudo` with their options;
   - `nice`, and `timeout` with its duration.

   A `{` is skipped like a keyword, so `{ eval "$C"; }` and `{ bash -c "$C"; }` are refused, and
   `dash`, `ksh` and `mksh` are shells beside `sh`, `bash` and `zsh`, matched by basename
   (`/bin/dash -c "$C"`) (#969's review).

   `stdbuf`, with its options, and `builtin` are wrappers too, and `time` is skipped as a keyword,
   so `stdbuf -o0 sh -c "$C"`, `builtin eval "$C"` and `time sh -c "$C"` are refused. Pins also
   name forms the wrappers above already cover: `env -- bash -c "$C"`, `exec -a name sh -c "$C"`,
   a bare `nice sh -c "$C"` and a cluster `bash -lc "$C"` (#970's review, X1 tests correction 11).

   `.` or `source` is refused too when it reads a process substitution (`<( … )`) or a
   here-string whose text holds `$` or a backtick.

   These stay allowed:
   - a shell given a script file, with or without expanding arguments, wrapped or not
     (`sudo -u x ./script.sh`);
   - `.` or `source` of a file path, even one a substitution builds
     (`. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"`, as `deploy/` does);
   - an `eval` or a `-c` whose argument expands nothing.

   No line of `.github/`, `.cargo/` or `deploy/` uses any of these forms.
2. **A command word that is solely an expansion is read through its variable** (step 3c, beside
   arrays). The variable must be assigned in the same file, with `C=…` or `C+=…`.
   - Its definitions are concatenated in order, as 3c reads an array's.
   - The command is refused if that value builds or runs `live`, or if a definition cannot be read
     statically.
   - This rule does not refuse expansions of variables the file does not define:
     - `"$@"` and positional parameters;
     - environment inputs;
     - `"${VAULT_IMPORT[@]}"`, which is left to 3c's array reading through its definition.

   A variable that `read` sets, and an array that `mapfile` or `readarray` sets, counts as a
   definition whose value cannot be read statically (the coordinator's ruling on #970, a
   tightening). So `read -r C < f; $C` and `while read -r C; do $C; done < f` are refused,
   exactly where any other unreadable definition is: as the real command word. `echo "$C"`
   after such a `read` stays allowed.

   `export`, `declare`, `local`, `readonly` and `typeset` define too, with their options skipped:
   a readable value is read through, so `export C=ca; C+=rgo …; $C` is refused for its value, and
   an unreadable one (`export C=$(cat f)`) counts as unreadable. `printf -v C …` sets a value
   that cannot be read. A wrapper of item 1 before a bare variable command word is skipped, so
   `env $C`, `sudo $C` and `xargs $C` are read as `$C` alone (#970's review, X1 tests
   correction 11, a tightening).

   Undefined arrays stay refused, unlike undefined scalar variables: a command word expanding a
   whole array (`"${x[@]}"`) that the file never defines is refused, as the pending
   `array_expansions_are_read_through_their_definitions` already pins. That only tightens, and
   real CI defines its arrays (`VAULT_IMPORT` at `deploy/set-secrets.sh:29`).

   The reviewer's cross-line `C=ca; C+=rgo …; $C` example is 3c's pinned case.
3. **A disclosed residual of items 1 and 2.** Where 3b re-reads a quoted word (`echo "$C" | sh`), a
   readable definition is read through, but an unreadable one is not refused. So
   `C=$(cat f); echo "$C" | sh` passes the word scan. Refusing unreadable definitions in
   re-reads would refuse about 40 real lines of `.github/scripts/` and `deploy/`, such as
   `case "$unit" in` and `install -o "$ADMIN_USER" …`. The coordinator accepted this residual as
   disclosed. What still stands against it: X1's build-file and live-feature checks (no crate
   may declare or turn on `live` but the marked runner, and only through the one compile-only
   form CI may run), and review.
4. **A known quirk is left alone** (resolved by item 6: a key inside a job's `outputs:` block is
   no longer read as a `run:` step). `block_lines(text, "run")` also reads a job's `outputs: run:`
   key as a `run:` value, as on `nightly.yml:32`. No rule refuses a top-level `${{ … }}` command
   word on its own, so it changes no verdict.
5. **Disclosed residuals outside the enumerated forms** (#976's review, step 3c). Items 1 and 2
   name their wrappers, shells and defining builtins one by one, and these forms are outside that
   list, so the word scan does not read through them:
   - `time -p` (only a bare `time` was skipped as a keyword; step 3c-2 skips its `-p`, #994's
     review);
   - the wrappers `setsid`, `flock`, `ionice`, `taskset`, `unbuffer`, `doas`, `su -c` and
     `runuser`;
   - `env -S`, which splits its string into a new command line;
   - a dynamic `printf -v "$N"`, whose target variable is itself an expansion;
   - the wrappers `chronic`, `nsenter`, `watch`, `parallel` and `find -exec`, and `coproc`
     (#979's review). Rule 1′ does not see through them either: a wrapper it does not model is
     itself the command word, so `chronic $C` passes the word scan (corrected in step 3c-2; #985
     said the opposite);
   - a shell fed from a file rather than a pipeline (`echo $C > f; sh f`): item 6's rule 2
     judges pipelines only (#985's review); the process-substitution form (`sh < <(echo $C)`) is
     closed in step 3c-2b. Its sibling minor, a
     `"$(dirname "$0")/../x.sh"` that the allowlist let through, is closed in step 3c-2: a `..`
     segment takes the path off the allowlist, and no real line uses one.

   The coordinator accepted them as disclosed, with a backlog row under E7-26 to close them.

   What still stands behind them: X1's build-file and live-feature checks (no crate may declare
   or turn on `live` but the marked runner, and only through the one compile-only form CI may
   run), and review. Step 3c reads the build files (`Makefile`, `*.mk`, `justfile`,
   `Dockerfile*`, `docker-compose*.yml`) anywhere in the repository; the walk skips only what is
   not the repository's own: `.git`, `node_modules`, and directories a `CACHEDIR.TAG` marks as a
   cache, as cargo's target directories are.

   #994's review (X1 tests correction 12d) named sinks outside item 6's lists: `setsid`, `flock`
   or `busybox` running a shell, a subshell `( sh )`, `source /dev/stdin`, `ash`, `fish`, `csh`,
   `python3` and `perl`. Item 6's sink allowlist (X1 tests correction 12e) closes each of them as
   the sink of a pipeline whose line expands. Still disclosed: the unquoted
   `$(dirname $0)/<path>`, which the allowlist takes like the quoted form; a `deploy/` function
   named `put_file` or `sql` whose body runs a shell, since the sink allowlist takes those two by
   name. As residuals of a line-based scanner (#1015's review, the fourteenth round): data that
   flows through a file (`echo $C > x.sh; sh x.sh`); an exact-line entry of item 6, which trusts
   the earlier lines of its file that set the variables it reads; and `env -S`, listed above.
   Two more, reachable only on purpose (#1015's review): a GNU `sort` long option abbreviated
   (`sort --comp=sh`), which the `--compress-program` check does not read; and `awk` or `sed` as a
   pipeline's first command with a program that expands, which rule 1′ does not judge since
   their command word is literal.

6. **A command word that starts with an expansion fails closed** (X1 tests correction 12b, after
   #979's review; the coordinator's ruling, a tightening under DEC-176). Eleven rounds of pinned
   forms kept leaking: `${C:-}`, `${C:0}`, `${C//zz/}`, `${C@E}`, `${!n}`, a nameref
   (`declare -n R=C; $R`), `set -- $C; "$@"` and an unquoted `echo $C | sh` all passed item 2.
   So the rule turns from a denylist to an allowlist:
   - **Rule 1′.** A command word, after item 1's wrappers and the keywords, whose first character
     (its quotes removed) is `$` or a backtick is refused, outside a quoted word 3b re-reads and,
     in a workflow, outside a `${{ … }}` of a line that is not a `run:` step. Only these pass:
     `"$@"`; a sole `$C` or `${C}`, or a whole array `"${x[@]}"`/`"${x[*]}"`, whose definitions in
     the file are all readable and hold neither `cargo` nor `live`; `"$(dirname "$0")/<literal
     path>"`; and `"${ARR[<digits>]}"` of such an array. Item 2's exemption for a variable the file
     does not define (an environment input such as `$TOOL`) is superseded: it is refused, because
     a workflow's `env:` could supply `cargo build --features live`.
   - **Rule 2.** A pipeline that feeds `sh`, `bash`, `dash`, `ksh`, `zsh` or `mksh`, by any path,
     is refused when an earlier command's words hold `$` or a backtick, quoted or not.
   - `declare -n`, `local -n` and `typeset -n` make their variable unreadable, and a `set` with an
     expanding argument is refused in a file that runs `"$@"` as a command word.
   - Four parser fixes keep real lines allowed: `$(( … ))` is read whole as arithmetic, its
     substitutions still read (as a command word it is refused like any expansion, a tightening
     of the ruling's exemption that no real line needs); an escaped `\$` or backtick expands
     nothing; a logical line runs on while a double quote is open (a single-quoted span is
     already read by the here-doc and text reading); and a key inside a job's `outputs:` is not a
     `run:` step (item 4).
   - A file the check cannot read as text (a dangling symlink, a build file that is not UTF-8) is
     reported as a problem naming its path instead of aborting the check.

   Measured on a prototype before pinning, the rule refuses no line of `.github/`, `.cargo/` or
   `deploy/`. The scan's remaining parser limits (here-doc bodies fed to commands other than a
   shell, `case` arms, `[[ =~ ]]` parentheses, and single-quoted strings outside the text reading)
   are a backlog row under E7-26; rule 1′ does not depend on them.

   **The thirteenth round** (#994's review, X1 tests correction 12e; the coordinator's rulings,
   tightenings under DEC-176). A denylist of shells and wrappers kept leaking: a comment that
   swallows the sink (`echo $C | # c` then `sh`), descriptor forms (`time -p 2>&1 $C`,
   `{fd}>&1 $C`) and compound sources (`{ echo $C; } | sh`). So the class is closed by structure:
   - **Rule 2 becomes a sink allowlist.** On a logical line holding `$` or a backtick, every
     pipeline sink, after its wrappers, must be `sha256sum`, `tee`, `tr`, `sort`, `grep`, `cut`,
     `tail`, `paste`, `cat`, `head`, `uniq`, `wc` or `read`, or, in a `deploy/` file only,
     `put_file` or `sql`: the sinks the repository's scripts use. Any other sink, a variable and
     an empty sink are refused. (The thirteenth round also listed `awk` and `sed`, said their
     executing forms were refused wherever they stood, and said a compound command's shell needed
     no case of its own; #1015's review showed all three false, and the fourteenth round below
     corrects them.)
   - **The command word is found by structure.** Every unquoted word of a redirection before it
     (`2>&1`, `{fd}>&1`, `&>x`, `<&0`) is skipped, and each wrapper, matched by basename
     (`/usr/bin/time` too), has its options read by arity, `--` included; an option whose arity
     is not known is refused. Rule 1′ then judges the word reached.
   - **An interim exact-line list** names, by file and trimmed text, the lines these rules refuse
     only because the scan reads them wrongly: `deploy/set-secrets.sh`'s `systemd-run` pipeline
     and its `[[ "${line%%=*}" =~ (KEY|SECRET|TOKEN|PASSWORD) ]]` test. Any other such line is
     refused until a person adds it. The tokenizer work is backlog story E7-28.
   - **The fourteenth round** (#1015's review; the coordinator's rulings, tightenings under
     DEC-176). On a line that expands, a sink must be one simple command: a sink holding `{` or a
     loop, `if` or `case` keyword is refused (`echo $C | { read x; sh; }`). `awk` and `sed` leave
     the allowlist, since they execute in more forms than a scan reads (`sed 1e`, `sed -f
     /dev/stdin`, `awk '{ system ($0) }'`, `awk '{ print | "sh" }'`); the repository's six lines
     that feed them go on the exact-line list, as do its one `while read` loop and its two
     `done < <( … )` loops. `sort --compress-program` is refused. A process substitution (`<(`,
     `>(`) is refused on a line that expands or runs a shell (`tee >(sh)`, `sh <(echo $C)`).
   - **A shape rule was considered and not adopted.** Requiring the command word to match
     `[A-Za-z0-9_./-]+` refused about 180 real lines (`[`, `}`, `case` arm patterns, fragments of
     multi-line awk and jq programs). Judging a command word's shape needs a full shell parser,
     which the structural skip and the sink allowlist do not.

   Measured on a prototype, these rules refuse no real line beyond the eleven the exact-line list
   names.

   **The threat model.** X1 is defence in depth against an accident or a careless change: a CI
   line that would build or run `live` by mistake. It is not a sandbox against an author who
   means to get past it; a line-based scan of a shell cannot be. Review and the live-token
   backstop remain the controls for that. A stronger, artifact-level check is noted in backlog
   story E7-28: assert from `cargo metadata` and the build plan that no CI job resolves the
   `live` feature.

### Rationale

- Each item only refuses more (DEC-176). Item 1 refuses no line the repository holds, and item 2
  refuses only through a definition the file itself holds.
- The forms item 1 names are the ones that run a string as a command line. Refusing them where the
  string expands closes the reviewer's `eval "$C"`, `sh -c "$C"` and `bash <<< "$C"` without
  reading variables.
- A bare `$C` cannot be judged without knowing `C`. Reading the file's own definitions, as 3c reads
  arrays, judges it without refusing the deploy scripts' wrappers.

### Alternatives considered

- **Refuse every command word that is solely an expansion.** Rejected: it refuses real deploy and
  workflow lines.
- **Change the deploy scripts to avoid `"$@"` and arrays.** Rejected: the check follows the
  repository, not the other way round.

<a id="dec-852"></a>

## DEC-852: a mutation shard runs its mutated package's tests first

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79: a reversible change to the gate's configuration that judges more mutants and lowers, raises, or skips nothing; CI and xtask are founder-owned in `CODEOWNERS`, so the founder may overturn it) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The DEC-852 builder (agent), under DEC-79 |
| **Story or claim** | Found on [#931](https://github.com/kunwarshivam/mandate/pull/931)'s shard 13 after [DEC-850](earlier-600-899.md#dec-850); DEC-498's shard budget; DEC-464's ten-minute limit |

### Decision

1. **Every test phase of the mutants run starts the mutated packages' tests first.** xtask writes
   a nextest tool configuration with one override, `filter = "package(=<p>) | …"` over the
   packages the run's own mutants are in, and `priority = 100`, nextest's highest
   (`test_order_config`). It hands the file to every test phase, the baseline's included, with
   `--cargo-test-arg=--tool-config-file --cargo-test-arg=mandate-mutants:<file>` (`TestOrder`).
   cargo-mutants 27.1.0 passes `--cargo-test-arg` to test phases only. Nextest 0.9.146 honours
   `priority` both in a repository's `.config/nextest.toml` and in a `--tool-config-file`. Both
   were checked by command on a two-crate workspace in which the override made the
   alphabetically later crate's tests start first.
2. **A shard's packages are its own slice's.** For a shard, `shard_packages` asks
   `cargo mutants --list --json` with the same `--shard` and `--sharding slice` the run passes, so
   the selection is cargo-mutants' own. Unsharded (`cargo xtask check`), every package with a
   mutant goes first. A slice with no mutant writes nothing, since cargo-mutants ends it before
   any test.
3. **It only reorders.** The override has no other key, so no test is filtered, retried, or timed
   differently. Every `--test-package` still runs. A mutant the mutated crate's own tests miss is
   still judged by every other tested package, within the same 180-second cap. If they cannot
   finish within it, the result is the existing `TIMEOUT`, which `mutants_outcome` refuses. No cap,
   shard count, package, or mutant changes. DEC-850 stands as written.
4. **Three tests hold it.**
   - `a_shard_runs_its_mutated_packages_tests_first` parses the configuration and requires one
     override with exactly `filter` and `priority`, the exact-name filter, and priority 100. It
     also requires both arguments once, and the file removed with the run.
   - `the_planned_shards_test_the_gates_mutants_and_no_more_per_shard` requires that each of a
     fixture's one-mutant shards puts exactly one package first. Between them the shards must
     cover every mutated package.
   - `the_job_refuses_a_crate_whose_mutants_no_live_test_would_judge` reads cargo-mutants' own
     scenario logs from a real run and requires `--tool-config-file mandate-mutants:` on every
     test phase.

   Each was shown to fail on a seeded bug (below).

### Rationale

#### The failure

Nextest starts tests in the order of their binaries' names, then their tests' names. A shard
tests every package the diff touches plus the reference harness (DEC-497). So a mutant in a
package whose name sorts late waits for every earlier package's tests before its own start.
#931's tested packages are `mandate-alpaca`, `mandate-executor`, `mandate-refcases` and
`mandate-shell`. Its `mandate-shell` mutant, `adapters.rs:1891:9` `use_profile -> Ok(())`, is
caught by exactly one test, `adapters::tests::core_executor_keeps_its_fold_and_takes_the_profile_it_is_handed`.
That test fails in 9 milliseconds.

On a throwaway branch (#931's head with the mutant planted, `fast` replaced by a nextest run
over the shard's four packages, run
[37892141696](https://github.com/kunwarshivam/mandate/actions/runs/37892141696)):

| nextest over the four packages | Result |
|---|---|
| fail-fast, as cargo-mutants runs it | the one failing test finishes 1529th of 1749, the run cancelled at **212.8 s** |
| `--no-fail-fast` | the same single failure; the whole suite **203.2 s** |

On the mutants shards the same catch came at 173 seconds of the 180-second cap
([37888679198](https://github.com/kunwarshivam/mandate/actions/runs/37888679198)). On run
37885302040's first attempt it came too late, as `TIMEOUT` at 180. So whether a late-caught
mutant passed depended on the runner's speed. A mutant no test caught at all would have reported
`TIMEOUT` rather than `MISSED`. That still failed closed, but for the wrong reason, and it named
no survivor.

#### The measurement, with this change

Run [37894224210](https://github.com/kunwarshivam/mandate/actions/runs/37894224210), by
`workflow_dispatch` on `claude/mutants-test-order-measure`: #931's head `12bee907`, DEC-850's
change, this one, and the measurement-only `--caught`. Its diff and sixteen mutants are #931's.
All sixteen shards, `full-checks` and `full` passed.

| Shard | Mutant | Before (37888679198, DEC-850 alone) | After |
|---|---|---|---|
| 13/16 | `adapters.rs:1891:9` (`mandate-shell`) | caught, 6s build + **173s test**; job 318s | caught, 6s build + **1s test**; job **155s** |
| 7/16 | `shape.rs:62:55` (`mandate-executor`) | caught, 24s build + 33s test; job 308s | caught, 22s build + 29s test; job 304s |

Every shard took 135 to 304 seconds, where 131 to 318 was the range before.

#### The budget

DEC-498's equation is unchanged, with `k = 1`:

```
k × (60 + 180) + 180 ≤ 600 − remainder      so the remainder has 180 seconds
```

The remainder is the job less its three capped phases:

| Shard | Job | Baseline test | Mutant build + test | Remainder |
|---|---|---|---|---|
| 13/16 | 155 | 4 | 6 + 1 | **144** |
| 7/16 | 304 | 117 | 22 + 29 | **136** |

DEC-850 measured 127 to 136. The extra `cargo mutants --list` for the shard is about 1 to 2
seconds, the same as the listing the plan already makes. The remainder stays inside the 180
reserved. The capped phases themselves are unchanged in bound. In practice they got shorter
wherever the mutated package's own tests catch the mutant.

What the order cannot do is make the tested set's whole suite fit 180 seconds. On #931 that
suite runs for about 203 seconds on the `fast` runner. A mutant that only the last tests reach,
or that no test catches, still reaches the cap and is refused as `TIMEOUT`. That is the existing,
loud, fail-closed outcome. Bringing the suite under the cap is a lever on the capped phase,
recorded as E1-6 (a cheaper reference harness), not something this decision claims.

#### Seeded bugs

Each was planted alone in `xtask/src/main.rs`, and the named test failed, then it was reverted:

| Seeded bug | Test that failed |
|---|---|
| the order's two arguments not added to the run | `a_shard_runs_its_mutated_packages_tests_first`, `the_job_refuses_a_crate_whose_mutants_no_live_test_would_judge` |
| `shard_packages` ignores the shard (every mutated package first) | `the_planned_shards_test_the_gates_mutants_and_no_more_per_shard` |
| `priority = 0` | `a_shard_runs_its_mutated_packages_tests_first` |
| `package(<p>)` (a substring match) for `package(=<p>)` | the same |
| `mutants_scheduled` passes no order to `mutants_args` | `the_job_refuses_a_crate_whose_mutants_no_live_test_would_judge` |

### Alternatives considered

- **Raise the test cap.** It would buy margin from DEC-498's capped seconds for a cost that is
  ordering, not work. Refused, as DEC-498 item 5 requires.
- **Narrow `--test-package` to the mutated package.** It would drop the external oracles
  (DEC-497) and the other touched crates from judging a mutant. A weakening. Refused.
- **A static `.config/nextest.toml`.** It would reorder every nextest run in the repository, and
  it cannot name a shard's own package.
- **Prioritize every mutated package of the diff in every shard.** On #931 that puts
  `mandate-executor`'s two minutes ahead of shard 13's `mandate-shell` tests again. The shard's
  own slice is the selection that helps.
- **A tests-correction PR.** The one test that catches the mutant already fails in 9 ms. No test
  in `mandate-shell` can run earlier under nextest's order, and no package that sorts earlier
  may depend on `mandate-shell`.

<a id="dec-854"></a>

## DEC-854: How the order builder intersects the opening policy with the broker's profile (B3)

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79; each reading below is the one that refuses more and adds no risk, DEC-176) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | An agent (Claude Code, the B3 tests builder), under DEC-79 and DEC-176 |
| **Story or claim** | E7-23; [first live trade brief](../tasks/first-live-trade.md) row B3, LT-3; claim [#857](https://github.com/kunwarshivam/mandate/issues/857); [DEC-529](DEC-529.md) item 2, [DEC-531](DEC-531.md) items 2 and 3, [DEC-630](DEC-630.md) |

Trading spec §5.2 says the builder "sizes to the quantity form the profile allows for the order
type the policy chose" and that "an empty intersection is a refusal before the intent, never a
fallback to another order type". It does not say which quantity forms the policy allows, which
row is read, whether the time in force takes part, or what an empty intersection does to an exit.
These are the readings B3's tests pin.

### Decision

1. **The policy's side.** An opening or increasing order is a limit order (§5.1) sized as a
   quantity: whole or fractional, **never notional**. The builder proposes a quantity and a limit
   price, §5.1 uses no notional buys, and §5.3 rule 1 keeps quantity and notional apart. A US
   equity opens only in the regular session (§5.1), even where a profile declares another
   session's row: the profile can narrow the policy, never widen it (LT-3). The asset class is
   read here for that platform rule only, never to learn a broker rule (LT-2).
2. **The profile's side.** The row read is the market's own asset class and session. A form is
   offered only when that row's `limit` cell for the form exists **and lists the time in force
   the opening will carry**. The caller states the time in force, since the builder does not choose
   it. A missing row or cell offers nothing (DEC-630 item 9).
3. **The form.** Fractional when offered, so a profile that allows it sizes exactly as today
   (LT-14). Otherwise whole: the budget is truncated to whole shares, or to the market's own
   increment where that is coarser than one share. Otherwise the intersection is empty.
4. **An empty intersection refuses the buy, and only the buy.** It is `no_opening_form`, and
   nothing is proposed. A discretionary exit and a hold are exactly what §8.3 gives without a
   profile. A profile is never a reason an exit did not go out (`AGENTS.md` rule 13).
5. **One share or nothing (DEC-529 item 2).** §8.3 step 3's budget, the least of Delta,
   `max_order_usd`, cap − MV − working and the gross room, at the ask, is truncated to whole
   shares. When that truncation leaves zero, the step's existing hold applies and nothing is
   sent. The builder's share of DEC-529 item 2 is those limits at the ask. The collar, the fee
   reservation, and the eligibility floor's price and liquidity floors stay the gate's
   (DEC-130 item 17). They are checked again there on the order the builder proposes, so no rule
   is relaxed.
6. **Deployment.** For each asset class the mandate allows, deployment states the time in force
   its openings carry. Every class must have a non-empty intersection in its opening session: the
   regular session for US equities and the crypto session for crypto. Any class with an empty
   intersection refuses the deployment `no_opening_form`, and so does an empty list.

### Rationale

Each reading refuses where the spec is silent, and none makes an order more likely to be sent.
Reading the time in force keeps an order the broker would refuse from reaching it: on Alpaca a
`gtc` fractional limit is not offered (§5.2), so an opening sent `gtc` is sized in whole shares.
Keeping fractional first leaves every Alpaca `day` opening unchanged.

**A consequence for the wiring, not for this decision:** Alpaca's profile declares no crypto
`limit` cell yet (DEC-531 item 6, B2a). So item 6 would refuse an Alpaca crypto mandate until
`mandate-alpaca` declares §5.2's crypto limit cells. Nothing calls the deployment check or the
profile-aware builder yet. The story that wires them must add those cells first, or LT-14 breaks.

### Alternatives considered

- **Leave the time in force out of the intersection.** Rejected: the cell is keyed by time in
  force as well (DEC-630 item 1), and an order valid in type and form but not in time in force is
  still one the broker refuses.
- **Prefer whole shares everywhere.** Rejected: that changes Alpaca's fractional sizing (LT-14).
- **Refuse the whole evaluation on an empty intersection.** Rejected: that would let a profile stop
  a discretionary exit (rule 13).
- **Put the fee reservation into §8.3's one-share check.** Rejected for B3: it is a mandate spec
  change, and the gate already reserves the fee on the proposed order.

<a id="dec-855"></a>

## DEC-855: The founder's OAuth login uses fresh PKCE, a single-use exact `state`, and a strict callback (O1b)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-24 lane, under DEC-176. Each item reads the published flow the way that accepts less, and none adds risk. The founder approves the `getrandom` row (F-6) |
| **Story or claim** | E7-24 (the first live trade brief's O1b); claim #859; the O1b tests PRs |

### Decision

1. **One callback per login.** `callback` takes the login by value, not `Clone`, so any callback
   spends it (DEC-691 item 6); `compile_fail` doctests pin both.
2. **`state` exact, and first.** `state` must appear exactly once and, percent-decoded, equal the
   login's own byte for byte, or the callback is `state_mismatch`; nothing else is read before.
3. **The callback.** The path is exactly `/callback`, and `code` appears once as visible ASCII
   (no space, as DEC-847 item 6), else `malformed`; `error` is `authorization_denied`, unread.
   No error carries the state, the code or the target's text (LT-9, CN-9).
4. **`iss` (RFC 9207).** Checked after `state` and before `error`: at most once, byte-equal to
   `AuthServer::issuer`, else `issuer_mismatch`. A missing `iss` is accepted, even when the server
   sets `authorization_response_iss_parameter_supported`: there is one pinned server (DEC-847
   item 2), so no mix-up. Discovery does not read that flag; a follow-up row should require `iss`
   when it is advertised.
5. **Fresh secrets.** Verifier and `state`: 32 OS bytes each (`getrandom`), 43-character base64url,
   or `random_unavailable`. `S256` via `mandate-canon`'s `Digest`. Secrets are `SecretString`s.
6. **The request** carries exactly `response_type`, `client_id`, `redirect_uri`,
   `code_challenge`, `code_challenge_method`, `state` and `resource` (RFC 8707), and no `scope`
   (none published; R0 needing one is a new decision). An endpoint query is `endpoint_shape`.
7. **Two parts.** Part 2 adds the loopback listener (`tokio`'s `net`), the code exchange, the
   token, LT-9's canary scan, and the login's lifetime (DEC-691 item 6: expiry spends a `state`).

### Rationale

Every refusal ends the login before a token exists (rule 3); `state` first is RFC 6749 §10.12.
Rejected: a `spent` flag (weaker than the type); `rustls`'s RNG (the brief names `getrandom`).

<a id="dec-861"></a>

## DEC-861: The founder's OAuth login reads one callback on 127.0.0.1, lives 600 seconds, and keeps only the token (O1b part 2)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-24 lane, under DEC-176: each item reads the published flow the way that accepts less, and none adds risk. The founder approves the `tokio` `net` line in `docs/dependencies.md` (F-6) |
| **Story or claim** | E7-24 (the first live trade brief's O1b); claim #859; DEC-855 item 7 |

### Decision

1. **The listener** binds `127.0.0.1:0` only. `accept` takes it by value: one request, then closed.
2. **The request** line is `GET <target> HTTP/1.1`, its head (blank line included) complete within
   8192 bytes, else `malformed`. The browser gets `200` or `400` with fixed text naming no state,
   code or reason. Any request spends the login (DEC-855 item 1).
3. **The lifetime** starts when the login began. At or after 600 s a request is `login_expired`,
   its target unread (DEC-691 item 6). The listener stays open past 600 s until a request
   arrives: loopback-only, failing closed (that request is `login_expired`), holding no token.
4. **The exchange** is one form `POST` of exactly `grant_type`, `code`, `redirect_uri`,
   `client_id`, `code_verifier` and `resource`, with no `authorization` header. Only a `200` with a
   visible-ASCII `access_token` and a `Bearer` `token_type`, in any case, is a token, else
   `malformed`. `refresh_token` is never read. `AccessToken` is a `SecretString` (LT-9).

### Rationale

Each refusal ends the login before a token is kept (rule 3); one request leaves nothing to replay.

### Alternatives considered

A listener that closes at 600 s: it adds a timer and a second refusal path, and the open one already refuses every late request.

<a id="dec-864"></a>

## DEC-864: An unreadable Robinhood cancel answer is `Unreadable`, and a lost or refused review places no opening but holds no exit (C1)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The coordinator agent, under DEC-79 and DEC-176, on the #1098 review: each reading adds no risk and none loosens a rule |
| **Story or claim** | E7-6, the [first live trade brief](../tasks/first-live-trade.md)'s C1; PRs #1097 and #1098; reads [DEC-860](DEC-860.md) item 4 |

### Decision

1. **A cancel answer.** An answer to a Robinhood cancel that the connector cannot read is
   `ConnectorError::Unreadable`, as that variant defines it: the broker answered and the
   connector could not read the answer. DEC-860 item 4's override to `Unknown` covers only a
   place answer. A lost cancel answer is `Unknown`. Neither ever reads as `CancelAccepted`, and
   the order stays live until a cancel is confirmed (trading-domain spec §5.7). `Unreadable`
   stops the shell and alerts the owner (DEC-85).
2. **The review of an opening or an increase.** A review that is lost, is `isError`, or has no
   `alerts` array places nothing.
3. **The review of a risk-reducing order.** A review never holds a risk-reducing order: an exit
   or a protective order is placed whatever its review answers, a lost or `isError` review
   included (`AGENTS.md` rule 13; connections spec §6.2).
4. **Tests.** These pin it, in `crates/mandate-rh-sim/tests/robinhood.rs` (#1098):
   - `a_lost_or_unreadable_cancel_answer_never_reads_as_cancelled`;
   - `an_is_error_that_is_not_a_boolean_is_never_success`;
   - `an_openings_review_refused_lost_or_without_alerts_places_nothing`;
   - `a_risk_reducing_order_is_placed_when_its_review_is_lost_or_refused`.

### Rationale

Both readings add no risk. An unreadable cancel answer never frees the order or its instrument,
and `Unreadable` stops the shell and brings the owner in, where `Unknown` would only wait. An
opening whose review cannot be read is not sent, so no risk is added on an unchecked answer,
while a risk-reducing order is never denied or held by its review (rules 3 and 13).

### Alternatives considered

- **`Unknown` for an unreadable cancel answer.** It contradicts `Unreadable`'s own definition,
  and DEC-860 item 4's override is written for a place answer only.
- **An absent `isError` read as failure.** Rejected: it contradicts DEC-860 item 4, under which
  a result without `isError` that carries an order record is an answer. Only a non-boolean
  `isError` is pinned as never success.

<a id="dec-866"></a>

## DEC-866: a mutation run executes a crate's simulator harness whenever it mutates that crate

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The coordinator agent, under DEC-79 and DEC-176: a reversible engineering decision that keeps the zero-missed-mutants gate unchanged and only lets a crate's own oracle judge it, as DEC-497 did for the reference suites |
| **Story or claim** | E7-6 C1, [#1097](https://github.com/kunwarshivam/mandate/pull/1097)'s mutants gate; DEC-497, DEC-124, DEC-139, DEC-464 |

### Decision

1. **A mutation run that mutates a crate also runs the test package that is its simulator
   harness.** `cargo xtask ci mutants` adds `--test-package=<harness>` beside the mutated package
   and DEC-497's `mandate-refcases`, so a connector's mutants are judged by the tests written
   against its simulator.
2. **The pairing comes from cargo's graph, not from a list.** A workspace package `S` is the
   simulator harness of `M` when:
   1. `S`'s name ends in `-sim`;
   2. no workspace package takes `S` as a normal or build dependency, so `S` is outside every
      product graph, which is where `xtask/layers.toml` places `mandate-rh-sim` (layer 10);
   3. `S` takes `M` as a direct dev-dependency, so `S`'s tests link `M`;
   4. `M` is not built into another of `S`'s workspace dev-dependencies: `M` is the top of what
      the harness drives, not plumbing under it.
3. **The whole harness package runs.** No nextest filter narrows it, as in DEC-497 item 3.
4. **A mutation of a crate no harness drives costs nothing.** The run is what it was before.
5. **Today the rule pairs exactly one crate:** `mandate-robinhood` with `mandate-rh-sim`. The
   executor, the MCP client and accounting, which `mandate-rh-sim` also dev-depends on, are built
   into `mandate-robinhood` and so are not paired (item 2.4); `mandate-sim` is a product crate that
   `mandate-backtest` and `mandate-refcases` build on, so it is no harness (item 2.2).
6. **The rule is `simulator_harnesses` and `simulator_oracles` in `xtask/src/main.rs`,** with a unit
   test over a synthetic graph, `a_mutated_connector_is_judged_by_its_simulator_harness`, and a
   test over the real graph, `the_simulator_harnesses_judge_the_connectors_they_drive`, which pins
   item 5's pairing so that a new harness or a new dev-dependency of one changes it in the same
   PR rather than silently.

### Rationale

A mutant is judged only by the tests cargo-mutants runs, and it runs the mutated package's tests
plus whatever `--test-package` names. The Robinhood connector's tests live in
`crates/mandate-rh-sim/tests/robinhood.rs`, in the simulator's package, because the simulator
depends on the connector and not the reverse (layers.toml keeps the simulator above every product
crate). So no shard ran them: on #1097, nineteen of forty-six shards each reported a
`mandate-robinhood` mutant missed, and a reviewer confirmed by hand that those tests kill the
non-equivalent ones (an `offered` check forced true, `placed` forced to `None`, the opening refusal
disabled, a refused cancel read as accepted). The gate was not wrong to fail; it was judging the
connector without its oracle. This is the same hole DEC-497 closed for the reference suites, and
the same kind of fix: the zero-missed bar stays where it is, and the tests that exist for a crate
now judge it.

The pairing is derived, for DEC-497's reason: a table naming each harness is a claim someone must
keep true, and an omitted harness fails silently: its crate's mutants are judged without it, and a
test that would catch one never runs. Conditions 2.1 to 2.3 read what the repository already says:
the `-sim` name is how the workspace names its simulators (`mandate-sim`, `mandate-rh-sim`), the
dependency direction is what `xtask/layers.toml` enforces, and the dev-dependency is what makes the
harness's tests able to fail on `M` at all.

Condition 2.4 is the one judgement. `mandate-rh-sim`'s tests link the executor, the MCP client and
accounting too, and DEC-497's logic alone would run them for every mutant of those crates and,
through the closure, of everything under them. That would add the harness's build and tests to
most core-crate shards, against DEC-464's ten-minute budget, to judge crates the harness was not
written to judge: those crates have their own tests and DEC-497's suites. Keeping only the top of
the harness's dev-dependencies pairs a simulator with the connector it simulates the broker for
and nothing else, which is item 5's result.

### Alternatives considered

- **A const table, `mandate-robinhood` → `mandate-rh-sim`.** One line today, but the next
  simulator has to remember to add itself, and forgetting it fails silently. Refused for DEC-497's
  reason; the real-graph test makes any change to the derived pairing visible instead.
- **Run every package whose closure includes the mutated crate.** Sound, and the most oracles, but
  it would run nearly the whole workspace for a core-crate mutant, which no shard fits in ten
  minutes.
- **Pair every direct dev-dependency of the harness.** Adds `mandate-rh-sim` to every executor,
  MCP and accounting mutant for little gain; refused under item 2.4's reasoning.
- **Move the connector's tests into `mandate-robinhood`.** Would need the connector to dev-depend
  on its simulator and split the simulator's suites across two packages; #1098 placed the tests
  where they are, and moving them is not this gate's business.
- **Exempt the missed mutants.** Weakens DEC-79's zero-missed rule on a safety-critical path;
  reserved for the founder and not needed.

<a id="dec-867"></a>

## DEC-867: A connector's `CancelAccepted` means the broker shows the order canceled

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The coordinator agent, under DEC-79 and DEC-176: the reading only tightens what a connector may report, and changes no executor behaviour |
| **Story or claim** | E7-6 (the [first live trade brief](../tasks/first-live-trade.md)'s C1, the Robinhood connector, PR #1097), and the Alpaca connector's cancel (E7-2 to E7-4, the [executor and connector brief](../tasks/M6-K-executor-and-connector.md); feature map `idempotent-executor-and-broker-connector`). PRs: the Robinhood tests correction (`claude/live-c1-tests-5`), #1097, the Alpaca tests (`claude/alpaca-cancel-confirm-tests`) and the Alpaca fix (`claude/alpaca-cancel-confirm`) |

### Decision

1. **What `CancelAccepted` means.** `BrokerOutcome::CancelAccepted` means the broker confirmed
   that the order is canceled: trading-domain spec §5.7's `PendingCancel --> Canceled: confirmed`.
   The executor's existing fold stands: it folds `CancelAccepted` as `Canceled` with the cancel
   confirmed, which releases the reservation and lets a "cancel, confirm, submit" sequence go on.
2. **When a connector may answer it.** A connector answers `CancelAccepted` only when the broker's
   own answer shows the order canceled.
   - A cancel request the broker took whose order it does not yet show canceled is answered with
     the order's observed state as `BrokerOutcome::Order(..)`, through the connector's existing
     state mapping: Alpaca's `pending_cancel` is `PendingCancel`, and Robinhood's states read as
     connections spec §6.2 says (`new`, `queued`, `confirmed`, `unconfirmed` working,
     `partially_filled` working with its fill, `filled` filled). The executor then waits for the
     broker's own canceled state, with the cancel still unconfirmed.
   - A lost answer stays `Unknown`, and an unreadable one stays `Unreadable`
     ([DEC-864](earlier-600-899.md#dec-864)).
3. **Alpaca.** After a 2xx `DELETE`, the connector reads the order back once by client order id,
   as its refused-`DELETE` path already does, and answers what that read says. A failed read-back
   is `Unknown`, never `CancelAccepted`.
4. **The type says it.** `CancelAccepted` gets a doc comment in
   `crates/mandate-executor/src/types.rs` stating item 1, in the Alpaca fix PR.
5. **Tests.** `crates/mandate-rh-sim/tests/robinhood.rs` pins item 2 for Robinhood
   (`a_cancel_answered_with_an_order_not_cancelled_is_that_order_never_cancel_accepted`,
   `a_cancel_answered_with_the_order_cancelled_is_cancel_accepted`), and
   `crates/mandate-alpaca/tests/hand.rs` pins items 2 and 3 for Alpaca.

### Rationale

The executor folds `CancelAccepted` as a confirmed cancel (§5.7), but Alpaca's client answered it
for any 2xx `DELETE`, against its own doc ("**not** a confirmation"), and #1097's Robinhood
connector answered it for any readable answer. An Alpaca order can sit in `pending_cancel` and
still fill, so an order only pending cancellation was treated as canceled: its reservation was
released, and a protective exit's "cancel, confirm, submit" could send a sell while the old
protective sell was still live. That is an oversell or a short, against `AGENTS.md` rule 12 and
trading spec §5.4 ("nothing is submitted in the instrument while a cancel is unconfirmed").

Keeping the meaning in the type and narrowing the connectors changes no executor code. An order
answered as `Order(..)` keeps `cancel_unconfirmed` set until a terminal state arrives, and the
executor's existing bound on an outstanding cancel queries the order (DEC-160 (18)), so no exit
waits on a confirmation that never comes. The reading adds no risk: it only reports fewer
confirmations.

### Alternatives considered

- **Make `CancelAccepted` mean "request accepted" and change the executor's fold.** It would
  touch every executor path that folds it (`step.rs`, `protection.rs`, `kill.rs`,
  `reconcile.rs`) and widen a safety-critical change for no gain: the connectors already know
  the order's state, or can read it once.
- **Keep the Alpaca answer and rely on the streamed update.** A lost or late update would leave
  the reservation released and the exit sent against a live order.

<a id="dec-868"></a>

## DEC-868: The E7-28 resolved-feature check refuses `--all-features`, any live-only edge and an unresolvable build

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-28 tests builder, as readings that only tighten DEC-529 item 3 and DEC-851 item 6 (DEC-176) |
| **Story or claim** | E7-28 (the artifact-level `live` check DEC-851 item 6 names), claim #857, branch `claude/live-e7-28-tests` |

### Decision

The check reads what `cargo metadata` resolves for the default build and for each cargo invocation
of each workflow job. Where the first live trade brief's X1 row and DEC-851 do not settle a case:

1. **`--all-features` is refused in any job, whatever it resolves**, even while no crate declares
   `live`. A resolve-only reading would pass it today and fail it silently the day the runner
   declares `live`.
2. **A live-only package (one the runner's `live` brings in as `dep:<name>`) is refused in any build
   but the one compile-only job, through a dependency of any kind**, dev and build included, not
   only a normal one: `cargo test` builds dev-dependencies, and LT-1 bars a test from the live
   hosts.
3. **A build the check cannot resolve is a problem, not a pass**: a resolve that fails, an
   invocation word the shell expands, or a default build cargo cannot resolve is reported naming
   its job, as DEC-851 item 6 reports a file it cannot read.

### Rationale

Each reading refuses more than its alternative and refuses nothing the repository holds: no workflow
passes `--all-features` or `live`, no crate declares `live`, and every job's invocation is literal.

### Alternatives considered

Judging only the resolve (item 1), refusing only a normal dependency (item 2), and skipping what
cannot be resolved (item 3): each lets through a build that may resolve `live`.

<a id="dec-871"></a>

## DEC-871: CI runs `fast`'s parts in parallel jobs, the nextest run in three partitions, under an aggregate `fast`

| | |
|---|---|
| **Status** | Accepted; item 2's shard count amended by [DEC-2325](trading-core.md#dec-2325): four `fast-test` partitions, the longest tests started first |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | A builder agent (Claude Code), under DEC-79: a reversible engineering decision that removes no test, check, baseline or gate and weakens no safety rule |
| **Story or claim** | DEC-464 follow-up; branch `claude/ci-fast-shards` |

### Decision

1. **The required `fast` check is an aggregate**, like `full` (DEC-538). It runs no check itself:
   it needs `fast-lint`, `fast-test`, `fast-pending` and `fast-python`, runs
   `if: ${{ always() && !github.event.pull_request.draft }}`, and passes only when every one of
   them succeeded. A failed, cancelled or skipped part fails it, and a matrix succeeds only when
   every shard did. Its name and `merge-approved.sh`'s check of it are unchanged.
2. **The parts, which together run exactly `cargo xtask ci fast`, each once:**
   - `fast-lint`: `cargo xtask ci lint`, then `cargo xtask ci spec-guard`, and on a
     documentation-only change the DEC-112 short path (`docs-checks.sh`: typos, spec guard,
     trailers), with no toolchain.
   - `fast-test (1/3)`, `(2/3)`, `(3/3)`: `cargo xtask ci test-nextest` with
     `MANDATE_TEST_PARTITION=K/3`, nextest's `--partition=count:K/3`. Each shard builds every test
     binary and runs its third of the tests; the three partitions together run every test once.
   - `fast-pending`: `cargo xtask ci pending`, then `cargo xtask ci test-doc` (the doctests). Both
     need the test profile the shards build and run in seconds once it is built.
   - `fast-python`: `cargo xtask ci test-python` (pytest), which builds only xtask.
3. **`test` has three parts in xtask:** `test-nextest`, `test-doc` and `test-python`.
   `cargo xtask ci test`, `ci fast`, `check` and the nightly run all three, unpartitioned, as
   before; only `ci test-nextest` reads `MANDATE_TEST_PARTITION`, which must be `K/N` with
   `1 <= K <= N` in plain decimal, or the job fails.
4. **Every part keeps a ten-minute timeout** (DEC-464) and the DEC-610 draft guard, and every part
   but `fast-lint` reports success on a documentation-only change.
5. **rust-cache:** `fast-lint` and `fast-python` each keep their own cache (`shared-key`). The
   three test shards and `fast-pending` build the same test profile, so they share the
   `fast-test` key, and only shard `1/3` saves it, so the other jobs neither race to upload it nor
   spend their time limit on the save.
6. **xtask's tests hold the shape:** the fast jobs' `cargo xtask ci` parts expand to exactly
   `FAST_JOB` with `test` split, each part once and in no other job; the matrix is `1/N` to `N/N`;
   `fast` needs every part; its verdict script is run over all 625 combinations of four results;
   every `ci` job carries the draft guard or follows one that does.

### Rationale

The single `fast` job had `timeout-minutes: 15` and took 13 to 14.6 minutes on Rust pull
requests, breaking DEC-464's ten-minute limit and now cancelling runs: #1180 took 14m37s, #1156
took 13m09s, and #1187 (run 37980244367) spent 14m09s in `cargo xtask ci fast` and hit the
timeout while rust-cache saved. Measured from the job logs:

| Part of `cargo xtask ci fast` | #1180 (job 113975426433) | #1156 (job 113956411473) |
|---|---|---|
| Setup before the step (checkout, tools, toolchain, cache restore) | 0m27s | 0m36s |
| Lint, then the test profile's build, before nextest starts | 4m43s | 4m50s |
| nextest run (4,934 and 4,909 tests) | 6m12s | 4m53s |
| Doctests | 4s | 4s |
| pytest (216 tests) | 2m46s | 2m23s |
| Pending tests (45 and 39) and the spec guard | 8s | 6s |
| rust-cache save | 15s | 15s |

Most of the time is the nextest run and pytest, which run one after the other behind the lint and
build phase. The longest single test takes about 47 seconds, so three counted partitions each run
roughly two minutes beside a build of about four, and pytest runs beside them on its own runner.

### Alternatives considered

- **Raise the timeout.** DEC-464 forbids it: every required check and aggregated job stays under
  ten minutes.
- **One job per `FAST_JOB` part without partitioning `test`.** `test` alone would be its build
  plus 5 to 6 minutes of nextest plus 2.5 minutes of pytest, at or over the limit.
- **Pending tests inside a test shard.** Saves one runner's build, but makes one shard different
  from the others and longer; a separate job keeps the matrix uniform.
- **`hash:` partitions.** Stable when tests are added, but less even than `count:` for the same
  set; `count:` still runs each test exactly once.

<a id="dec-872"></a>

## DEC-872: Every key the account stream names is placed by one unshared id, or in doubt

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-6 C3 tests-correction builder (Claude Code), under DEC-79 and DEC-176: each item only moves keys from placeable or cancellable to in doubt, so it refuses more and loosens no rule |
| **Story or claim** | E7-6, the [first live trade brief](../tasks/first-live-trade.md)'s C3; [DEC-860](DEC-860.md) item 4; [DEC-870](DEC-870.md) item 2; the independent review of #1200 (majors a to c, minors 2 to 4) |

### Decision

This reads [DEC-870](DEC-870.md) item 2 fail-closed where the account stream is incomplete or
inconsistent. It changes nothing for a stream that is neither.

1. **Anywhere.** A key the journal knows is every `ClientOrderId` the account stream names
   anywhere: the `client_order_id` of any `OrderSubmitted`, whether or not its other members can
   be read; the `client_order_id` of any `OrderStateChanged`; and any key such a record names in
   `replaces` or `replaced_by`. After `restore`, every such key is either placed, with one
   recoverable id and its own readable instrument and side, or in doubt. A `Submit` of a key in
   doubt is `Unknown` (`Ambiguous`) with nothing called, and its cancel is `NotSent`
   (`no_order_id`) with nothing called. So a key named only by a state change, an orphan
   successor whose `replaces` names a key never submitted, a successor named only by the old
   order's `replaced_by`, and a key whose `OrderSubmitted` has an unreadable `side` or
   `instrument_id` are each in doubt; their instrument and side are never guessed.
2. **Shared ids.** A broker id carried by the version-2 records of two or more keys maps none of
   them: a cancel by it could cancel an order another key owns, so each of those keys is in doubt.
3. **Instrument and side.** A key's instrument and side come from its own `OrderSubmitted`; only a
   successor without one takes those of the order it `replaces`, back to that chain's
   `OrderSubmitted` (DEC-870 item 2). Two `OrderSubmitted` records under one key that disagree on
   instrument or side, or of which one cannot be read, leave the key in doubt; two that agree
   exactly keep it placeable.

### Rationale

DEC-860 item 4 says a key the journal knows is never placed again, and DEC-870 item 2 that
`in_doubt` holds every other key the stream submitted. The review of #1200 found keys the stream
names that the rebuild dropped from both sets, so a later `Submit` of them was placed again, and
two readings that could cancel or check against the wrong order. Each item resolves a gap by the
reading that adds no risk: a key in doubt is escalated to the owner (DEC-85) rather than placed,
cancelled, or guessed (CN-7).

### Alternatives considered

- Skip a record that cannot be read: rejected, its key could then be placed again.
- Let the last `OrderSubmitted` or the `replaces` chain win over a key's own record: rejected, a
  cancel would be checked against an instrument and side the key may never have had.
- Let the first key to carry a shared id own it: rejected, journal order says nothing about which
  key the broker's order belongs to.

<a id="dec-873"></a>

## DEC-873: The E7-28 workflow reader is closed world: what it does not recognise cannot be read

| | |
|---|---|
| **Status** | Accepted; item 5's `CARGO_*` list extended by two names (`CARGO_PROFILE_DEV_OPT_LEVEL`, `CARGO_PROFILE_TEST_OPT_LEVEL`) by [DEC-931](earlier-900-1999.md#dec-931) |
| **Date** | 2026-10-09 (UTC); round 6 additions 2026-10-10 |
| **Decided by** | The E7-28 builder on the coordinator's round-5 ruling, and on its round-6 scope ruling, as readings that only tighten DEC-868 item 3 (DEC-176): on `main` no workflow check of this kind exists yet, so every rule here refuses more |
| **Story or claim** | E7-28 (the artifact-level `live` check), claim #857, PRs #1169 and #1234 (the round-5 and round-6 tests corrections) |

### Decision

The check that reads `.github/workflows/*.yml` for cargo builds (`resolved_live_problems`) is **a
reader of known shapes, not a proof**. Four review rounds each found shell that computes the word
`cargo` or writes cargo configuration at runtime, so the reader stops listing what it refuses and
lists what it accepts. Anything it does not positively recognise is reported as "cannot be read"
(DEC-868 item 3).

1. **Command words.** A command word is accepted only when it is a plain literal token (no `$`,
   backtick, `$'`, brace, glob character, quote or backslash) naming a program on an explicit list:
   cargo (and `cargo-<subcommand>`, `cargo.exe`), `curl`, `echo`, `printf`, `sha256sum`, `tar`
   (naming the members it extracts), `sudo install` (into exactly `/usr/local/bin/`), `rm`,
   `rustup` (exactly `show active-toolchain` and a bare `toolchain install`), `test`/`[`, `gh`
   (`api`, `pr`), `jq`, `tee`, `grep`, `cat`, `exit`, and the shell words `for`, `case`, `fi`,
   `done`, `esac`. The list is what the repository's workflows use and nothing more. A `case`
   block's pattern words are patterns, not command words. A redirection before the command word
   (`>/dev/null cargo …`, `2>/dev/null cargo …`) is skipped, with its descriptor number, so the
   word after it is judged; a YAML value that starts with `>` or `|` but is no block scalar header
   (`run: >/dev/null cargo …`) cannot be read (round 6, B1).
2. **Repository scripts.** Only `.github/scripts/docs-only.sh`, `docs-checks.sh`,
   `start-postgres.sh` and `merge-approved.sh` may be command words, each only while its whole
   text, comments included, holds no `cargo` word, no `.cargo` and no `CARGO_`. Otherwise the line
   cannot be read and the problem names the script. Any other script or `./…` command word cannot
   be read. Round 6 (B8) reads each listed script with every repository script it runs,
   transitively and under the same rules: a call is followed only as `"$(dirname "$0")/<name>"`
   or a literal `.github/scripts/<name>`, `<name>` a plain file name; any other computed or path
   command word cannot be resolved to a file and cannot be read. A script line that runs an
   interpreter, a shell, `awk`, `eval`, `source` or `.` cannot be read unless it is, byte for byte,
   one of the lines a person has read (`merge-approved.sh`'s two `awk` programs). A script may not
   write a build input (item 5), run `curl -O` or `tar`, or hold a name item 5 refuses
   (`GITHUB_ENV`, `RUSTFLAGS` and the like).
3. **npm and npx** are accepted only in a job whose `defaults.run.working-directory` is `web` or
   below it and which holds no cargo word anywhere. Round 6 (B7): working directories are resolved
   before that test (`web/..`, `web/../`, `../crates` and `web/../crates` are not under `web`), a
   step's own `working-directory` must stay under `web` too, and the directory's `package.json`
   must be one the check reads (`web/package.json`) whose text names no cargo. Within such a job
   `npm` runs only as `npm ci`, `npm run <script>` of a script that `package.json` holds, and
   `npm test` (with `--if-present`); `npx` only in the repository's three forms (the two
   Playwright installs and the pinned Impeccable detector). `npx`, `npm exec`, `npm pkg set`,
   `npm config set`, `npm install`, a missing script and a script given arguments cannot be read.
4. **Interpreters, wrappers and indirection** cannot be read: `python*`, `node`, `perl`, `ruby`,
   `awk`, shells, `source`, `.`, `eval`, `find -exec`, `env` (`-S` included), `xargs`, and every
   wrapper but `sudo install`. So can a process substitution (`<(`, `>(`), a heredoc whose consumer
   is not `cat`, a here-string whose consumer is not `jq`, and (round 6, B2) a command
   substitution, `$(` or a backtick, in the body of a heredoc whose delimiter is unquoted. A step
   or default `shell:` is read only as exactly `bash`, the one form the repository uses; one with
   any argument (`bash -c …`, `bash --rcfile x {0}`, `sh -c …`) cannot be read (round 6, B4). The exceptions are exact lines a
   person has read, matched byte for byte by file and line: `web-e2e.yml`'s `node -p` version
   probe and its `xargs -r sudo rm -f` line.
5. **Configuration written at runtime** cannot be read: a write (redirection, `tee`, `curl -o`, a
   `tar` member, `sudo install`) to anything but `$GITHUB_OUTPUT`, `$GITHUB_STEP_SUMMARY`,
   `/dev/null`, `/dev/stdout`, `/dev/stderr`, a descriptor, or a plain file name in the working
   directory; any `GITHUB_ENV`, `GITHUB_PATH` or `.cargo`; any `CARGO_HOME`; any `RUSTFLAGS`,
   `RUSTDOC*` or `RUSTC*`; and any `CARGO_*` name except exactly these five, which the workflows
   need: `CARGO_TERM_COLOR`, `CARGO_MUTANTS_VERSION`, `CARGO_MUTANTS_SHA256`,
   `CARGO_PROFILE_DEV_DEBUG` and `CARGO_PROFILE_TEST_DEBUG`. No prefixes.
   Round 6 adds:
   - **Build inputs (B5).** No writer the reader reads may write `Cargo.toml`, `Cargo.lock`,
     `rust-toolchain*`, `config.toml`, `config` or `build.rs` (in any case), the `crates` or
     `xtask` directory itself, or any path under `crates/`, `xtask/` or `.cargo`; a plain file name
     is no longer enough. `curl -O`, `--remote-name`, `--output-dir` and `-K`/`--config`, and `tar`
     with no member named, cannot be read, since their targets are not named; `cp`, `mv` and
     `install` stay off the program list.
   - **Environment names (B3).** An `env:` key, at workflow, job, step, service or container
     level, and a command-prefix assignment (`NAME=value cmd`) are read only when the name is
     exactly one the repository's workflows set (`READ_ENV_NAMES` in `xtask/src/main.rs`) or one
     of the five `CARGO_*` names. So `BASH_ENV`, `ENV`, `PATH`, `LD_PRELOAD`,
     `RUSTUP_TOOLCHAIN` and `NODE_OPTIONS` cannot be read, in any case. An inline `env:` mapping
     cannot be read. A name assigned by a command of its own (`failed=0`) or by a `for` loop must
     be on the same list or a lowercase local, which bash neither exports nor gives a meaning;
     `printf -v` cannot be read.
6. **Local `uses: ./…`** actions and reusable workflows cannot be read. Reading them recursively is
   future work. **Non-local `uses:` is allowlisted** (round 6, B6): it is read only when it names
   one of the actions the repository's workflows use, pinned exactly as they pin it
   (`READ_ACTIONS`), with `codeql.yml`'s tag-pinned CodeQL actions only in that file. Another
   action, another pin, a quoted pin, a `docker://` image and a remote reusable workflow cannot be
   read; nor can a job `container:` or a `services:` image other than `nightly.yml`'s
   digest-pinned PostgreSQL (`READ_IMAGES`).
7. Extending any list in items 1 to 5 is a code change a reviewer sees, with a doc comment naming
   the workflow line that needs it. No interpreter and no expansion form is ever added.

### The threat model and what is out of scope

DEC-851's threat model governs this reader: it is defence in depth against **an accident or a
careless change**, a CI line that would build or run `live` by mistake. It is not a sandbox
against an author who means to get past it, and a line-based reader of shell and YAML cannot be
one.

**Out of scope.** Deliberate obfuscation is out of scope: spellings built to slip past the reader,
such as the round-5 review's `c""argo` inside a string an allowed program runs, a
string-concatenated `execSync` in JavaScript, or a payload placed where an allowed program will
load it. The coordinator ruled on round 6 that the reader closes the cheap, closed gaps such a
review finds (B1 to B8) and does not chase obfuscation beyond them. Also out of scope is code
that an allowed program runs on its own: an npm package or a `package.json` lifecycle script that
`npm ci` or `npm run` executes, a downloaded and checksum-verified artifact, an action the
allowlist names, `gh`, or a repository script's behaviour beyond its text. The reader judges the
lines it can see. The controls for what it cannot see are review and the proposed defence-in-depth
build guard (a guard inside the build that refuses to compile the `live` feature outside the one
compile-only job), which stays a proposal for the founder and is not built here.

### Rationale

A deny list loses to the next spelling of `cargo` (`${C}go`, `$'\x63argo'`, `{c,}argo`, a heredoc
into `python3`, a file copied into `$GITHUB_ENV`). An allow list refuses the shapes it does not
know rather than chasing them, so a careless new line, an unknown program or an unlisted form
fails closed until a reviewer extends the list. It does not fail closed on everything: an allowed
program can do more than the reader models, as the out-of-scope paragraph above says. Each item
refuses more than round 4's reader and refuses nothing the repository's workflows hold today, once
three comments in `docs-only.sh`, `docs-checks.sh` and `base-ref.sh` stop naming cargo. Round 6's
rules each close one round-5 finding; the repository lines they would otherwise refuse are admitted
by the narrowest rule, never by weakening one: `web.yml`'s pinned `npx --yes impeccable@4.1.0
detect src/` is one of the three exact `npx` forms, and `merge-approved.sh`'s two `awk` programs
are exact script lines.

### Consequences

- A new workflow step that uses a program, an action, an image, an `env:` name, a `shell:` form or
  an `npm`/`npx` form not on its list fails `cargo xtask live-feature` until the list is extended
  in review.
- The reader is still not a proof: an allowed program can do more than the reader models (a
  downloaded archive, `gh`, a repository script's own behaviour beyond its text). **The
  defence-in-depth build guard (a guard inside the build itself that refuses to compile the `live`
  feature outside the one compile-only job) is proposed for the founder** as the answer to that
  gap. It is not built here.

### Alternatives considered

- Keep extending the deny list (rounds 1 to 4): each round found new spellings.
- Comment-blind scanning of the repository scripts: weaker, since a comment can be turned into code
  by a one-character edit the scan would not see.
- Allowlisting interpreters or `npm` everywhere: they run programs the reader cannot see.

<a id="dec-874"></a>

## DEC-874: An odd broker id or the order of the records never decides whether a key is placed

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-6 C3 tests-correction builder (Claude Code), under DEC-79 and DEC-176: items 1 and 2's tightenings, and the order-independence of items 2 and 3, which can move a key from in doubt to placed only to the reading DEC-872 item 3 already states, with the id rule unchanged and only a cancel enabled. The coordinator ruled on the second direction in #1224's review round 1 |
| **Story or claim** | E7-6, the [first live trade brief](../tasks/first-live-trade.md)'s C3; journal spec §9.16; [DEC-870](DEC-870.md) item 2; [DEC-872](earlier-600-899.md#dec-872); the three review minors #1200 and #1202 left for a follow-up; #1224 |

### Decision

This adds three readings to [DEC-872](earlier-600-899.md#dec-872), which stays as written. Each is about how
`RobinhoodConnector::restore` reads the account stream.

1. **Odd ids.** §9.16 makes `broker_order_id` non-empty text or `null`. A version-2
   `OrderStateChanged` whose `broker_order_id` is present and is neither leaves its key in doubt,
   whatever readable id the key's other records carry. That covers the empty string, a number, an
   array, an object, and a boolean. The key's cancel is `NotSent` (`no_order_id`) and a `Submit`
   of it is `Unknown` (`Ambiguous`), with nothing called. A missing member (version 1) and `null`
   still carry no id and are not odd.
2. **Inheritance is read after the whole stream.** A successor without its own `OrderSubmitted`
   takes the instrument and side its origin has once every record is read, back through the
   `replaces` chain. It never takes what the origin had when the successor's record arrived.
   - So an `OrderSubmitted` of the origin that disagrees with an earlier one leaves both the origin
     and every successor that inherits from it in doubt, wherever it falls in the stream.
   - A `replaces` chain that loops back on itself reaches no `OrderSubmitted`, so its keys are in
     doubt, and the rebuild still finishes.
3. **A key's own submit wins wherever it falls.** DEC-872 item 3 says "a key's instrument and side
   come from its own `OrderSubmitted`". That holds regardless of record order. A successor's own
   readable `OrderSubmitted` that arrives after its `OrderStateChanged` gives it its own instrument
   and side, and its cancel goes by its one id with them. An own submit that cannot be read, or two
   that disagree, still leave the key in doubt (DEC-872 item 3).

Together, three things depend only on each key's own records and their relative order, never on
how the stream interleaves different keys' records: which keys are placed, by which id, and with
which instrument and side.

### Consequences and safety

The readings move keys in both directions, and both are recorded here.

- **Towards in doubt.**
  - Item 1 holds keys that main placed: the empty string as the one id, and a non-string id
    beside a readable one.
  - Item 2 holds successors that main placed with a stale origin: the origin's disagreeing second
    submit arrives after the successor's record.
- **Towards placed.** Two shapes become placed that main holds in doubt only because of record
  order:
  - Item 2: the origin's only `OrderSubmitted` arrives after the successor's
    `OrderStateChanged`. #1224's property shrank to this case, `[[], [0], [], [], [7]], (0, 1)`:
    main gives `NotSent`, and the reading cancels by `rh-4-a` with SPY/Buy.
  - Item 3: a key's own readable `OrderSubmitted` arrives after its inheriting record.

The second direction is acceptable under DEC-176 because it brings the code to DEC-872 item 3's own
text. It does not bring the spec toward the code.

- **The id rule is unchanged.** A key is placed only by one unshared non-empty text id on its own
  records.
- **Instrument and side.** They are the key's own journaled ones, or its origin's as the whole
  stream gives them.
- **Only a cancel is enabled.** The only new action such a key allows is a cancel by that id, which
  reduces risk (`AGENTS.md` rule 13).
- **A `Submit` stays refused.** A `Submit` of such a key is still `Unknown` (`Ambiguous`) with
  nothing called (DEC-860 item 4), so no key the journal knows is ever placed again.

### Rationale

#1200 and #1202 left three review minors:

- An empty-string id was the key's one id, so a cancel could go out with `order_id: ""`. A
  non-string id was skipped, so the key could cancel by an id that the odd record may contradict.
- A successor kept its origin's instrument and side as they were when its record arrived. A later
  disagreement under the origin was missed, and a later only submit was never seen.
- A successor's own submit that arrived after its inheriting record left it in doubt. That fails
  closed but contradicts DEC-872 item 3.

Journal order between two different keys' records says nothing about the orders themselves, so a
rebuild whose result depends on it is wrong in one direction or the other. Reading each key's own
records as a whole removes that dependence.

### Alternatives considered

- **Accept the empty string as an id:** rejected. §9.16 rules it out, and the cancel would name no
  order.
- **Skip a non-string id and read the key's other ids:** rejected. The record that carries it is
  corrupt, and nothing says which id is the broker's.
- **Keep inheriting at the time the successor's record arrives:** rejected. The result would depend
  on how the stream interleaves two keys' records, which journal order does not make meaningful.
- **Read the whole stream, but hold in doubt every key that main holds in doubt:** rejected. It
  would keep the order dependence in the other direction, and it would deny a cancel that DEC-872
  item 3 and the journal fully support.

<a id="dec-875"></a>

## DEC-875: The Robinhood connector's reads reach only the one agentic account, and fail closed otherwise (C2)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | The E7-6 C2 tests builder (Claude Code), under DEC-79 and DEC-176: each item fills a gap CN-8 leaves by the reading that refuses more, and none loosens a rule or touches live money |
| **Story or claim** | E7-6, the [first live trade brief](../tasks/first-live-trade.md)'s C2 and LT-7; connections spec CN-8, §6.2 rule 6, U-R12 |

### Decision

1. **The account check.** `get_accounts` is read only to confirm the recorded account (the
   contract: `account_number` comes from the user, never from `get_accounts`). The check passes
   only when exactly one record has `agentic_allowed` `true`, that record carries the recorded
   number, and no other record carries that number. Two or more agentic records, or the
   number listed twice, is `AmbiguousAgenticAccount`. Otherwise, if the recorded account is absent
   or not agentic, the result is `NoAgenticAccount`. Another account is never adopted in its place.
   Account numbers match **byte-equal, no normalization**: a case-different, padded or longer
   number is another account. It never counts as the recorded one, and never rescues a recorded
   account that is missing or not agentic.
   A record without a text `account_number` or a boolean `agentic_allowed` makes the list
   unreadable.
2. **Every read checks first.** Each read runs the account check, fresh, before its own tool, and
   a refused check is the read's refusal. A pass is never remembered: two reads make two
   `get_accounts` calls, and a list that changes between them is judged anew. So no order, position or balance of an account that is
   not the one agentic account is read at all. Reads draw on the `Ordinary` budget, as
   `mandate-mcp`'s `CallClass` says.
3. **What leaves the connector.** A read whose arguments carry an `account_number` that is not
   the recorded number as byte-equal text, a near miss included, is `OtherAccount`, with nothing called. The tool is called with the
   recorded number. Records naming another account are dropped. A record naming no account, or an
   answer of any other shape, refuses the whole read (rule 3): dropping it could hide an order or
   position of the agentic account from the preflight. A record naming a **near miss** of the
   recorded number also refuses the whole read. A near miss is a number that is not byte-equal to
   it but, once both are trimmed and ASCII lower-cased, equals it or is a prefix of it, or has it
   as a prefix. The tool was called with the exact number, so a near miss is an unexpected shape,
   not another account (#1233 review round 1). The dropped records are not hashed,
   journaled or put in an error. The connector writes no exchange record (DEC-869 item 1).
4. **Assumed shapes.** The contract publishes no outputs. Until the founder's run reads real
   answers (R0 step 4): `get_accounts` is `accounts[]`; `get_portfolio` is one record;
   `get_equity_positions` is `positions[]` and `get_equity_orders` is `orders[]`. Every record
   carries `account_number`. Only that top-level `account_number` is attributed. Nested data in
   the assumed shapes, such as an account named inside a record, is out of scope until R0 step 4
   reads the real shapes. A different real shape refuses, and that is a finding for the
   connector, never worked around. The same holds if the founder's list shows a second agentic
   record, such as a matching crypto account (crypto is not on this path).
5. **Rule 13.** No `Submit` or `Cancel` waits on a read or on the account check. A failed check
   sets nothing that would later hold an exit, a protective order or a cancel on the agentic
   account. These still go out on the `RiskReducing` budget and name that account.
6. **Not C2's.** The account fingerprint (CN-5) follows after the live run, as the brief says.
   Mapping the records to DEC-470 item 1's preflight facts is C4's. Serving these reads from
   `mandate-rh-sim`, and the end-to-end test against it, follow with C4.

### Rationale

CN-8 says what must not get through and leaves open what happens when the account list is
unclear, when a record cannot be attributed, and when a caller names another account. Each item
refuses in those cases. A refused read blocks openings through C4's preflight, which adds no risk.
Exits do not depend on reads, so no refusal here can hold a risk reduction.

### Alternatives considered

- Trust the recorded number and ignore the other agentic records: rejected. The coordinator's
  brief asks for no guess, and two agentic accounts break the brief's dedicated-account premise
  (connections spec §6.3).
- Drop a record that names no account: rejected, because it could hide the agentic account's own
  order or position.
- Check the account once and remember the result: rejected. A stale pass outlives a change in the
  list, and a remembered failure could later be read as holding an exit.

<a id="dec-879"></a>

## DEC-879: The Robinhood connector's reads refuse any filter key their tool does not list (C2)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-10 (UTC) |
| **Decided by** | The E7-6 C2 tests-correction builder (Claude Code), under DEC-79 and DEC-176: a tightening that only refuses reads, loosens no rule and touches no live money |
| **Story or claim** | E7-6, the [first live trade brief](../tasks/first-live-trade.md)'s C2; connections spec CN-8 and §6.2 rule 6; the independent review of #1246, minor 3 |

### Decision

1. **A per-tool allowlist of filter keys.** `RobinhoodConnector::read` takes the caller's filters
   as the tool's other arguments. Besides `account_number`, which [DEC-875](earlier-600-899.md#dec-875) item 3
   already governs, a filter key is sent only when the read's tool lists it:
   - `get_portfolio`: none;
   - `get_equity_positions`: none;
   - `get_equity_orders`: `order_id`, `state`, `symbol`, `created_at_gte`, `placed_agent`,
     `cursor`.

   The `get_equity_orders` keys are the parameters the published contract lists for it
   ([the tool contract record](../tasks/robinhood-contract.md), `mandate-rh-sim`'s
   `contract/tools.json`). The contract names no parameter for `get_portfolio` or
   `get_equity_positions` beyond the account ([DEC-849](DEC-849.md) item 5 assumes
   `account_number` alone), so where the keys are unclear the narrowest set wins: none.
2. **Refused with nothing called.** A filter carrying any other key, a different spelling of the
   account such as `accountNumber`, `account_id` or ` account_number` included, is
   `RobinhoodError::UnlistedFilter`. Neither `get_accounts` nor the read's tool is called. Keys
   match byte for byte, with no normalization.
3. **Order of the checks.** The `account_number` check of DEC-875 item 3 runs first, so a filter
   naming another account is `OtherAccount` whatever keys stand beside it; then the allowlist;
   then the account check of DEC-875 items 1 and 2.
4. **Values are the tool's.** The allowlist reads keys only. A listed key's value is forwarded as
   given, and the tool judges it; an answer it refuses is `Unreadable` as before.
5. **Changing the list.** A key is added only by a new decision citing the contract text that
   lists it, such as the founder's run reading the real tool list (R0 step 4).

### Rationale

The review of #1246 found that `read` forwarded every filter key other than `account_number`
verbatim. A key such as `accountNumber` or `account_id` could name another account in a spelling
the server might honour and the connector never checks, which is the leak CN-8 forbids. An
allowlist closes it without guessing which spellings a server reads. The change only refuses
reads, and a refused read blocks openings through C4's preflight, which adds no risk. Exits,
protective orders and cancels never pass through `read` (`AGENTS.md` rule 13, DEC-875 item 5),
so no refusal here can hold a risk reduction. That makes it a DEC-176 tightening an agent may
accept.

### Alternatives considered

- A denylist of account-like keys (`accountNumber`, `account_id`, ...): rejected. It must guess
  every spelling a server might read, and a missed one leaks.
- Strip unlisted keys and send the rest: rejected. A caller that asked for a filter would get an
  unfiltered answer without knowing, and C3's list-and-match relies on its filters being applied.
- Validate values too (the `state` and `placed_agent` enums): left to the tool, which refuses
  what it does not take; it widens nothing a key check leaves open.

<a id="dec-880"></a>

## DEC-880: `mandate-api` locates a refused body through `serde_path_to_error`

| | |
|---|---|
| **Status** | Accepted (agent, a reversible engineering decision; the founder reviews after the fact under DEC-79) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | lane L2, from L2's range DEC-880 to DEC-889 |
| **Story or claim** | E10-10, claim #750; #993's two minors, pinned by #1104 |

### Decision

1. `mandate-api`'s `decode` deserializes every request body through
   `serde_path_to_error::deserialize`, and builds the refusal's JSON pointer from the path serde
   was deserializing when it refused (DEC-681 item 10, #1104): an array index as `/<index>`, a
   member or variant name escaped as RFC 6901 says. The code is still read from serde's own error.
   This replaces the arithmetic that turned serde's byte position into a pointer.
2. The duplicate-member walk stays, because serde's reader takes a member named twice without
   refusing it, and it alone gives `duplicate_member` its pointer.
3. The crate is never on the hashing path (ES-07): it only locates a refusal of a request body.
   It is added as a workspace dependency, pinned `0.1.20`, with a row in `docs/dependencies.md`
   and an entry in `mandate-api`'s `allowed_external`.

### Rationale

A byte position cannot tell a plain last member apart from an internally tagged object followed
by a sibling: serde_json reports a custom refusal after the closing `}` or `]`, and reads past the
`,` in an array before it gives the error a position, so the position-based pointer named the
parent or a sibling (#1104's evidence). The path serde was deserializing names the member, the
array item, or the tagged object itself, which is DEC-681 item 10's "at the member, where serde
can name it". The crate adds one crate new to the workspace; its dependencies `serde_core`,
`itoa` and `serde` are already in the lock.

### Alternatives considered

- **Tracking the path by hand in `walk`.** A position-driven walk cannot tell the two cases above
  apart; tracking the path by hand means wrapping every `Deserializer`, `SeqAccess`, `MapAccess`
  and `EnumAccess`, which is the crate this decision adopts, rewritten and owned here.
- **Stepping back over a closing `}`, `]` or `,` from serde's error position.** Fails #1104's
  tagged-object test: it locates a refusal inside `who` at `/who/kind`, a sibling.

<a id="dec-881"></a>

## DEC-881: `decode` refuses an internally tagged object written as a JSON array, as `type`

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: the reading only tightens; the founder reviews after the fact under DEC-79) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | lane L2, from L2's range DEC-880 to DEC-889 |
| **Story or claim** | E10-10, claim #750; found by the review of #1104 |

### Decision

1. `mandate-api`'s `decode` reads an internally tagged object only from a JSON object. Given a
   JSON array in its place, it refuses the body as `type` at the tagged member's pointer, the same
   code and pointer a scalar there gets today (`"who": 5` is `type` at `/who`, DEC-681 item 10's
   "`type` at the member, where serde can name it"). The schemas allow only an object for each:
   - `Actor` (`envelope.rs`, `envelope.schema.json#/$defs/Actor`);
   - the kill switch's `Scope` (`requests.rs`, `commands/kill-switch-request.schema.json`'s
     `scope`);
   - the problem's tagged `Violation` (`problem.rs`, `envelope.schema.json#/$defs/Violation`).
2. The expected answer for each array form, shown for an `Actor` member `who`:

   | Body's `who` | Today | Expected |
   |---|---|---|
   | `[]` | `missing` at `/who` | `type` at `/who` |
   | `["user", "a"]` | accepted as the user `a` | `type` at `/who` |
   | `["client", "a", "b"]` | accepted as the client `a` for `b` | `type` at `/who` |
   | `["bogus", "a"]` | `enum` at `/who/0` | `type` at `/who` |

   An array form that is an array's item is refused at that item: `"crowd": [["user", "a"]]` is
   `type` at `/crowd/0`, never at `/crowd` nor at `/crowd/0/<n>`. The same holds for `scope`
   (`/scope`) and for a problem's violation (`/violations/<index>`).
3. The decision covers only internally tagged objects. serde's derived plain structs also read a
   JSON array, their members in order, through `visit_seq`: a whole `KillSwitchRequest` written
   as `[{"kind": "agent", "id": "a"}]`, and a `record` written as an array, decode today. That
   leniency is a separate follow-up in the backlog, with its own tests first and its own
   decision; this decision does not settle it.
4. Nothing else changes: the object form decodes as before, a refusal inside a tagged object is
   still located as DEC-880 and #1104 pin (`/who`, `/who/kind`), and a scalar is still `type` at
   the member.

### Rationale

serde's internally tagged enums accept a sequence as well as a map: the first item is read as the
tag and the rest as the variant's members in order. So `decode` accepted a shape the schema
refuses, the code was looser than the spec, and a client tested against `decode` could send a
body that a schema-validating peer rejects. The array form decodes to the same value as the
object form, so accepting it never let anything through that the object form would not, but the
spec is the contract (DEC-176): refusing it only refuses more and adds no risk. `type` is the code
DEC-681 item 10 gives a member of the wrong JSON type, and a non-object `who` already gets it at
`/who`; `missing` for `[]` named a `kind` member an array cannot have.

### Alternatives considered

- **Leave it, as leniency only.** Reads the spec as loosely as the code, which DEC-176 leaves to
  the founder.
- **`malformed` at `""`.** The body is JSON; DEC-681 item 10 keeps `malformed` for a body that is
  not.
- **Refuse the array form at its first item (`/who/0`), as serde's `enum` does for `["bogus"]`.**
  Names an item the schema never has; the wrong thing is the member's JSON type.

<a id="dec-882"></a>

## DEC-882: `decode` refuses a plain object written as a JSON array, as `type`

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: the reading only tightens; the founder reviews after the fact under DEC-79) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | lane L2, from L2's range DEC-880 to DEC-889 |
| **Story or claim** | E10-10, claim #750; the follow-up [DEC-881](earlier-600-899.md#dec-881) item 3 names |

### Decision

1. `mandate-api`'s `decode` reads every shape the crate derives as a plain struct only from a JSON
   object. Given a JSON array in its place, it refuses the body as `type` at that shape's pointer:
   `""` for a whole body, the member's pointer for a member, and the item's pointer for an array's
   item. This is the code and pointer a scalar there gets today, DEC-681 item 10's "`type` at the
   member, where serde can name it". The rule selects every derived struct the crate decodes,
   each of which its schema allows only as an object:
   - the request bodies (`requests.rs`, `commands/*-request.schema.json`): `PauseRequest`,
     `HoldRequest`, `EndDelegationRequest`, `LiftHoldRequest`, `ResumeRequest`,
     `AcknowledgeRequest`, `OwnerRequest`, `ConfirmRequest`, `ApprovalResponseRequest`,
     `DelegationPreviewRequest`, `StopRequest`, `KillSwitchRequest`, `OwnerExitRequest`, and
     `RevokeRequest`, and their nested `DelegationChosen` (`delegation`) and `BidConfirmation`
     (an `owner_exit` item);
   - the responses (`responses.rs`, `commands/*.schema.json`): `CommandAccepted`,
     `ApprovalResponseAccepted`, `ConfirmAccepted`, `EndDelegationAlreadyEnded`,
     `DelegationPreview`, and `CommandStatus`, and their nested `PreviewedDelegation`
     (`delegation`) and `Step` (a `steps` item);
   - the envelope's shapes (`envelope.rs`): `Watermark` (`common.schema.json#/$defs/Watermark`,
     an `as_of` item), `Record` and `StepUpEvidence` (`envelope.schema.json#/$defs/Record` and
     `#/$defs/StepUpEvidence`);
   - the problem (`problem.rs`, `envelope.schema.json#/$defs/Problem`).

   No derived struct of the crate has a schema that allows an array. The members whose schemas
   are arrays (`as_of`, `steps`, `violations`, `owner_exit`, `confirmed_paths`,
   `warnings_acknowledged`, `dropped`, and a policy finding's set value) are `Vec`s or an untagged
   enum's array variant, never a struct, and keep reading arrays.
2. The expected answer for each array form, shown for a pause's `record` and for a whole kill
   switch:

   | Body | Today | Expected |
   |---|---|---|
   | `{"record": ["sha256:…", "sha256:…"]}`, positional, items that would decode | accepted as that record | `type` at `/record` |
   | `{"record": ["sha256:…", 5]}`, an item that would not | `type` at `/record/1` | `type` at `/record` |
   | `{"record": []}` | `type` at `/record` (too short) | `type` at `/record`, unchanged |
   | `[]` as a pause, every member optional | accepted as `{}` | `type` at `""` |
   | `[{"kind": "agent", "id": "a"}]` as a kill switch | accepted | `type` at `""` |
   | `[["agent", "a"]]` as a kill switch, an item DEC-881 refuses once fixed | accepted | `type` at `""` |

   An array in a struct's place is refused at that place, never at an item inside it. The same
   holds at `/step_up`, `/delegation`, `/owner_exit/<index>`, `/as_of/<index>`,
   `/steps/<index>`, and at `""` for each response, problem, and envelope shape decoded whole.
3. Nothing else changes:
   - the object form decodes as before;
   - a scalar in a struct's place is still `type` there;
   - a refusal inside an object keeps the path DEC-880 (#1117) locates it at
     (`/record/artifact`, `/as_of/0/seq`);
   - an internally tagged object is read as DEC-881 says;
   - the scalar newtypes (`Decimal`, `Ref`, `Timestamp`, `Asset`, `Id`, `EventId`) read a JSON
     string as before, and the untagged `CurrentBase` and `PolicyValue` keep their shapes;
   - `encode` writes every shape as an object, as before.

### Rationale

serde's derived structs accept a sequence as well as a map: `visit_seq` reads the struct's
members in their declared order, and a member marked `default` may be left off the end. So
`decode` accepted bodies every workspace API schema refuses: `[]` was a valid pause, and a record
written as two strings was a record. The array form decodes to the same value the object form
would, so accepting it let nothing through that the object form would not, but the schema is the
contract and a client tested against `decode` could send what a schema-validating peer rejects.
Refusing the array form only refuses more and adds no risk, so it is DEC-176's to take. `type` is
DEC-681 item 10's code for a member of the wrong JSON type, and `"record": 5` already gets it at
`/record`. This closes DEC-881 item 3.

### Alternatives considered

- **Leave it, as leniency only.** Reads the spec as loosely as the code, which DEC-176 leaves to
  the founder.
- **Refuse at the first offending item (`/record/1`), as serde does today for a bad item.** Names
  an item the schema never has; the wrong thing is the member's JSON type. It would also accept a
  positional array whose items all decode.
- **`malformed` at `""` for a whole body given as an array.** The body is JSON; DEC-681 item 10
  keeps `malformed` for a body that is not.
- **Refuse only the request bodies.** The responses, the problem, and the envelope's shapes are
  decoded by the same `decode` (the CLI and the web app read them), and their schemas are objects
  too.

<a id="dec-883"></a>

## DEC-883: A connect start serves Alpaca paper OAuth only, and refuses with the first of its refusals in a fixed order, step-up first

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), E10-13, from L2's range DEC-880 to DEC-889, on the review of #1129. Under DEC-176: item 2 refuses a start that no spec text admits, and item 1 fixes an order the specs leave open, so each only refuses more or reports less. Neither loosens anything |
| **Story or claim** | E10-13, claim #754; PR #1129 (the manager start tests); [connections spec](../../specs/connections.md) §3, §4, §5.2, §6; [journal spec](../../specs/journal.md) §9.8 rule 131; [DEC-693](DEC-693.md) items 2 and 7; [DEC-698](DEC-698.md); [DEC-821](DEC-821.md) item 7; DEC-124 |

### Decision

1. **Order.** When more than one refusal holds for a connect start (`manager::plan_start`), the
   start refuses with the first of the following, and only that one:
   1. `StepUpNotVerified`: the step-up evidence for the start's digest does not count.
   2. `EnvironmentRefused`: the environment is not paper ([DEC-693](DEC-693.md) item 2).
   3. `BrokerRefused`: the broker is one this route does not serve (item 2).
   4. `FoldCannotAnswer`: the control-stream fold cannot answer for the id or for the
      `account_ref` ([DEC-693](DEC-693.md) item 7).
   5. `RequestOpen`: the id has an open `ConnectionRequested` (rule 131, opening).
   6. `AlreadyEstablished`: the id has a `ConnectionEstablished` (rule 131, opening).
   7. `AccountRefUsed`: an earlier request or establishment names the `account_ref`
      ([DEC-699](DEC-699.md) I3).

   With none of them holding, the start commits exactly `[ConnectionRequested]`.
2. **The start route, `POST /connections/oauth/start`, serves Alpaca only.** A start for any
   other broker is refused as `BrokerRefused` before anything commits.
   - The route is the Alpaca OAuth flow of connections spec §5.2, with its fixed redirect URI
     `/v1/oauth/alpaca/callback`, and it connects paper only ([DEC-821](DEC-821.md) item 7,
     [DEC-693](DEC-693.md) item 2).
   - Robinhood connects over MCP OAuth (`mcp_oauth`, connections spec §3 and §6). It has no paper
     environment (connections spec §4 and §6.1, DEC-124), so a paper start for it could never
     complete.
   - Kraken Derivatives US takes an API key only (connections spec §3 and §7,
     `record::Broker::takes`), never OAuth.
   - A request journaled for a start that cannot complete would only wait for its teardown.
     Refusing it first commits nothing and spends no step-up (journal spec §9.8).
3. **The connect digest is unchanged.** `connect_digest` binds any broker it is given. Only
   `plan_start` refuses one this route does not serve.
4. A test pins the order exhaustively, over every combination of step-up, environment, broker,
   and fold answers. It uses its own ordered check as the oracle, so swapping any two refusals
   fails it.

### Rationale

- **Step-up first.** A caller whose step-up does not count learns nothing of the control
  stream's state, whether an id is open or established or an `account_ref` is used. This is
  [DEC-698](DEC-698.md)'s reason for the revoke.
- **Environment and broker next.** Both are facts of the request itself, not of the stream.
  DEC-693 item 2 says a live start is refused before any challenge would matter.
- **`FoldCannotAnswer` before the stream facts.** No refusal states a fact the folds did not
  establish (DEC-693 item 7). The owner is told to retry.
- A fixed order makes the refusal deterministic and testable. Every start refused before is still
  refused, with one fixed reason.

### Alternatives considered

- **Leave the order to the implementation.** Rejected: an order that puts a stream fact ahead of
  step-up tells an unverified caller about the stream, as DEC-698 found for the revoke.
- **Admit every broker at start, and let the checks refuse later.** Rejected: it journals a
  request and spends a step-up on a connect that can never complete.
- **Serve Robinhood through this route.** Rejected for now: its MCP OAuth flow is connections
  spec §6's and has no paper environment. Serving it would need its own flow and a new decision.

<a id="dec-884"></a>

## DEC-884: A connect's finish reads the request fold first, establishes paper only, and refuses a self-conflict

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), E10-13, from L2's range DEC-880 to DEC-889. Under DEC-176: item 1 fixes an order the specs leave open, and items 2 and 3 refuse inputs no spec text admits, so each only refuses more. None loosens anything, and none denies a teardown or a revocation |
| **Story or claim** | E10-13, claim #754; the manager finish tests (stacked on PR #1129); [connections spec](../../specs/connections.md) §5.2 steps 5 and 6, §9.1; [journal spec](../../specs/journal.md) §9.8 rules 54, 55, 64, and 131; [DEC-693](DEC-693.md) item 7, [DEC-694](DEC-694.md), [DEC-697](earlier-600-899.md#dec-697), [DEC-699](DEC-699.md) items 4 and 5 |

### Decision

1. **Order.** `manager::plan_finish` reads the control-stream fold of the connection's request
   before the connect's outcome, and stops at the first of:
   1. `FoldCannotAnswer`: the fold is unavailable, stale, or behind ([DEC-693](DEC-693.md) item 7).
   2. `NotOpen`: no request is open to close (rule 131, closing: a request is closed once).
   3. `ClosedByRevoke`: a `ConnectionRevoked` closed the request. The plan carries no effect and
      no refusal: the teardown runs and nothing is journaled ([DEC-699](DEC-699.md) item 5).

   With the request open, a passed outcome commits exactly `[ConnectionEstablished]` version 2,
   and a refused one exactly `[ConnectionRefused]` (occasion `connect`), each repeating the
   request's members as rule 131 says. No plan carries an effect and a refusal ([DEC-697](earlier-600-899.md#dec-697)).
2. **The finish establishes paper only.** A passed outcome of a `live` request is refused as
   `LiveNotServed`. Rule 64 requires a live establishment's `margin_attestation`, which this path
   does not carry, and the start route connects paper only ([DEC-693](DEC-693.md) item 2,
   [DEC-883](earlier-600-899.md#dec-883)). A live request comes only from a one-step writer such as the CLI's
   `connection record`, which commits its own establishment. A refusal or a teardown of a live
   request still closes it, so the deadline always ends `connecting`.
3. **`already_connected` naming the request's own id is refused as `ExistingIsSelf`.** Rule 55
   requires `existing_connection_id` to differ from `connection_id`, so such a record would be
   refused at append. The request stays open and the deadline's teardown closes it.
4. **The reason a refusal records is journal spec §9.8's.** An executor reason names its own check,
   so a reason of another check is not representable; `account_mismatch` and `already_connected`
   are the manager's; the four teardowns have `check` `null` (rule 54).

### Rationale

- **Fold first.** No plan states a fact the fold did not establish (DEC-693 item 7), and a
  revoked request must never be closed again, whatever the connect's outcome (I8).
- **Refuse rather than write a record the journal would refuse.** Items 2 and 3 name, before any
  append, inputs whose record breaks rule 64 or 55. Each leaves the request open for its
  teardown, which never adds risk.

### Alternatives considered

- **Read the outcome first.** Rejected: a passed connect whose request a revoke closed would be
  established, against DEC-699 item 5.
- **Carry `margin_attestation` in the passed outcome.** Rejected for now: no route starts a live
  connect, and adding one is a founder decision (DEC-79, live money).
- **Leave items 2 and 3 to the journal's refusal at append.** Rejected: the manager would plan a
  record it knows is refused, and the server would have to tell that refusal apart.

<a id="dec-886"></a>

## DEC-886: How an API-7 operation's lenient decoder reads the bodies DEC-682 item 27 leaves open

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), from L2's range DEC-880 to DEC-889, confirmed by the merge coordinator. None of the readings applies more than the strict decoder would. None weakens a safety invariant or an `AGENTS.md` non-negotiable |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); [workspace API spec](../../specs/workspace-api.md) §5's lenient rule; [DEC-682](earlier-600-899.md#dec-682) item 27; [DEC-689](earlier-600-899.md#dec-689) item 2; [DEC-880](earlier-600-899.md#dec-880) |

### Context

DEC-682 item 27 makes the six API-7 bodies (pause, hold, the kill switch, an owner exit, Skip on an
approval, and ending a delegation) lenient: only the hard members are strict (the path ids, the
kill switch's `scope`, an owner exit's `instrument`, and a Skip's `verdict` and `content_hash`),
and every other member that is omitted, unknown, or fails to parse is dropped and listed in the
`202`'s `dropped`. The spec, the item, the schemas, and the server's cases in
`schemas/workspace-api/examples/lenient/api7.json` leave six bodies open, which A2's
`decode_lenient` and its tests need settled:

- an unknown member inside the hard kill-switch `scope`;
- a workspace `scope` with no `id`;
- a defective member nested inside a non-hard member: the cases drop the whole member for a nested
  parse failure (`/record`, `/step_up`, `/owner_exit`), and say nothing of a nested unknown member;
- an owner exit's bid members given as a mix of `null` and values, which the strict schema accepts;
- a pause or hold body that is JSON but not an object;
- a member named twice, which the strict decoder refuses `duplicate_member` (DEC-880).

### Decision

1. **Unknown member inside the hard kill-switch `scope`.** It is dropped and listed at
   `/scope/<name>`. `kind` and `id` are kept and apply in the route's workspace only. **A dropped
   `workspace` (or `workspace_id`, `ws`) naming another workspace never widens or redirects the
   stop.**
2. **Workspace scope with no id.** `{kind: "workspace"}` with no `id` is the route's workspace
   scope.
3. **Defective nested non-hard member.** A defective nested non-hard member (unknown, malformed, or
   wrong-typed inside it) drops the whole top-level member, which is listed at its top-level
   pointer. Part of it is never applied.
4. **Partial bid confirmation.** An owner exit's bid members given as a mix of `null` and values
   make the confirmation partial. Every bid member present, nulls included, is dropped and listed.
   All four `null` stays a valid body.
5. **Non-object JSON body.** A pause or hold body that is JSON but not an object is read as `{}`
   with `[""]` dropped. The other four operations refuse it.
6. **Duplicate members.** A duplicate non-hard member is dropped and listed once. A duplicate hard
   member is refused `duplicate_member`.
   - The strict decoder already refuses duplicates (DEC-880), so the lenient one refuses no more
     than strict.
   - Which scope or instrument was meant cannot be resolved without risking applying the wrong
     one.

   The owner's recovery is to resend the stop with one scope; the Stop button always sends a
   well-formed body. The coordinator has mentioned this item to the founder as an FYI, since it
   refuses a malformed kill switch.

The settled readings and mechanics the same decoder takes:

7. A Skip keeps a valid `step_up`: the schema conditions only `delegation` on the verdict.
8. A Skip's `delegation: null` is kept, and a non-null one is dropped (DEC-682 item 11).
9. A kill switch's `owner_exit: []` is kept: the schema allows an empty array.
10. Ending a delegation, the kill switch, an owner exit, and Skip refuse a body that is not JSON
    (the server's cases, and `check_lenient.py`).
11. `dropped` lists pointers once each, in the order their members appear in the body.
12. A member left out is never listed: there is nothing at its pointer (`check_lenient.py`).
13. The tests PR's stub returns a new `Refused::Unimplemented` variant, the form the pending gate
    accepts (DEC-137); the implementation PR removes it.

### Rationale

Each reading either drops a member, so nothing new is applied, or refuses exactly what the strict
decoder already refuses. Items 1 and 3 never apply part of a member, so a nested `workspace` can
never move a command to another workspace (#1155). Items 2 and 5 only tighten toward never refusing
a stop. Item 6 refuses no body the strict decoder accepts.

### Alternatives considered

- **Refuse an unknown member inside `scope`, and a workspace scope with no id.** Refuses a stop for
  a member that applies nothing.
- **Drop only the nested unknown member and keep the rest of its parent.** Applies part of a member
  the client sent malformed: a loosening, for the founder.
- **Keep a mixed `null` and value bid confirmation, as the strict schema allows.** Applies a partial
  confirmation: a loosening, for the founder.
- **Drop a duplicated hard member.** Leaves no scope or instrument to act on, or guesses one.

### Consequences

- Two tests PRs (DEC-77): the first pins every server case, a non-JSON or non-object body, and
  idempotency over the members kept; the second generates corruptions of each operation's members
  against an independent hard-member list and pins the workspace members (#1155) and duplicates.
- The implementation PR then implements `decode_lenient` and removes the stub variant.

<a id="dec-887"></a>

## DEC-887: An owner exit's bid members sent only as `null` or left out are no confirmation, and nothing is dropped

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L2 (`claude-code`), from L2's range DEC-880 to DEC-889, confirmed by the merge coordinator. The kept value and what applies are the same under both readings, so it adds no risk, and it weakens no safety invariant or `AGENTS.md` non-negotiable |
| **Story or claim** | E10-10, claim [#750](https://github.com/kunwarshivam/mandate/issues/750); [workspace API spec](../../specs/workspace-api.md) §5; [DEC-682](earlier-600-899.md#dec-682) items 12 and 27; [DEC-886](earlier-600-899.md#dec-886) item 4 |

### Context

An owner exit's bid confirmation (`bid`, `bid_size`, `quoted_at`, `floor`) is all or nothing: §5
and the schema say that "if any of its members is missing or unparsable, every one sent is
dropped". DEC-886 item 4 settled a mix of `null` and values (partial, dropped whole) and all four
`null` (a valid body, nothing dropped). It left open a body whose bid members are each `null` or
left out, with at least one of each, such as `{"instrument": …, "bid": null}`. The implementation
PR's mutation run found no test pinning it.

### Decision

1. **Bid members sent only as `null`, with the rest left out, are no confirmation.** Nothing is
   dropped: `dropped` names none of them, and no confirmation is kept.
2. **A mix of `null` and values stays partial** and is dropped whole, every member sent listed,
   nulls included (DEC-886 item 4, unchanged). An unparsable member, with or without nulls, makes
   the confirmation partial too.

### Rationale

`null` and absent both mean "no confirmation sent" in the regular session and for crypto (DEC-682
item 12), so a body that sends only nulls sent no confirmation, and nothing was dropped. This
matches DEC-886 item 4's all-four-`null` body. Under either reading the kept value has no
confirmation and equities wait for the session, so the reading changes only what `dropped` lists.

### Alternatives considered

- **(a) Nothing listed.** Chosen.
- **(b) Drop and list every member sent, nulls included**, reading §5's "missing" literally. It
  lists a `null` the schema allows as if it were defective, and differs from the all-four-`null`
  body only by which members were left out.

### Consequences

A tests-correction PR pins a lone `null` bid and three `null` with one left out, each with
`dropped: []` and no confirmation kept, before the implementation PR lands.

<a id="dec-890"></a>

## DEC-890: `mandate journal verify` and `verify-cold` run the connection lifecycle check on one stream, and report the cause check as not run

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176; coordinator ruling 2026-10-09 ~19:28 UTC) |
| **Date** | 2026-10-09 (UTC) |
| **Decided by** | Lane L5's builder (`claude-code`) under DEC-79 and DEC-176, on the coordinator's ruling of option (ii). Each reading takes the outcome that adds no risk: a tail range fails closed, a check that cannot run is said not to have run, and nothing that judges a record passes it unjudged. It weakens no safety invariant and touches no `AGENTS.md` non-negotiable |
| **Story or claim** | E12-3, claim [#748](https://github.com/kunwarshivam/mandate/issues/748), slice W3; [journal spec](../../specs/journal.md) §9.8 (stream rules 66 to 68 and 131), §11; follows [DEC-782](DEC-782.md) item 5 and [DEC-885](DEC-885.md); amends the report forms of [DEC-115](../04-decision-log.md#decisions) (E5-4's `verify`) and [DEC-490](DEC-490.md) item 7 (`verify-cold`) |

### Context

DEC-782 item 5 deferred §11's two connection checks in the CLI until the library and a
multi-stream input existed. E7-17 (#1159, #1183, DEC-885) built the library:
`verify_connection_lifecycle_from(start, rows)` with `ConnectionStart::{Genesis, Anchored,
Unanchored}`, and `verify_connection_causes_from_genesis(rows)`.

Both commands read one stream's export (DEC-115 item 8, DEC-490 item 3). Two questions follow:

- **The lifecycle check on one stream.** §9.8 says of stream rules 66 to 68 "Each holds on its own
  stream", and rule 131 is "held and checked as rules 66 to 68 are". Rules 66, 67, and 131 read
  only the control stream; rule 68 reads only its account stream, taking its `account_ref` from
  the stream's own `stream_id`. DEC-885 item 1 defines the connection anchor per stream, after
  that stream's records `1` to `n − 1`, and I4 says a range from `seq` 1 is anchored on nothing.
  So a one-stream export from `seq` 1 is that stream's full chain, and the library's per-stream
  fold gives its records the verdict the multi-stream full-chain run gives them.
- **The cause check on one stream.** §11 runs `connection_cause_mismatch` "Across the control
  stream and its account streams, in the full-chain run only (a range never holds the other
  stream)", and DEC-885 item 6 keeps it there. A control-stream establishment or rotation names a
  `ConnectionChecked` on an account stream, and an account-stream copy names its control-stream
  original. On one stream every valid version-2 establishment, rotation, and copy would fail: a
  false SEV-1 (§11, "On failure").

### Decision

1. **The lifecycle check runs on control and account streams.** On a single-stream export of a
   control or account stream, both commands run `verify_connection_lifecycle_from`:
   - with `ConnectionStart::Genesis` when the trusted start's `from_seq` is 1 (DEC-885 I4);
   - with `ConnectionStart::Unanchored` for any other `from_seq`. The CLI cannot fold the stored
     chain before the range (DEC-885 item 2, DEC-782 item 4), so the range fails closed at its
     first judged record, reported as `connection_lifecycle_mismatch` at that `seq` (DEC-885
     item 4).

   Agent and other stream types run no connection check.
2. **The run order is DEC-782's.** The check takes DEC-782 item 1's second step, the stream-type
   range checks, after checks 1 to 6 and before the anchor and token checks. On a control stream
   it runs beside `anchor_self_mismatch` and `break_glass_cause_mismatch`, and the failure at the
   lowest `seq` is reported (DEC-782 item 2). Those checks judge `AnchorComputed` and
   `RecordsAccessed`, and this one judges only connection records, so no record fails both. The
   failure prints as DEC-782 item 3's `result: failed, seq <n>, connection_lifecycle_mismatch`.
   DEC-782 is not edited.
3. **The cause check is not run by either command.** `connection_cause_mismatch` needs the control
   stream and its account streams together, which a single-stream export never holds. It belongs
   to the full-chain run (§11, DEC-885 item 6) and to a future multi-stream input (backlog,
   E12-3).
4. **The report says the cause check was not run, and never skips it silently.** Both commands
   print one more line, exactly:

   ```text
   not run: connection_cause_mismatch (needs the control stream and its account streams together)
   ```

   - **Where:** just before `result:`, after the input lines (`anchor:` for `verify`, `token:` for
     `verify-cold`). Every earlier line and the `result:` line are unchanged, so a reader of the
     result line reads it as before.
   - **When:** whenever a run reaches DEC-782 item 1's second step on a control or account export
     holding a record the lifecycle check judges: a `ConnectionRequested`,
     `ConnectionEstablished`, `ConnectionCredentialRotated`, or `ConnectionRefused` on a control
     stream, or any connection record on an account stream (§11, "No anchor"). It is printed
     whatever the result from there: verified, a stream check's failure, the anchor's, or the
     token's. It also prints on a control export holding only `ConnectionRequested` or
     `ConnectionRefused` records, where the cause check would have nothing to judge: the line is
     still true, since the check did not run, and printing it there is conservative and intended.
   - **When not:** on an agent or other stream; on a control or account export holding no judged
     record (a revocation is never judged, I6); on a run that stops at checks 1 to 6 or the cold
     walk, which never reached a connection check and whose result already reports why; and on an
     input refusal, which writes no report (DEC-115 item 11). Each of these
     reports exactly as before, byte for byte.

   This adds one line to DEC-115's report and to DEC-490 item 7's "seven lines, in this order and
   nothing else". DEC-115 and DEC-490 are not edited; this decision amends their report form.
   `result: verified` keeps its meaning: every check that ran passed.

### Rationale

The lifecycle check needs nothing outside the stream it judges, so running it on the stream an
export holds is the spec's own check, not an approximation. A range from `seq` 1 is that stream's
full chain, and any other range fails closed exactly as DEC-885 item 4 and DEC-782 item 4 already
do for `held_mismatch`.

The cause check is the opposite case. On one stream it cannot tell a sound journal from a broken
one, and running it would raise a SEV-1 on every valid connection. Leaving it out is the only
honest choice, and saying so in the report keeps an auditor from reading `verified` as covering a
check that never ran. The line sits before `result:` so that the result line stays the last line,
where DEC-115 and DEC-490 put it, and keeps DEC-782 item 3's form. It is printed on a failure
too, because a fixed rule ("the run reached the stream checks on connection records") is simpler
to read and to test than one that depends on which check failed. The line goes once a
multi-stream input runs the cause check.

### Alternatives considered

- **Run the cause check on a range from `seq` 1.** Rejected: one stream never holds a cause's
  other half, so every valid version-2 establishment, rotation, and copy would fail, a false SEV-1
  that pauses agents or freezes mandate changes on a sound journal.
- **A bare `result: verified` with no line.** Rejected: a reader would take it as covering the
  cause check, which did not run.
- **Run the lifecycle check from the range's first event on a tail.** Rejected by DEC-782 and
  DEC-885: it fails valid records that refer to state before the range.
- **Print the line only on a verified result.** Rejected: whether the cause check ran does not
  depend on which other check failed, and a conditional line is easier to misread.
- **Put the line after `result:`.** Rejected: it would move the result off the report's last
  line, where both report forms put it.

<a id="dec-897"></a>

## DEC-897: The verification run's tightenings: malformed in-range rows fail, `from_seq` 0 is a `range` refusal, anchor rows are verified first, no `causation_id`, and artifact-store outages never pass

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176, with the coordinator's rulings; items 1 to 4 and 5(b) only tighten, item 5(a) applies journal spec §11 as written, and the founder reviews after the fact under DEC-79) |
| **Date** | 2026-10-10 (UTC) |
| **Decided by** | Lane L5's audit lane (`claude-code`) under DEC-79 and DEC-176, with lane L5's coordinator's rulings. This decision changes no rule: items 1 to 4 and 5(b) each take, where the specs leave a choice, the reading that never ends better than the other (no skip, pass, `incomplete`, or refusal where the other would record a failure), and item 5(a) applies §11 as written. Item 5(a)'s cost is stated in that item |
| **Story or claim** | E12-3, claim [#748](https://github.com/kunwarshivam/mandate/issues/748), the verification run of [workspace API spec](../../specs/workspace-api.md) §4.8.1 "Verification"; [journal spec](../../specs/journal.md) §3, §9.13, §11; related [DEC-787](DEC-787.md), [DEC-788](DEC-788.md), [DEC-789](DEC-789.md), [DEC-893](DEC-893.md), [DEC-895](DEC-895.md), [DEC-896](DEC-896.md) |

### Context

The run of workspace API §4.8.1 is closed on its refusal order ([DEC-788](DEC-788.md) item 4),
inputs ([DEC-787](DEC-787.md)), run order (DEC-786), and outcomes ([DEC-789](DEC-789.md),
[DEC-896](DEC-896.md)). Writing its tests left places where the text allows two readings, and
one (item 5(a)) where it allows one that the tests must still pin. Where there is a choice, this
decision takes the reading that never ends better than the other: never a pass, an `incomplete`, or
a refusal where the other would record a failure.

### Decision

1. **A malformed in-range anchor or segment row fails the range; it is never skipped.**
   - **The clause read.** §4.8.1's run order, steps 2.1 to 2.4: the checks judge "every
     `AnchorComputed` on `ctl:{ws}` whose leaf for this stream has a `seq` inside the range" and
     "every `SegmentExported` manifest of this stream whose range lies wholly inside the run's
     range", read only from the workspace's own `ctl:{ws}` in the run's snapshot (DEC-787 item 2).
     The text says which rows count, not what a row of one of those types that does not parse is.
   - **The reading.** A `ctl:{ws}` row whose `event_type` is one the walk judges (`AnchorComputed`
     or `SegmentExported`) and which covers the range, and whose body fails strict parsing (§11
     check 1: it parses with duplicate keys rejected and re-canonicalizes to its own bytes), fails
     the range: `result: fail`, never `pass` or `incomplete`. A row whose body does not parse
     cannot show that it does not cover the range, so it is judged as one that does, as
     [DEC-895](DEC-895.md) item 1 treats a candidate start row whose payload cannot be known.
   - **The alternative.** Skip the row, as a row the lookup cannot read, and judge the range on
     the rows that parse.
   - **Why it is tighter.** Skipping lets whoever can corrupt one hot control row remove an
     anchor or a manifest from the run's view, and with it the `anchor_head_mismatch`,
     `anchor_root_mismatch`, token, `segment_manifest_mismatch`, `segment_rows_mismatch`, or
     `segment_gap` check that row would have failed. Failing turns that into a found fault. It is
     DEC-895 item 1's reasoning, applied to the walk's rows rather than the start's.

2. **`from_seq` 0 is refused at step 5, as a `range` violation (422).**
   - **The clause read.** §4.8.1: "`from_seq` is at least 1", and the order of refusals
     (DEC-788 item 4): step 2 refuses the members that are not ids when they are malformed, and
     step 5 refuses `range`.
   - **The reading.** `from_seq` 0 is a well-formed integer outside the range, so it is refused at
     step 5, 422 `invalid` with violation `range`, nothing recorded. Every other member is checked
     at the step §4.8.1 gives it, so a request with `from_seq` 0 still meets the role check, the
     shape of its other members, the idempotent replay, and the `stream_id` 404 first.
   - **The outcomes.** A request with `from_seq` 0 whose key names a recorded run is answered by
     that record, or 409 `idempotency_conflict` when its members differ (step 3); one on an absent,
     malformed, or foreign stream is the 404 (step 4); only then is it 422 `range` (step 5). Placed
     at step 2, the same requests would each be 422.
   - **The alternative.** Treat `from_seq` 0 as a malformed member at step 2. §4.8.1's page
     contract is its precedent: a `limit` outside 1 to 1,000 is 422 `invalid` like one that is not
     an integer, a shape fault with no state read.
   - **Why the chosen reading still holds, and is tighter.** Both readings refuse every such
     request and record nothing, so neither admits a request the other refuses. A `limit`'s bounds
     are fixed numbers; `from_seq`'s upper bound is the stream's head, which only step 5 can read,
     so its bounds cannot all be checked at step 2. The chosen one holds every bound on `from_seq` (at least 1, at
     most the head) in one predicate at step 5, the check AU-8's test asserts with "an
     out-of-range `from_seq` or `to_seq` is refused". Split across steps 2 and 5, the lower bound
     sits in a shape check that a type such as an unsigned integer can satisfy with 0, and a 0
     that reached the run would make the prefix `seq` 1 to `from_seq − 1` underflow. It also keeps
     DEC-788 item 4's order intact: a reused key is always answered by its record or 409, and a
     foreign stream always by the 404, whatever the range says.

3. **The walk verifies control-stream anchor rows first, before the account stream's rows that
   depend on them.**
   - **The clause read.** §4.8.1's run order runs the per-event checks on the range, then reads
     the anchors at steps 2.1 and 2.2; DEC-787 items 1 and 2 put those anchors in the same snapshot
     but do not say when they are verified.
   - **The reading.** For a range on an account stream, the `AnchorComputed` rows of `ctl:{ws}`
     that the range's checks read are verified before any row of the account stream is judged
     against them, and a failure there fails the range. "Verified" means exactly what DEC-787 item
     3 applies to the trusted-start record: §11 check 1 (item 1's strict parse), check 2
     (`column_mismatch`), and check 4 (`rehash_mismatch`), in that order, on the row. It does not
     include checks 3, 5, or 6, which the walk runs only on the range's own rows. For a range on
     `ctl:{ws}` itself, those rows are range rows and the per-event checks reach them in `seq`
     order, as §4.8.1 says.
   - **The alternative.** Read each anchor row only when step 2.1 or 2.2 reaches it, and judge
     it by those steps alone.
   - **Why it is tighter.** No account-stream row is ever judged against an anchor row that has
     not itself passed checks 1, 2, and 4, so the walk never relies on a row that does not
     re-hash, even transiently. The alternative runs checks 2 and 4 on these rows nowhere, so a
     range it passes on an anchor row that does not re-hash fails here. Where both readings fail,
     only which code a range with several faults reports can differ, as DEC-786 says of the run
     order.

4. **`causation_id` is `null` on the `VerificationRun` the run produces,** until a later decision
   says what causes a run.
   - **The clause read.** Journal spec §3's envelope: `causation_id` is set by the writer, an event
     ID or `null`. §9.13 and rules 110 to 112, 132, and 133 say nothing of a `VerificationRun`'s
     cause.
   - **The reading.** Every `VerificationRun` the run appends has `causation_id` `null`, whatever
     its `trigger`.
   - **The alternative.** Name a cause: for example the `SegmentExported` behind a
     `segment_export` run, or the run's trusted-start record.
   - **Why it is tighter.** A `causation_id` is a claim that a trace follows (§4.8.1's causal trace) and
     no append rule checks for this type. `null` claims nothing, so no reader can be led from a
     run to an event that did not cause it, and no cross-stream or cross-workspace link (AU-1) can
     be introduced through it. A later decision that names a cause adds a checked link.

5. **Outages of the artifact store, until the cold-store drills settle them.** The artifacts a
   run reads are kept under `sha256:` refs (journal spec §6.3) in object-locked storage, timestamp
   tokens among them (§6.2); below, "the store" is that artifact store.
   - **(a) An artifact check 6 cannot read because the store does not answer fails
     `artifact_missing`.** This is §11's text, not a choice between readings.
     - **The clause.** §11 check 6: "`artifact_missing`, `artifact_mismatch` — every referenced
       artifact exists and re-hashes"; and §11 "Incomplete": "At this version one check can be
       incomplete, the token check". A referenced artifact the run cannot show to exist and
       re-hash has not passed check 6, and check 6 cannot end `incomplete` at this version, so the
       event fails `artifact_missing` at its `seq`. The tests pin it so that no run treats an
       outage as a pass or as `incomplete`.
     - **The cost, stated.** It is a `fail` result, so §11 "On failure" applies: a SEV-1, and on
       an account or agent stream the affected agents are paused, on the control stream a freeze
       of mandate and deployment changes. Under `AGENTS.md` rule 13, agent mode `paused` may hold
       exits and protective orders, so an outage of the store during a run can hold exits until
       the agents resume. A rerun once the store answers records the true result.
     - **The tension with DEC-787.** [DEC-787](DEC-787.md) rejected `segment_manifest_mismatch`
       for an unreadable cold store because "an outage is not tampering, and a false SEV-1 freezes
       the control stream for nothing". That rationale applies here too. DEC-787 could avoid the
       false SEV-1 because its case comes before the run and is a refusal (item 5); check 6 runs
       inside the walk, where §11 gives no outcome but a pass or `artifact_missing`.
     - **Changing it.** Moving an outage to `incomplete`, or to a refusal, would let a run end
       better than §11 says. It needs a change to journal spec §11, which is the founder's
       (DEC-79), not an agent reading.
   - **(b) An in-range anchor's token the run cannot read because the store does not answer does
     not stop the run, and the run is never `pass` or `incomplete` on it.**
     - **The clause read.** §4.8.1's run order step 2.2, the token check "for those anchors and for
       the trusted-start anchor"; §11 "Incomplete", whose outage arm ([DEC-896](DEC-896.md) item 3)
       is a reading for the trusted-start anchor. Neither says what an unread in-range token is.
     - **The reading.** The token check for that anchor neither passes nor ends `incomplete`, and
       the run goes on: the rest of step 2.2, then steps 2.3 to 2.5, run as §4.8.1 orders them. If
       any check fails, the run is recorded `fail` as usual, with that failure. The unread token
       is not a pass, so that range's result cannot be `pass`. Only if nothing failed is the run
       refused: 422 `invalid`, nothing recorded, retryable once the store answers. Where check 6
       already reads that token (a range on `ctl:{ws}`, whose in-range anchors are range rows),
       (a) applies first.
     - **Alternative (i): record the check `incomplete`, cause `token_unverifiable`,** as DEC-896
       item 3 does for the start anchor. When a later check fails, both record that `fail`. When
       none does, (i) records `incomplete`, on which DEC-789 item 9 lets a restore drill pass, so a
       drill could pass while the store is down; the chosen reading refuses and records nothing a
       drill or a reader could take as a result.
     - **Alternative (ii): refuse at step 2.2.** When no later check fails, both refuse. When one
       does, (ii) records nothing and raises no SEV-1, so a failure the run would have found goes
       unrecorded; the chosen reading records it.
     - **Why it is tighter than both.** In every case it ends in an outcome at least as strict as
       each alternative's, and stricter than each in one case: no failure found after the unread
       token goes unrecorded, and no `incomplete` or `pass` ever rests on an unread in-range token.
     - **A new outcome.** It adds a refusal after the run starts, which DEC-788 item 4's closed
       order of refusals (steps 1 to 7, all before the run) lacks. This decision makes that
       refusal; the open point below leaves only its violation name and spec text.
   - **What stays as decided.** DEC-896 item 3's `token_unverifiable` incomplete case for the
     trusted-start anchor, and the founder's DEC-789 item 9 rule for restore drills, are unchanged.
   - **Only the founder may relax this.** Moving (a) to `incomplete`, or (b) to a recorded
     `incomplete` or `pass`, would let a run, or a drill, end better than it does here, which
     loosens the rule; that is reserved for the founder (DEC-79). The cold-store drills may confirm
     it, or tighten it, by an agent decision.

### Consequences

E12-3's T1, T2, and T3 test PRs pin these readings: a malformed in-range `AnchorComputed` and a
malformed in-range `SegmentExported` each fail the range and are never skipped (item 1); `from_seq`
0 is 422 `range` at step 5, after the replay and the 404 (item 2); anchor rows are verified before
any account-stream row is judged against them (item 3); every produced `VerificationRun` has
`causation_id` `null` (item 4); an artifact check 6 cannot read because the store does not
answer fails `artifact_missing` (item 5(a)); and an in-range anchor's token the run cannot read
leaves the run going, so a later failure is recorded `fail`, while a run with no failure is
refused 422 with nothing recorded, never `pass` or `incomplete` (item 5(b)). A test
that would pass under the alternative reading of any item is not enough.

### Open points, settled before the tests that need them

These points are open. None of them is settled by the looser reading.

- **Item 1's failure code.** This decision fixes only `result: fail` for the range. The
  `failure.check` and `seq` are not fixed. `non_canonical` is per-event, and its `seq` would
  name a `ctl:{ws}` row outside the range. T2 (anchors) and T3 (segments) name the code in an
  amendment to this decision, and their tests pin it. Until then, no test may accept a
  `pass` or `incomplete` for such a range.
- **Item 3's failure code.** A verified anchor row that fails check 1, 2, or 4 fails the range,
  and this decision fixes only `result: fail`. As in item 1, the per-event codes would carry a
  `seq` of a `ctl:{ws}` row outside the range. T2 names the code in an amendment to this
  decision, and its tests pin it.
- **Item 5(b)'s refusal after the run starts.** This decision makes it: a run that cannot read an
  in-range anchor's token and finds no failure is refused, 422 `invalid`, nothing recorded,
  retryable. DEC-788 item 4's order of refusals (steps 1 to 7) has no step after the run starts.
  Only the `violation` name and the spec text that adds the step are open: a workspace API spec
  change, made before T2.
- **Journal spec §11's outage sentence.** The "Incomplete" outage sentence (DEC-896 item 3)
  should say it covers the trusted-start anchor only, so that it does not read as covering
  in-range anchors. This is a spec follow-up and changes no verdict here.

### Rationale

Items 1 to 4 each end in a failure, a refusal, or a record that claims less, where the other
reading could end in a skip, a pass, or a drill that passes. Item 5(b) records every failure the
run finds and refuses only when it found none, so it never ends better than either alternative.
Item 5(a) is §11's own outcome, at the cost item 5(a) states. Every refusal records nothing and is
retryable. The verdicts DEC-787, DEC-788, DEC-789, DEC-893, DEC-895, and DEC-896
give are unchanged.

### Alternatives considered

Each item names its alternative above. One more, for item 5:

- **Leave the outage cases unpinned until the drills.** Rejected: the run's tests would then
  accept either outcome, and the looser one could land unnoticed.
