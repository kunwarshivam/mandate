# workspace

DEC-2100 to DEC-2199. The rules in force:

- **Passkey step-up is the standard** for approvals, confirms, deployments, resumes and Stops;
  the interim `cli_confirm` challenge (half-open life, confirmation within it, fixed refusal
  order) covered connects and approvals until passkeys replaced it (DEC-2105–2108, DEC-2125–2128,
  DEC-2140).
- **The API server is axum** and serves the workspace's reads and commands: versions,
  confirmation, deployment, go-live, resume/Stop/owner exit, acknowledgments, alerts, holds,
  catalogue and instrument reads, mandate-version reads that fail closed on contradiction
  (DEC-2101–2103, DEC-2141–2152).
- **Identity comes from the hosted control stream**: `principal_id` on one's own session,
  memberships read from the control stream; a stub token issuer exists for dev and CI cells only
  (DEC-2113, DEC-2100).
- **The customer real-money gate is off by default**, set only by a platform operator, journaled,
  and necessary but not sufficient; real money is for the founder's own workspace and Robinhood
  account only (DEC-2134, DEC-2135).
- **Journal records carry the step-up evidence**: `StepUpAsserted`, `CredentialEnrolled` v2,
  `MandateConfirmed`/`AgentDeployed` v3 (DEC-2122, DEC-2131).
- A test cell may waive a first passkey's 24-hour cool-off and journals each waiver; staging's
  waiver expires no later than 2026-11-05 (DEC-2146, DEC-2150).


<a id="dec-2107"></a>

## DEC-2107: `mandate step-up confirm` refuses a late re-run before it looks for an earlier confirmation

| | |
|---|---|
| **Status** | Accepted (agent: a DEC-176 reading that only tightens [DEC-2106](DEC-2106.md) item 3; it adds no risk) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | `claude-code` (lane L1, claim [#759](https://github.com/kunwarshivam/mandate/issues/759)), on the #1876 review |
| **Story or claim** | E9-4 S2; claim #759; #1876; reads [DEC-2106](DEC-2106.md) items 2 and 3 |

### Context

DEC-2106 item 3 says a re-run with the same digest, principal and code finds its earlier
confirmation and appends nothing, as "the one exception" to item 2.9. It does not say whether
that exception also reaches past item 2.8, the deadline. Read one way, a re-run after the deadline
reports the earlier confirmation; read the other, it is refused `late`. The #1876 review asked for
the order to be settled in a decision rather than left to the tests.

### Decision

1. **Item 3 is an exception to item 2.9 alone.** Items 2.1 to 2.8 are judged first, in their
   order, on a re-run as on a first run. A re-run at or after the request's deadline is refused
   `late`, and one for a request no longer pending is refused `not_pending`, whatever the control
   stream already holds.
2. **Only then does item 3 apply.** An earlier `StepUpConfirmed` of the challenge with the same
   `action_digest`, `principal` and `code` is reported as committed, with nothing appended. Any
   other earlier confirmation of the challenge is refused `challenge_confirmed`. That includes one
   by another principal.

### Rationale

A confirmation after the deadline can authorize nothing: the grant it would serve is refused
anyway. Answering `late` tells the owner so, rather than reporting a record that will not count.
Keeping the exception to one item keeps the order DEC-2106 item 2 lists the only order.

### Alternatives considered

- **Item 3 first, so any re-run finds its record.** Rejected: it reports success for a request
  that can no longer be granted.

<a id="dec-2110"></a>

## DEC-2110: How the API server reads its control stream's rows into membership records

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176; every item only refuses or narrows, and none loosens a rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane MB, claim [#1623](https://github.com/kunwarshivam/mandate/issues/1623)) |
| **Story or claim** | E21-55; claim #1623; [workspace API spec](../../specs/workspace-api.md) §3.11 item 8; [identity spec](../../specs/identity.md) ID-3, §5; [journal spec](../../specs/journal.md) §2, §3, §9.12; [DEC-978](DEC-978.md) item 8; [DEC-991](earlier-900-1999.md#dec-991) items 2 and 4 |

### Context

`mandate-api-server` wires `members::until_parser`, which fails every membership lookup, so
every signed-in route answers `membership_unavailable` and no demo step past sign-in runs (E21-54
step 1). DEC-991 item 4 makes `mandate_identity::control_entry` the one mapping from a stored
envelope to a membership record, but says nothing of which rows the API server hands it: the
journal returns every row of the stream it was asked for, and `control_entry` reads one envelope
and knows neither the workspace nor the stream's environment.

### Decision

1. **Every row is read.** `members::membership_records(workspace, rows)` parses each row's body
   as a canonical envelope. A body that does not parse is `Unreadable` at its `seq`, whatever its
   stream or type: no row is passed over unread.
2. **Only the workspace's own stream counts.** A row whose envelope `stream_id` is not
   `ctl:<workspace>` is another workspace's record and says nothing here (journal spec §9.12: the
   control stream is the workspace's, and no record names its workspace). It is passed over,
   whatever its type, so another workspace's grant grants nothing and its revoke revokes nothing
   here.
3. **Every row of the workspace's stream maps through `control_entry`.** One that does not map,
   a `Member*` record or any other (a missing `event_time` included), is `Unreadable` at its `seq`.
   A record that is not a `Member*` type is `ControlEntry::Other` and adds no membership record.
4. **The stream's environment is its `StreamOpened`'s.** The workspace's first row must be
   `StreamOpened` at `seq` 1 (journal spec §2), and its envelope's `environment` is the stream's;
   DEC-991 item 2 writes the founding grant in that environment. A stream whose first row is not
   that is `NotOpened`. A stream with no row of the workspace's gives no record, which the lookup
   reads as no membership.
5. **A `Member*` record of another environment fails the read; any other record is passed over.**
   A `Member*` record in the workspace's own stream whose `environment` is not the stream's is
   `OtherEnvironment`, never skipped: passing over a revoke there would leave its member active
   (`AGENTS.md` rule 3). A record of another type in another environment adds no membership
   anyway, and is passed over.
6. **Only schema version 1.** A `Member*` record at any `schema_version` other than 1, the one
   journal spec §9.12 closes, is `UnknownVersion`; a later version is read only once a decision
   says how.
7. **Any refusal fails the whole lookup.** `members::control_parser(workspace)` is the
   deployment's `ParseMembers`: it answers each refusal as `LookupFailed`, so the route answers
   `membership_unavailable` and grants nothing (identity spec ID-3, workspace API spec §3.11 item
   8). The records are folded by `ControlLookup` at the request's instant, never at `recorded_at`
   or the parser's own clock: the parser reads no time.

### Rationale

The fold is only as safe as the rows it is given. Reading every row, refusing what does not map,
and passing over only what cannot be a membership of this workspace keeps "a record that cannot
be read is a failed lookup, never an empty membership" true at the parser as well as at the fold.

### Alternatives considered

- **Pass over a `Member*` record of another environment, like any other record.** Refused: a
  revoke written there would be dropped and its member would keep reading the workspace.
- **Fail the read on any row of another stream.** Refused: such a row is not this workspace's,
  so it cannot change a membership here, and failing on it would turn a store's stray row into a
  workspace outage. It is still read, so an unreadable one fails (item 1).
- **Read `stream_id`, `environment` and `schema_version` from the row's columns.** Refused: the
  body is what the chain hashes; the columns are copies of it.

<a id="dec-2114"></a>

## DEC-2114: The hosted workspace's founding bootstrap: the founder's sign-in link and founding grant, by the infra apply

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79; it adds no route, no grant beyond identity spec ID-13's founding exception, and no secret to the cluster; it loosens no rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane MB, on the coordinator's ruling; the chart and infra parts agreed with lane GO) |
| **Story or claim** | E21-58 (staging); [identity spec](../../specs/identity.md) ID-13, §3.2, §4.3; [journal spec](../../specs/journal.md) §9.12 rules 98 to 105; [workspace API spec](../../specs/workspace-api.md) §3.11 item 7; [DEC-991](earlier-900-1999.md#dec-991), [DEC-1023](DEC-1023.md), [DEC-2010](infrastructure.md#dec-2010), [DEC-2016](infrastructure.md#dec-2016) |

### Context

The founder's goal is a paper trade from the web app on staging. Staging's principal has no
membership, so the API answers every workspace route `not_found`; and staging sets no
`api.subjectLinks`, so the API mounts no subject-link file and refuses every sign-in (DEC-1023).
The cell does both by hand-run steps: `identity link` in a container on the host, then `workspace
open` and `workspace found` as Jobs (DEC-2010). The founder rules that no person runs anything
against the cluster by hand, and that the founder's identity-provider subject, which is personal
data, is never committed and never logged.

### Decision

1. **Two parts, both run by the infra apply.** Staging's founder is linked and founded by
   `cargo xtask infra` for staging and by the workspace chart, never by a hand-run command.
2. **The subject never reaches the cluster.** The founder's Supabase subject is the secret
   `FOUNDER_OIDC_SUBJECT` of the `infra-staging` GitHub environment, whose required reviewer is the
   founder; `infra-production` holds none.
   - It reaches only the `xtask infra` step's environment, as `MANDATE_FOUNDER_OIDC_SUBJECT`, with
     no `TF_VAR_` prefix, so OpenTofu never reads it.
   - xtask reads it once and masks it before any other use.
   - xtask runs the `mandate` binary the job built with no secret in its environment, `identity
     link --issuer <staging's api.issuer> --member <founder ULID>`, with the subject on standard
     input only: never an argument, an environment dump, or a log line.
   - The link file it writes holds one line, `sha256:<digest of {issuer, subject}> <ULID>`
     (DEC-1023). xtask applies it as the ConfigMap `subject-links` in the workspace's namespace,
     over `kubectl apply -f -` on standard input, so no argument carries even the digest. Staging's
     values set `api.subjectLinks: subject-links` once an apply has written it.
   - The ConfigMap is applied on every staging apply, empty when the secret is unset, so the secret
     alone decides who is linked, and rotating or removing it is a rerun.
   - The API server mounts the file by `subPath`, which the kubelet never refreshes. So when the
     applied content differs from the content before (read to compare, never printed) and the
     `api` Deployment exists, the apply restarts it. The apply may write; verify stays read-only.
   - A test of the xtask path feeds a canary subject and asserts it appears in no argument list,
     environment, output, or file it writes except as that digest.
3. **The founder's principal** is an opaque ULID committed in staging's values,
   `founding.member` (`01KDEM0000000000000000000F`), like the workspace's own id: it names no
   person, address, or subject (identity spec §3.1).
4. **The founding grant is a chart hook.** With `founding.enabled`, the workspace chart renders the
   Job `founding`, a Helm hook on `post-install,post-upgrade` with `before-hook-creation`, as
   migrate is. It runs as process `control` (its image, role, journal settings, egress policy, and
   pod security), with `workspace found --open --workspace <api.workspace> --member
   <founding.member> --method oidc`. It carries no subject.
   - `--open` opens a control stream holding no event, in `paper`, before the grant; a stream
     holding anything is never opened again (`workspace_not_new` and `members_exist` refuse as
     `found` does).
   - A re-run for the same member commits nothing and succeeds, so every upgrade may run it.
   - A refusal fails the hook, and so the release, loudly: Flux waits for Jobs, and the last Job is
     kept for verify.
5. **Verify names the refusal by its code only.** Staging's infra verify reports the `founding` Job
   by its Complete condition, as every `ws-*` Job, and a failed one by its log class. The founding
   refusal codes (`workspace_not_new`, `members_exist`, `workspace_not_open`) are classes of their
   own, so the line prints the code and never a log line. Everything stays inside verify's existing
   window.
6. **When the workspace is not new.** If staging's control stream holds records and no membership,
   the grant is refused `workspace_not_new`, and the conservative fallback is a fresh workspace ULID
   in staging's values (`api.workspace`), changed through gitops and recorded in a decision. No
   journal row is ever deleted, and no later-grant path is opened: that needs E9-7's member routes
   and an existing admin.

### Rationale

Linking from a digest computed on the runner keeps the personal datum out of the cluster, etcd, the
chart, and every log, which is stricter than a Secret mounted into a Job. Building the `mandate`
binary from the same commit as the API server keeps the digest's form identical on both sides. A
hook that re-runs an idempotent command is the chart's existing pattern for one-off work, and makes
a refused founding visible as a failed release instead of a silent Job.

### Alternatives considered

- **An in-cluster Secret holding the subject, read by the founding Job.** Rejected: the subject
  would sit in etcd and in a pod's files, and the link would still need a writer the API reads.
- **`docker run` of the published `mandate` image on the runner, as the cell does.** Rejected: the
  runner holds every infra secret and verifies no image signature there, and a registry pull may
  need credentials the job lacks.
- **A plain Job named by a hash.** Rejected: Flux waits for Jobs, so a failed one times the release
  out and never re-runs; a hook named by its role keeps verify's line stable.
- **An API route that founds a workspace.** Rejected for now: no spec row serves it, and the
  founding grant is the system's (journal spec §9.12 rule 98).

<a id="dec-2116"></a>

## DEC-2116: Staging's apply builds the CLI in its secret-free step, and writes the model provider's key with `vault put`

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79; it keeps every secret out of build scripts, arguments, files and logs, and loosens no rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane MB, at the coordinator's direction, with lane GO's #1958) |
| **Story or claim** | E21-32, E21-58 (staging); supersedes [DEC-2630](process.md#dec-2630) item 3's CLI sentence in part; builds on [DEC-2014](infrastructure.md#dec-2014), [DEC-2020](infrastructure.md#dec-2020), [DEC-2052](infrastructure.md#dec-2052) item 5, [DEC-2114](workspace.md#dec-2114) |

### Context

DEC-2630 item 3 says the CLI is built before the control-plane window opens, so the provisioning
step fits in it. It does not say where it is built, and the runner cannot build it in a container.
Staging's agents also need the model provider's key in the vault, and nothing wrote it there.

### Decision

1. **The CLI is built beside xtask.** It is `target/debug/mandate`, built by `infra.yml`'s build
   step before the control-plane window opens, so the step fits in it. That step has no secret in
   its environment, so no build script ever sees one (DEC-2014's rule for that step). This replaces
   DEC-2630 item 3's sentence "The CLI is built before the control-plane window opens, so the step
   fits in it"; the rest of item 3 stands.
2. **The model provider's key** (DEC-2020). `MANDATE_OPENROUTER_API_KEY` is set on the apply step
   alone, from the `infra-staging` secret `OPENROUTER_API_KEY`. It is never a `TF_VAR_` name, so
   OpenTofu never reads it. When it is not empty, the step masks it and writes it on every run with
   `mandate vault put model-provider-key`. That command takes the secret-import identity's
   credentials from its Secret and the key on the CLI's standard input alone. A rotation is the
   secret replaced and the apply run again. An unset key writes nothing. Production has no such
   secret.

### Rationale

DEC-2630 is Accepted, so only its Status line may change (decisions README). Building in the
secret-free step keeps the one rule that step already has. Writing the key with the same identity
and standard-input path as the rest of item 3 adds no new way for a secret to leave the vault's
path.

### Alternatives considered

- **Build the CLI in a container image.** Refused: the runner refuses Docker.
- **Pass the key as a `TF_VAR_`.** Refused: OpenTofu would read it and could keep it in state.

<a id="dec-2120"></a>

## DEC-2120: The web-push address store, `mandate-vault-addresses`: its own directory, key, and Unix users, a writer and a read-only reader

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane L2's builder agent (Claude Code) under DEC-79 and DEC-176, at the L2 lead's direction. The coordinator approved the crate and its layer, and the ruling in item 9 that a deactivation never blocks. Each item tightens how an address is held or who reaches it, or adds a store accepted decisions already require; none loosens a rule, weakens an invariant, or touches a founder-reserved matter |
| **Story or claim** | E8-14, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [workspace API spec](../../specs/workspace-api.md) §4.11, §5.7 ("The vault's part"); [journal spec](../../specs/journal.md) §9.15 ("The active set", rules 210 to 216); builds on [DEC-692](earlier-600-899.md#dec-692), [DEC-795](DEC-795.md), [DEC-2502](DEC-2502.md), [DEC-2503](DEC-2503.md) item 5, [DEC-2505](DEC-2505.md) |

### Context

Workspace API §5.7 says the vault holds each web-push address under its `address_ref`, answers
"which of this member's entries hold this endpoint" from a keyed hash it never exports, and lets
the sweep delete an inert entry by a conditional delete. No such store exists. The API server sits
at layer 7 and can take neither `mandate-vault` (layer 8) nor `mandate-vault-local` (layer 7, and
the token store). The dispatcher reads an entry for each send and must not be able to write one.
This decision sets where the store lives, who can reach it, and what each side can do.

### Decision

1. **A new crate, with its own users and directories.**
   - The crate is `mandate-vault-addresses`, layer 6, safety-critical. Its only external
     dependencies are `ring` (ChaCha20-Poly1305, HKDF, HMAC, `SystemRandom`), `secrecy`, `rustix`
     (`fs` only, for the opens of item 2; already a dependency of `mandate-vault`, which adds the
     `fs` feature, under `docs/dependencies.md`'s `rustix` row, to which the code PR adds this
     crate's purpose), and `thiserror`. It never takes `mandate-connections`,
     `mandate-vault-local`, `mandate-vault`, or any crate that names a token, a code, or a client
     secret. Only `mandate-api-server` and `mandate-dispatcher` may depend on it. The code PR that
     adds the crate also adds it to every `forbidden_internal` list in `xtask/layers.toml` that
     names `mandate-vault-local`.
   - The dispatcher runs as its own Unix user, `owlhead_notify`, in its own unit, never inside the
     API's or the executor's process. `owlhead_notify` is in neither `owlhead_api`'s nor
     `owlhead_exec`'s group, and neither of them is in its group.
   - Under `MANDATE_VAULT_DIR` (DEC-692 item 1's vault root, owned by root, `0755`):

     | Path | Owner and group | Mode | Who can do what |
     |---|---|---|---|
     | `addresses/` | `owlhead_api:owlhead_notify` | `2710` (setgid) | The API creates and removes workspace directories. The dispatcher can only pass through: it cannot list |
     | `addresses/<workspace_id>/` | `owlhead_api:owlhead_notify` | `2710` (setgid) | One per workspace. The API creates, replaces, and unlinks entries. The dispatcher opens an entry by a name it already knows and cannot list |
     | `addresses/<workspace_id>/<address_ref>` | `owlhead_api:owlhead_notify` | `0640` | One entry. The dispatcher reads it; nobody else |
     | `addresses/<workspace_id>/.lock` | `owlhead_api:owlhead_notify` | `0600` | The writer's lock (item 3) |
     | `addresses/<workspace_id>/.tmp-<16 hex>` | `owlhead_api:owlhead_notify` | `0640` | A write in progress (item 3) |

     The executor (`owlhead_exec`) can neither read nor list an address. The filesystem, not code,
     enforces that the dispatcher cannot write, as DEC-692 item 1 enforces that the API cannot read
     a token. `pending/` and `tokens/` are unchanged, and this crate never opens them.
   - **The keys.** The current key is the credential `vault-address-key`. During a rotation the
     retiring key is the credential `vault-address-key-previous` (item 6). Each is 32 bytes, a
     root-only `0600` file loaded with systemd `LoadCredential=` and read only from
     `$CREDENTIALS_DIRECTORY`, never from the environment. Both the API's unit and the
     dispatcher's unit load them. The API's unit keeps DEC-692 item 2's pending key and never gets
     the token key. The dispatcher's unit loads the address keys and nothing else.
   - **Opening refuses rather than repairs**, as DEC-692 item 3 does. Either handle refuses to open
     when any of these holds:
     - any of `MANDATE_VAULT_ADDRESS_KEY`, `MANDATE_VAULT_KEY`, `MANDATE_VAULT_PENDING_KEY`, or
       `MANDATE_VAULT_TOKEN_KEY` is set, whatever its value;
     - `vault-address-key` is missing;
     - either address key is a symlink, is not a regular file, is not exactly 32 bytes, or grants
       any group or other permission;
     - `vault-token-key` is among its credentials;
     - for the dispatcher, `vault-pending-key` is among its credentials;
     - the vault root is not owned by root with an `st_mode` of exactly `0o040_755` (DEC-692
       item 1);
     - `addresses/` has another owner or group, or an `st_mode` other than exactly `0o042_710`.

2. **How every file is opened.** Every path is opened relative to a directory descriptor, never by
   a path string.
   - At open, the handle opens the vault root and then `addresses/` with
     `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC` and checks each with `fstat`: the root as item 1's
     root-owned `0755`, `addresses/` as its `2710`.
   - Every operation opens `<workspace_id>/` relative to the held `addresses/` descriptor, the
     same way, and checks its owner, group, and `st_mode` (`0o042_710`) before using it.
   - An entry is opened relative to its workspace's descriptor in five steps; a failure at any
     step refuses the entry before decryption:
     1. `fstatat` with `AT_SYMLINK_NOFOLLOW` must show a regular file.
     2. `openat` with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, so a symlink, a FIFO, or
        a device can neither redirect the open nor block it.
     3. `fstat` on the open file must show the same device and inode as step 1.
     4. The link count must be exactly 1.
     5. The owner must be `owlhead_api`, the group `owlhead_notify`, and the `st_mode` exactly
        `0o100_640`.
   - A directory swapped in after the open is never followed, because no later open goes through
     a path. The users' ids come from the host's `/etc/passwd` and `/etc/group`, as
     `mandate-vault-local` reads them.

3. **Entries.** Entries are namespaced per workspace by directory and keyed by `address_ref`. Only
   a name that parses as a canonical ULID is an entry; `.lock` and `.tmp-…` never parse as one.
   Each entry is a cleartext header followed by a sealed body:

   | Part | Field | Meaning |
   |---|---|---|
   | Header (cleartext, authenticated) | `format` | `1` |
   | | `member` | The user principal whose address it is (journal §9.15's `member`), an opaque ULID |
   | | `version` | `1` on the first write; every write bumps it by one |
   | | `written_at` | UTC, nanoseconds, set by every write; the caller passes it, since the store reads no clock |
   | | `key_version` | Which address key sealed the body (item 6) |
   | Body (ChaCha20-Poly1305) | `endpoint` | The push endpoint (workspace API §5.7) |
   | | `keys` | `{p256dh, auth}` |
   | | `key_id` | The workspace VAPID key the subscription was made with (item 5) |
   | | `request_digest` | The step-up digest of the `PUT` that wrote it (§5.7) |
   | | `endpoint_hash` | Item 4's keyed hash |

   - **Sealing.** The body is sealed under the key `key_version` names, with a random 12-byte
     nonce from `SystemRandom`. The associated data is a domain tag
     (`mandate-vault-addresses/entry/v1`), the length-prefixed `workspace_id` and `address_ref`,
     and the whole header. An entry renamed onto another `address_ref`, moved to another
     workspace, or with any header field edited fails to open.
   - **What a failure means.**
     - A stored `member` that differs from the one asked for is not a fault. The reader answers
       `None`, exactly as for a missing entry. The writer's `put_if_absent` and `put_again`
       answer `Conflict`, and `entries_holding` skips the entry.
     - A header that does not parse, a body that does not open, or a file that fails item 2 is an
       error naming only the `address_ref` and a closed fault.
     - The member is compared before the body is opened. A tampered header naming the recipient
       still fails the seal.
   - **What the sweep reads.** `inert_older_than` and `compare_and_delete` read `version` and
     `written_at` from the header. They need no member and never open the body. A header edited
     to keep an entry alive fails the seal on its next read and is still deleted by its version.
     A candidate needs an old file as well as an old header (item 7): a `written_at` later than
     the sweep's `now` (a clock that ran ahead, or an edited header) makes an entry a candidate
     only when its file's modification time is also older than the cutoff, so a fresh write in
     flight is never one.
   - **Writes.** A write creates `.tmp-<16 random hex>` with `O_CREAT | O_EXCL`, mode `0640`,
     writes and syncs it, and renames it over the entry's name. The writer holds an exclusive
     `flock` on `.lock` across each read-compare-write, so the `version` checks and
     put-if-absent are atomic between API workers. The sweep unlinks any `.tmp-…` file older
     than its cutoff by the file's own modification time.

4. **The keyed endpoint hash.**
   - A per-workspace subkey is derived inside the crate with HKDF-SHA-256 from the address key
     that seals the entry. The info string is `mandate-vault-addresses/endpoint-hash/v1` followed
     by the length-prefixed `workspace_id`.
   - The hash is HMAC-SHA-256 under that subkey over the length-prefixed `member` and `endpoint`.
     It is scoped to the member as well as the workspace: one endpoint held by two members, or in
     two workspaces, hashes differently.
   - `entries_holding` computes the hash under each entry's own `key_version` and compares in
     constant time.
   - The hash is stored inside the sealed body and deleted with the entry. No operation returns
     it, logs it, or hands it to a caller to journal, and the subkey never leaves the crate.
   - Every write recomputes it under the key that seals the body, so a re-seal (item 6) or an
     entry's next write after a rotation stores it under the new key's subkey.
   - It is kept although the open body holds the endpoint, because §5.7 says the vault answers
     the endpoint question from a keyed hash, and comparing two fixed-length MACs in constant time
     needs no comparison of endpoint strings of different lengths. Dropping it would need §5.7
     changed first.

5. **Where `key_id` comes from** (DEC-2503 item 5). The web client sends the id of the workspace
   key it subscribed with, and the API refuses an id that is not the workspace's current key.
   - What the API stores is therefore always the workspace's current VAPID key id, read by the API
     at the write. It is a deployment value equal to the request's claim, not a choice the member
     can vary.
   - That is why §5.7's step-up digest, over `{channel, action, endpoint, keys}`, need not bind it.
     A retry after a rotation fails the "current key" check rather than storing another id.
   - §5.7's list of what the vault holds omits `key_id`. A backlog row owes that spec line.

6. **Key rotation and restore.**
   - **One slot.** A handle holds at most two keys: the current `vault-address-key` and, during a
     rotation, the previous `vault-address-key-previous`. Writes seal under the current key. Reads
     open an entry under whichever key its `key_version` names.
   - **Rotation.** A new `vault-address-key` is installed and the old one moves to
     `vault-address-key-previous`, in both units at once. A rotation is refused while any entry
     remains under the previous key, so the one slot never holds two retiring keys and no entry
     is orphaned.
   - **Re-sealing.** The re-seal tool runs as `owlhead_api`, the one user that can write
     entries. For each entry under the previous key it takes the workspace's lock, opens the body,
     re-seals it under the current key with its `endpoint_hash` recomputed under the current key's
     subkey (item 4), and replaces the entry only while its `version` is still the one it read,
     bumping it as every write does. An entry's next ordinary write re-seals it the same way.
   - **Retiring the previous key.** The previous key retires only when the re-seal tool reports
     zero entries under its `key_version`. It stays loaded for at least 30 days; the 30 days are a
     minimum, not a deadline. The first rotation is blocked until the re-seal tool exists. The
     30-day minimum does not apply to a compromised key, which retires as soon as the tool reports
     zero, or at once (below).
   - **Emergency rotation.** An emergency overrides "a rotation is refused while any entry
     remains under the previous key". After a compromise of the previous key, it is dropped at once,
     without waiting for the re-seal. After a compromise of the current key, a new key is installed
     at once as the current key, the compromised one becomes the previous key, and the older
     previous key, if any, is dropped at once. Every entry still under a dropped key is then a
     `RetiredKey` fault, which the dispatcher records as `address_missing` and which raises
     `channel_lost`, and those members set their addresses again (DEC-795 item 7).
   - **An entry the re-seal tool cannot open.** The tool never erases. It lists each entry it
     cannot open (a file check, a header, or a seal that fails) by workspace id and `address_ref`
     in its report, exits non-zero, and raises one operator alert carrying those opaque ids only.
     Such an entry counts as remaining under its key, so it blocks the previous key's retirement
     and the next rotation until an operator erases it by `address_ref` (the writer's `erase`, item 7). One
     corrupt entry therefore blocks rotation loudly, never silently.
   - **The key version.** `key_version` is the first 8 bytes of HMAC-SHA-256 under the key over
     `mandate-vault-addresses/key-version/v1`. It names a key without revealing it, and needs no
     separate configuration.
   - **Restore**, as DEC-692 item 8 does. Backups carry `addresses/` with its owners and modes, and
     never `/etc/owlhead` or the credentials. After a restore:
     - an entry removed since the backup comes back, but the journal holds its `removed`, so the
       sweep erases it (item 9);
     - an entry added since the backup is gone, so its address reads `address_missing` until the
       member sets it again;
     - without the keys, every entry is unreadable and every member sets their addresses again.
     The restore runbook discloses this.

7. **Operations, by type.**
   - **`AddressWriter`**, the API's handle:
     - `put_if_absent(entry)` writes a new entry at `version` 1. An entry already under that
       `address_ref` with the same member and request digest is the same request: it answers the
       entry's current version and writes nothing. Another digest or member is `Conflict`, which
       the API answers `idempotency_conflict` (§5.7 setting step 1).
     - `put_again(entry)` is §5.7 setting step 6. It writes the same entry again, bumping
       `version` and `written_at`, and recreates it if a sweep deleted it meanwhile. Another
       digest or member is `Conflict`. An entry it recreates for an address that was removed
       meanwhile has no active `added`, so it lingers until the next sweep finds it older than 24
       hours. It holds no further right; this is retention only.
     - `entries_holding(member, endpoint)` returns the `address_ref` and `version` of each of that
       member's entries whose `endpoint_hash` matches. It never returns another member's entry.
       The caller keeps only the active ones (§4.11).
     - `compare_and_delete(address_ref, version)` deletes the entry only while its header's
       `version` is the one given. It answers `Deleted`, `Stale` (a later write), or `Absent`.
     - `inert_older_than(cutoff, now)` returns the `address_ref`, `version`, and `written_at` of
       each entry whose file was last modified before `cutoff` and whose `written_at` is before
       `cutoff` or later than `now`, and unlinks `.tmp-…` files last modified before `cutoff`. It
       lists; it judges nothing. The store cannot tell an inert entry: the sweep (A4) decides which
       candidates to erase by item 9's positive evidence and erases each with
       `compare_and_delete`, so an entry rewritten after the listing has a new version and stays.
     - `erase(address_ref)` deletes the entry unconditionally (`Deleted` or `Absent`), for the
       redaction or erasure that the event's `pii_refs` drive (journal spec §6.4).
     - `purge_workspace()` removes the workspace's directory and every file in it, for workspace
       deletion.

     Every operation takes the workspace id, so a missing argument can never make one handle reach
     across workspaces.
   - **`AddressReader`**, the dispatcher's handle, offers one operation:
     `entry(workspace_id, recipient, address_ref)`. It returns `None` when no entry exists or when
     the header's `member` is not the recipient, so another member's entry reads exactly as a
     missing one. An item 2 refusal, a malformed header, a seal that fails, or a retired key is an
     error naming only the `address_ref`. The endpoint, keys, and `key_id` come back as `secrecy`
     types for one send.
   - **The reader is read-only by type.**
     - `AddressReader` has no write, delete, erase, purge, or list method, and cannot be converted
       into or obtained from an `AddressWriter`.
     - `compile_fail` doctests pin both. Each is paired with a doctest that compiles the same code
       against the writer, so a `compile_fail` test cannot pass for an unrelated error.
     - The filesystem holds the same line if the code ever drifted: `2710` directories, `0640`
       entries.

8. **Nothing secret in any output.** No endpoint, key, `key_id`, request digest, hash, subkey, or
   key material appears in a log, an error, a `Display` or `Debug` output, a metric, or a
   notification (`AGENTS.md` rule 6, notifications spec NT-2). Errors carry opaque ids only: the
   workspace id, the `address_ref`, and a closed reason. A canary test plants a recognizable
   endpoint and keys and scans every error and `Debug` output the crate can produce.

9. **Which addresses are active, deactivation, and the one fold.**
   - **Serving: the fail-closed definition.** An address is active if and only if all of these
     hold:
     1. the member has a start (`MemberActivated` or `MemberReactivated`);
     2. the `added` seq is greater than the member's latest start seq;
     3. no end (`MemberDeactivated` or `MemberRemoved`) has a seq above that start;
     4. no matching `removed` (same member and `address_ref`) follows the `added`.

     With no start, nothing is active. A `removed` naming an `address_ref` that is not active, or
     another member's, changes nothing. `cooling_off` has no end record, so a cooling-off member's
     addresses stay active. The dispatcher (who is sent to), the API (the 10-address
     limit, an endpoint already held, the listing, and 404 against 200 on removal), and E9-7's
     helper use this definition, and send, list, count, and match nothing that fails it.
     Disregarding an address is the reversible tightening: it is stricter than §4.11's "last event
     is `added`", and a disregarded address is only ignored, never destroyed, so it adds no risk
     (DEC-176).
   - **Erasing: positive evidence only.** Deletion is not reversible, so the sweep never erases an
     entry merely because it is not active. It erases an entry only on positive evidence from a
     fold that succeeded and covers the entry's `written_at`, one of:
     1. a matching `removed`, after any last send ends, or 24 hours after the `removed`;
     2. the member's latest start followed by a `MemberDeactivated` or `MemberRemoved`;
     3. the `added` seq before the member's latest start;
     4. no `added` at all, for an entry whose `written_at` and file are both older than 24 hours
        (the inert case, item 7).

     Evidence 2 and 3, an address dead only because a membership ended, waits a further 7-day
     grace: evidence 2 from the end record's time, evidence 3 from the later start's time (a
     reactivation after the membership that added the address ended). A member with no start, a fold that fails, or a journal
     read that fails means keep the entry, and raises one operator alert carrying opaque ids only.
   - **Removing a dead address.** A removal of an address that is not active only because a
     membership ended, with no `removed` of its own, answers `200` with the watermark of the
     `MemberDeactivated` or `MemberRemoved` that ended it, as removing an address already removed
     does (§5.7).
   - **Reactivation.** An address added before a membership ended is dead for good, whether or not
     the member is reactivated: it is never listed, counted, matched, or sent to again. The member
     is told to add an address again by the in-app notice identity spec §5.2 step 3 already gives a
     reactivated member (`web_inbox`), never by push, since push is the out-of-band takeover signal
     (identity spec ID-14).
   - **One fold.** There is one active-set fold, and L3 writes it: `ActiveAddresses` in
     `mandate-notify`, deterministic, built by a parse-only constructor that fails closed over the
     `NotificationAddressChanged` records and the members' membership starts and ends, and
     implementing the serving definition and the evidence the sweep needs. Neither this crate nor
     `mandate-identity` folds addresses.
   - **No vault writes on deactivation.** E9-7's membership writer (lane L1) appends the `removed`
     records for a member's active addresses inside its own commit (§4.11 "Deactivation", DEC-795
     item 5), and touches no vault. It computes them with
     `removals_on_deactivation(&deactivation, active_refs)`, which E8-14's slice A2 adds to
     `mandate-identity` (layer 1), with these properties:
     - It takes the `MemberDeactivated` or `MemberRemoved` being committed, and accepts only those
       two by type.
     - It copies that record's actor and member; there is no free actor argument.
     - It drafts no step-up member and no `OwnerAlertSent`.
     - `active_refs` must be L3's fold output for that member, in add order, read at the same
       control-stream head `h` that the batch appends with as `expected_head`.
     - It drops repeated references (journal rule 214(d)), and its drafts come out in the input's
       order; a property test asserts that order.
   - **A fold error never blocks a deactivation** (the coordinator's ruling). L3's fold fails
     closed with an error, never an empty set. On that error, the writer commits the
     `MemberDeactivated` or `MemberRemoved` without the removals and raises one operator alert
     carrying opaque ids only. That path is E9-7's writer's, not the helper's, and a test of the
     writer must pin it. Under the serving definition, the member's addresses are no longer active
     once the deactivation commits, with or without the removals, so the dispatcher sends them
     nothing; the sweep erases their entries on evidence 2, after the 7-day grace.
   - **When entries go.** An entry with no last send (DEC-795 item 5: a deactivation's removals,
     and a replacement's) goes at the first sweep after it is 24 hours old, or after the 7-day
     grace for evidence 2 and 3. A removal with a last send keeps its entry until that send ends.
   - **The specs follow.** Workspace API §4.11 ("Active addresses" and "Deactivation") and §5.7's
     sweep text, identity spec §5.2 step 3, and journal §9.15's active set do not yet state the
     fold-error case, the serving definition, or the sweep's positive-evidence rule. A spec-only PR
     (DEC-2123, reserved in that PR) amends them (a backlog row). Until it lands, the code follows
     this item, which serves only less than the specs' text, and deletes only on positive
     evidence, after a grace.

10. **OpenBao comes later.** `mandate-vault`'s `SecretPath` gains an `Address` path, so the
    address store has an OpenBao backend behind the same two handles; this is a backlog row under
    E21-32. Until it lands, the demo host and the cell use this local store.

### Rationale

The endpoint and keys are the member's personal data, and the push path is their only reader. A
directory and a key of their own keep a mistake in this store from reaching tokens, and keep the
token key out of a process that has no use for it. Writing the dispatcher's read-only line into
both the type and the filesystem means neither a code change alone nor a misconfigured unit alone
lets the dispatcher alter where notices go.

Putting the member in an authenticated cleartext header makes "not yours" a plain comparison and
"tampered" a cryptographic failure, so the two are never confused. It also lets the sweep work
without the body. Binding the member into the associated data and into the reader's answer means
one record or one bug cannot route a member's notices to another member's browser.

### Alternatives considered

- **Put addresses in `mandate-vault-local`'s `tokens/` under a new kind.** Rejected: the API cannot
  read `tokens/` by design (DEC-692 item 1), and the dispatcher would need the token key.
- **Have the API server take `mandate-vault`.** Rejected: layer 8 is above it, and it carries the
  token paths.
- **Let the dispatcher read as `owlhead_api`.** Rejected: it could then write and delete entries,
  and redirect a member's notices.
- **Seal the member with the body.** Rejected: another member's entry would then be
  indistinguishable from a tampered one, and the sweep would need the key to read a version.
- **`2750` directories.** Rejected: the dispatcher would be able to list every address of a
  workspace, and it never needs to.
- **One endpoint-hash key for every workspace, or a hash over the endpoint alone.** Rejected: an
  equal hash would link one browser across members or workspaces.
- **The hash in the file name, for a lookup without decryption.** Rejected: the name would then
  reveal which entries share an endpoint to anyone who can list the directory.

<a id="dec-2132"></a>

## DEC-2132: The local cell's API takes `localhost` as its WebAuthn RP ID

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176; it admits one origin no hosted deployment uses, and grants no capability) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code` (coordinator) |
| **Story or claim** | E9-4; [DEC-2108](DEC-2108.md) item 1; [DEC-640](DEC-640.md); the cell's `deploy/cell/values/k3d.yaml` |

### Context

Since #2403 the API server derives its WebAuthn RP ID from its app origin at startup and refuses to
start when there is none ([DEC-2108](DEC-2108.md) item 1). `reduction::rp_id` takes the host of an
`https` origin only when the host has a dot. The local cell's API runs with the web app's origin,
`https://localhost:4317` (`deploy/cell/values/k3d.yaml`), where `next dev` serves `web/` for the
cell's browser tests. `localhost` has no dot, so the cell's `api` pod exits with "the app origin
names no relying party", helm's `--wait` times out, and every cell drill and `cell-demo-web`
(the trade loop's browser test) fails before it starts.

### Decision

1. **`rp_id` also admits the host `localhost`**, of an `https` origin, with or without a port.
   Every other single-label host stays refused, as does every `http` origin.
2. **This extends [DEC-2108](DEC-2108.md) item 1 and identity spec §6.1**, which name the hosted
   deployments' RP IDs: a local deployment's RP ID may also be `localhost`, its origin
   `https://localhost` with an optional port, as a hosted origin may carry one.
3. Nothing else changes: the hosted deployments' values (DEC-640, DEC-2108 item 1) are unchanged,
   and a server whose origin names no RP ID still refuses to start.

### Rationale

- WebAuthn admits `localhost` as an RP ID, and browsers treat it as a secure context. A passkey
  bound to it signs for that machine's origin alone, so admitting it lends no other origin
  anything.
- The alternative, serving the cell's web app at a dotted `*.localhost` host, changes the chart,
  the xtask's web origins, Playwright's base URL and the TLS certificate. Name resolution for such
  hosts outside a browser depends on the host's resolver, which the cell's CI runners do not
  promise.

<a id="dec-2135"></a>

## DEC-2135: Real money for the founder's own workspace and Robinhood account only, with an always-on agent

| | |
|---|---|
| **Status** | Accepted (founder-reserved under DEC-79: live money and narrowing a safety rule). The founder answered the coordinator's questions in the coordinator's session on 2026-10-26: "Just me first (Recommended)" at 08:21 UTC, "Carry over, $100 cap (Recommended)" at 08:27 UTC, and "Runner first (Recommended)" at 08:33 UTC. Recorded by lane LV |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | The founder, in the coordinator's session on 2026-10-26, answering the coordinator's questions; recorded by lane LV (Claude Code) |
| **Story or claim** | E7-6, E7-26, E21-30; [DEC-529](DEC-529.md), [DEC-2382](trading-core.md#dec-2382) items 2, 6 to 9, DEC-2134 (lane GL's customer real-money gate, not yet on `main`), [DEC-952](earlier-900-1999.md#dec-952), [DEC-470](earlier-303-599.md#dec-470); `AGENTS.md` rules 2, 3, 7, 8, 11 and 13 |

### Context

DEC-529 lets the founder place one live order on their own Robinhood agentic account. DEC-2382
lets the cell's executor carry the Robinhood connector for that account, and its item 8 lifts
"one order, then stop" for an always-on agent. Item 8 also read DEC-529's "for this order only"
narrowings as carrying to every order of that agent; lane LV recorded that reading as Proposed,
because it loosens. The founder then answered two questions.

The first question was "Who trades real money first?" The founder answered **"Just me first
(Recommended)"**. The option read: "You trade real money on your own Robinhood account first;
customers stay on Alpaca paper until you decide later." The option not taken was "Me and customers
now".

The second question was "For your real-money Robinhood agent: should the one-order safeguard
relaxations (unsigned build, command-line confirm, one-time statement) carry over so the agent can
keep trading on its own?" The founder answered **"Carry over, $100 cap (Recommended)"**. The option
read: "Agent keeps trading real money on its own, but only within the $100 cap, one ETF, and you
approving each opening trade (ASK). Risk gate and kill switch unchanged." The options not taken
were "One order at a time" and "Carry over, raise cap".

The third question was "Where should your always-on real-money agent run first?" The founder
answered **"Runner first (Recommended)"**. The option read: "Fastest: ~4 PRs. One program on your
machine trades real money on Robinhood. The managed cell path follows after, as the product
version." The option not taken was "Cell first".

### Decision

1. **Real money for the founder only.** Real-money trading is allowed only in the founder's own
   workspace, on the founder's own Robinhood agentic account. Every customer stays on Alpaca
   paper, and no customer has a real-money path. Whatever gate lane GL's DEC-2134 defines must
   stay off for every customer; nothing here turns it on.
2. **The carry-over is accepted.** DEC-2382 item 8's paragraph "DEC-529's 'for this order only'
   items" is Accepted, as the founder's answer. It covers DEC-529 item 3's narrowings (the
   unsigned build, `cli_confirm`, the FR-2.6 attestation), item 4's recovery rule, and items 5, 8
   and 12. They carry to every order of the founder's always-on agent on the founder's own
   account, and to no other agent or account.
3. **The limits stay.** They are set in the founder's confirmed mandate:
   - `max_order_usd` 100 USD, on one broad ETF (DEC-529 item 2), with a small allocation the
     founder sets and confirms;
   - `ask` for every opening, each waiting for the founder's own grant;
   - the agent's first start needs a flat account (DEC-470 item 1), and a restart reconciles
     first, as DEC-2382 item 8 says;
   - the risk gate, the US account rules, the kill switch and `AGENTS.md` rules 2 and 13,
     unchanged.
   A larger cap is a new decision of the founder's.
4. **The credential is the founder's alone.** The founder enters their Robinhood credential
   themselves: into the vault on the executor's path (DEC-2382 item 7), or by logging in through
   the browser on the runner's path (DEC-529 item 10). No agent asks for, reads, holds or uses it.
5. **Going live is the founder's step, and it is journaled.**
   - On the runner's path, the founder opens a live workspace on their own journal (DEC-952),
     records the Robinhood connection with the account attestation, and confirms and deploys the
     mandate. Each step is a journaled gesture behind a shown code, and `--confirm-live` is on the
     run.
   - On the API and cell path, the enable has no way in except the founder's own workspace. The
     requirements this sets on DEC-2134 are under Consequences.
6. **Agents are unchanged** (`AGENTS.md` rule 8). No agent reaches Robinhood's host, places a
   real order, runs the live deployment or reads its output, or touches a live credential. Agents
   test only against fixtures synthesized from Robinhood's published contract and the existing
   `mandate-rh-sim` (DEC-529 item 6, DEC-441 item 11, DEC-2382 item 2); nothing is recorded from
   a real account.

### Rationale

The founder wants the product working on their own money now, and customers on Alpaca paper
"until you decide later", in the words of the option they chose. DEC-529 item 9's counsel answer
for the founder's own account is unchanged.
Keeping real money to one workspace and one account, a 100 USD cap, one ETF and a grant on every
opening bounds the loss and keeps each decision with the person whose money it is. The carry-over
is what lets the agent keep working, rather than stop after one order.

### Alternatives considered

- **Me and customers now:** offered and declined by the founder.
- **One order at a time:** offered and declined. The agent could not run on its own.
- **Carry over, raise cap:** offered and declined. The cap stays 100 USD.
- **Cell first:** offered and declined. The cell path follows as the product version.

### Consequences

- DEC-2382's status line points here: its item 8 carry-over reads Accepted. Where DEC-2382's
  body still says that paragraph is Proposed (item 8 and "What this amends"), this decision
  supersedes it, and the status line governs.
- DEC-529's status line points here.
- **The order of the work changes; DEC-2382 item 9 is amended,** by the founder's third answer.
  Lane LV builds first on the runner the founder already has (DEC-529's R0): the always-on loop
  ([DEC-2390](DEC-2390.md)), the sell's booking (DEC-2300), the CLI kill switch (DEC-1152), and
  the runbook. The runbook does not tell the founder to leave the agent unattended until the CLI
  kill switch lands; until then the kill switch is Ctrl-C and the Robinhood app. The cell path
  follows as the product version: DEC-2386 slice C and DEC-2382 items 1, 4 and 7. On 2026-10-26
  the coordinator also relayed that the founder stopped new simulator work ("Enough with
  simulator, we have wasted so much time"; relayed, not seen in the founder's own words by lane
  LV), so DEC-2382 item 6's `simulated` environment and DEC-2386 slice B are not built now.
  Deferring them adds no risk, and they return if the founder asks. DEC-124 and DEC-529 item 3's
  simulated-first run on the existing `mandate-rh-sim` are unchanged: only new simulator work is
  deferred. Each slice is tests first,
  has an independent review and has zero missed mutants.
- **What this requires of DEC-2134.** Lane GL's gate gives the API and cell path no way to real
  money except the founder's own workspace. Only the founder, as operator, turns it on for that
  workspace. No HTTP route, workspace role, client or agent can turn it on. It journals each
  change and each refusal. DEC-2134 defines how the founder's workspace is marked.

