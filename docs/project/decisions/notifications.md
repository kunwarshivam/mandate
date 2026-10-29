# notifications

DEC-2500 to DEC-2599. The rules in force:

- **The dispatcher is a managed-cell process** reading its subject streams from the control
  stream; a control-stream alert is the cause when alerts share a notice key; email goes only
  through the recorded fixture in dev and CI (DEC-2500, DEC-2505, DEC-2516).
- **Web push**: addresses resolve through the vault entry and active set (`mandate-vault-addresses`,
  its own directory, key and Unix users); one VAPID key set per workspace in the vault, generated
  there, read once by the dispatcher; endpoints only on the browsers' own push services; the
  relay carries the deployment's VAPID header with a role or platform `sub`, never a person's
  (DEC-2502, DEC-2503, DEC-2506, DEC-792, DEC-726, DEC-727).
- **Payloads carry only what rule 6 allows**: closed alert kinds only, `ApprovalRequested` v3
  names the checks the dry run deferred and the decision it came from; a disagreement between
  request and decision is unreadable, and the respond route refuses such a grant
  (DEC-2514, DEC-2515, DEC-2518–2521).
- **Acknowledgment**: passkey step-up is accepted wherever `cli_confirm` is, in live too; the CLI's
  acknowledgment checks read in a fixed refusal order (DEC-2523, DEC-2512, DEC-2513).
- The mail guard admits no transport but the recorded fixture and the founder's; a distinct alert
  kind exists for a connection the platform cannot reach or use (DEC-2511, DEC-2520).


<a id="dec-2501"></a>

## DEC-2501: `ApprovalRequested` as a dispatcher cause, for E8-10 slice D7a

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | The L3 builder agent (Claude Code) under DEC-79 (reversible engineering readings) and DEC-176 (each item fills a gap with the reading that adds no risk: it sends nothing the spec does not already send, drops no notice, and changes nothing the owner is told) |
| **Story or claim** | E8-10 slice D7a (`ApprovalRequested` causes), claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [notifications spec](../../specs/notifications.md) v0.7 §3.2, §3.3, §3.4, §4.2, §5.1, §5.3, NT-1, NT-8, NT-10, NT-11; [journal spec](../../specs/journal.md) §9.6, §9.15 rules 124 and 125; builds on [DEC-704](earlier-600-899.md#dec-704) items 2 and 4, [DEC-705](earlier-600-899.md#dec-705), [DEC-707](DEC-707.md) item 1, [DEC-710](earlier-600-899.md#dec-710) item 5, and [DEC-725](DEC-725.md) item 7 |

### Context

Slices D2 to D4 (DEC-704, DEC-707) read only `OwnerAlertSent` as a cause. Spec §3.4 and §5.1 step
1 name `ApprovalRequested` as the other committed cause, §3.2 gives its row (`approval_requested`,
class `action`, text key `approval_needed`, trigger the agent stream's `ApprovalRequested`), and
journal spec rule 125 refuses an approval notice whose `cause_stream` is not an agent stream of
the notice stream's workspace. Five points are left open: which rows the step reads as approval
causes, their notice key, their recipients before roles, how long their sends are retried before
slice D7b, and what of the request the notice may carry.

### Decision

1. **The cause.** A committed `ApprovalRequested` is a cause exactly when it is on an agent stream
   among the step's subject streams (`Config::subjects`, which already lists the workspace's agent
   streams). A row of that type on any other subject stream is no cause and is skipped: the
   journal's catalogue admits none there, and issuing one would have rule 125 refuse the step's
   whole issue batch, holding back every alert in it. Both schema versions (1, and 2 with the order
   builder's plan, DEC-942) are causes alike. The step reads only the row's envelope (`event_id`,
   `event_type`, and the stream it is on), never its payload.
2. **The key and the notice.** An `ApprovalRequested`'s notice key is its own event id (spec §3.4;
   it names no owner command, so DEC-725 item 7's rule gives the same answer). One notice per
   request: a request that already has a `NoticeIssued` is skipped, across steps and restarts. Its
   `NoticeIssued` has `kind` `approval_requested`, `class` `action`, `cause` and `causation_id`
   the request's event id (rule 124), and `cause_stream` the request's agent stream (rule 125). It
   joins the step's one issue batch at the place its key is first read (subject streams in the
   order given, each in `seq` order, DEC-704 item 2), and the batch commits before any send. Each
   send keeps its one idempotency key, the digest of `(notice, recipient, channel)` (spec §5.1,
   DEC-710 item 5).
3. **Recipients before roles.** Until roles exist (E9-2) and the receive column is read (NT-10),
   an approval notice goes to the configured audience, as every notice does under DEC-704 item 4:
   spec §3.3's interim says that before roles every row resolves to the founder, and the
   `ApprovalRequested` names no approver ids to narrow it by (only `approvers_required`). Once
   roles exist the recipients are the approval's approvers within the receive column, a later
   slice. Being sent a notice grants nothing (NT-11).
4. **Retries before D7b.** An approval notice's sends follow spec §5.3's schedule with no window,
   and are never `abandoned`, until slice D7b adds the `not_pending` stop (DEC-707 item 1). D7a
   drops no approval notice early.
5. **What the notice carries.** The notice is the minted notice id and the text key
   `approval_needed` and nothing else (spec §4.2, NT-1): never the request's instrument, side,
   quantity, prices, order value, score, rule, plan, deadline, content hash, agent, or event id.
   Its rendered text, "An agent in your workspace needs your approval", is fixed by spec §4.2,
   mandate spec §6.4, and PX-6, so this decision changes nothing the owner is told (DEC-723 stays
   with the founder).

### Not decided here

- **The `not_pending` stop** (slice D7b): when an approval stops being pending, and whether a
  request already decided, timed out, or canceled when the dispatcher first reads it is still
  issued (spec §5.1 step 4 issues every cause with no `NoticeIssued`) and then abandoned at once.
- **`approval_reminder`** and **`approval_closed`**, each its own later slice.
- **A subject stream of another workspace in `Config::subjects`.** Rule 125 would refuse its
  notice as for any cause; how configuration is checked is the dispatcher process's (slice D10).

### Rationale

Item 1 is spec §3.2's "agent stream" read so that a misplaced row can never stall the safety
notices beside it. Items 2 and 3 apply the rules every cause already follows. Item 4 restates
DEC-707 item 1 for the notices that now exist. Item 5 restates NT-1 and §4.2, and reading no
payload member makes it hold by construction. None touches trading state (NT-5): a grant is still
judged only by mandate spec §6.4's checks.

### Alternatives considered

- **Read `ApprovalRequested` on every subject stream.** Rejected for item 1's reason: rule 125
  refuses the batch, and every alert in it would wait.
- **Key an approval notice by its content hash.** Rejected: spec §3.4 keys every notice by its
  cause's event id, and the hash is request content, which nothing on the notice stream may carry.
- **Issue approval notices in a batch of their own.** Rejected: DEC-704 item 2's one batch per
  step already commits every new notice before any send; a second batch would add an append that
  could fail between two issue groups.

<a id="dec-2506"></a>

## DEC-2506: The web-push provider's ports and its readings (E8-10 slice D10b)

| | |
|---|---|
| **Status** | Accepted (items 1 to 7, by the agent under DEC-79 and DEC-176: each is a reversible engineering reading that sends nothing more, marks no address the spec does not, and adds no risk; none changes what the owner is told, so DEC-729 item 4 and DEC-2504 stay with the founder) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | The L3 builder agent (Claude Code) under DEC-79 and DEC-176 |
| **Story or claim** | E8-10 slice D10b and E8-14, claim [#753](https://github.com/kunwarshivam/mandate/issues/753); [notifications spec](../../specs/notifications.md) v0.8 §4.6, §5.2, §5.9, NT-1, NT-2; builds on [DEC-2500](DEC-2500.md) items 6, 7 and 9, [DEC-2502](DEC-2502.md), [DEC-2503](DEC-2503.md), [DEC-2504](DEC-2504.md), [DEC-728](DEC-728.md), [DEC-729](DEC-729.md), [DEC-792](DEC-792.md) |

### Context

Slice D10b is the web-push channel adapter in `mandate-dispatcher`: the `Provider` that reads one
address's vault entry, seals the closed payload to it, signs the VAPID header, posts once, and maps
the answer. The spec and DEC-2500, DEC-2502, and DEC-2503 fix most of what it does. A few things
they leave open: how one call names one address when `AddressHandle` names none, what a few
failures the spec does not list become, and how DEC-2504's interim, web push off, is held.

### Decision

1. **One call, one address.** `AddressHandle` is `(recipient, channel)` (DEC-710 item 4) and names no
   address, while a web-push send is one address's (DEC-2502 item 4). So `WebPushProvider` is
   called through `Addressed`, the provider seen through one `address_ref` that the dispatcher's
   fan-out takes from its own fold of `NotificationAddressChanged`, and `Addressed` is the
   `mandate_notify::Provider`. `mandate-notify`'s handle is unchanged.
2. **The order of one attempt.** A handle on any channel but `web_push` is a wiring fault of the
   deployment: `permanent { provider_error }`, with no vault read and no POST. Then the vault entry
   is read (DEC-2502 item 7: no answer is `retryable { provider_error }`, no entry
   `permanent { address_missing }`); a `key_id` the process does not hold is
   `permanent { provider_error }` (DEC-2503 item 5); an endpoint off the allowlist is
   `permanent { address_rejected }` with no POST (DEC-792 item 4); then the message id is drawn,
   the body sealed, the header signed with this attempt's time through `prepare`, and one POST made.
3. **Failures the spec does not name.** A push service that could not be reached at all (a refused or
   reset connection, a TLS failure) is `retryable { timeout }`, as the relay's `unreachable` is
   (DEC-728, `relay_refusal`). A vault entry whose `p256dh` or `auth` does not parse is
   `permanent { provider_error }`: the workspace API checked both when it stored them, so it is the
   deployment's fault, and it marks nothing (DEC-728's reading). A vault read that spends the whole
   10-second budget is `retryable { provider_error }` with no POST (DEC-2502 item 7). A random source
   or a signer that fails is no outcome at all: the send is not formed (`NotifyError`), nothing is
   posted or journaled, and the next step tries again under the same key.
4. **One budget per send.** The vault read is given the whole `SEND_DEADLINE` (10 seconds, DEC-2500
   item 6) and the POST what the read left, so the two together never exceed it.
5. **The answer is a status.** The POST port returns `PushAnswer { status }` and nothing else, so no
   header, `Location` included, can reach a record (DEC-2500 item 9). The message id is a fresh
   16-octet draw from the provider's secure random source, as 32 lowercase hex digits, drawn before
   the POST so an accepted push always has its id.
6. **The class comes from the text key.** `Notification` carries no class, and spec §3.2's text keys
   partition the classes: `approval_needed` is `action`, `attention_needed` and `account_changed`
   are `safety`, and `brief_ready` is `info`. The envelope is that class's fixed pair (DEC-700
   item 3).
7. **The off switch.** DEC-2504 item 4 and spec §4.6 say that while web push is off the dispatcher
   makes no web-push attempt and journals none. Spec §5.2 gives no outcome for that, and any outcome
   would be journaled as an attempt, so the switch is not in the provider. It is `WebPushSwitch`, the
   process configuration's value (DEC-2500 item 7), `Off` by default; `sends` gives the
   `(recipient, channel)` pairs a step may send on, every `web_push` pair withheld while it is off.
   The switch withholds sends, never a recipient: the process slice (D10c), which wires it, keeps
   each notice's recipients from the whole audience. The direct-route subject is checked when the
   provider is built (DEC-2503 item 6), so a provider with a person's address never exists.

### Owed by other changes

- **The process slice (D10c):** the vault-backed `AddressBook` with its deadline, the `reqwest`
  `PushPost` (no redirects, HTTPS only, a bounded body read, no header read), the system `Clock`,
  and the step's wiring of `WebPushSwitch` to its sends while recipients come from the whole
  audience.
- **The fan-out (E8-14 with the dispatcher):** calling `Addressed` once per active address, the
  records per address once `NoticeAttempted` version 3 merges (DEC-2502 item 5), and NT-6's
  fan-out timing test.

### Rationale

Items 1 and 5 keep the address and the push service's answer out of every type the dispatcher
keeps. Items 2 to 4 give each failure the outcome that retries where a retry can help and marks an
address only where the spec or RFC 8030 names a dead subscription. Item 7 holds DEC-2504's interim
without inventing an outcome the spec does not have.

### Alternatives considered

- **Add `address_ref` to `AddressHandle`.** Rejected for now: every channel would carry a field only
  web push uses, and `mandate-notify` is another crate's change.
- **An "off" outcome from the provider.** Rejected: it would be journaled as an attempt, which spec
  §4.6 forbids while web push is off.
- **`retryable { provider_error }` for an unreachable push service.** Equivalent in effect (both
  retry and mark nothing); `timeout` keeps the direct and relayed routes alike.

<a id="dec-2507"></a>

## DEC-2507: The CLI's `agent status`, `resume`, `stop` and `exit` commands

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176: each item is a reversible engineering reading of the M7 brief's command table and DEC-279 item 10; none adds risk, and an owner exit is never refused) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), under DEC-79 and DEC-176 |
| **Story or claim** | E8-3's CLI follow-up (DEC-279 item 10), claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [M7 brief](../tasks/M7-escalation-v0.md) "The CLI control surface"; DEC-135 (PX-3, PX-4), DEC-136, DEC-158 option (c), DEC-2000 |

### Context

`mandate-cli`'s library has `agent::{status, code, command}` for every owner command in the M7
brief's table, tested over the in-memory journal (DEC-279). Only `deploy`, `pause` and `kill` reach
the binary (E10-16, DEC-2000). `status`, `resume`, `stop` and `exit` have no `clap` command, so
the founder's paper runbook has no `agent stop` (e2-paper-runbook). `acknowledge` waits for a
closed `OwnerAcknowledged` schema: `mandate-journal` catalogues the event but registers no payload,
so a real journal refuses it as `unknown_schema` (DEC-257 item 17).

### Decision

1. **The commands.** They follow the paved path `version confirm` and `agent deploy` set
   (DEC-530 item 4):
   - `mandate agent status <agent> --account <id>` commits nothing. It prints the mode, the
     startup hold, the count of pending approvals, each restriction in force, and then the
     controls in PX-3's order: pause, stop, kill.
   - `mandate agent resume <agent> [--code <code>]` and `mandate agent stop <agent> [--release]
     [--code <code>]`, without `--code`, print the code `agent::code` gives and commit nothing.
     With `--code`, each commits one `OwnerCommandIssued` through `agent::command`; a wrong code
     is refused `step_up_missing` and nothing is committed. A Stop with `--release` prints
     `RELEASE_WARNING` before its code, every time, so the warning the payload records as shown
     was shown.
   - `mandate agent exit <agent> <instrument> [--confirm-bid <bid> --bid-size <n> --floor <p>]
     [--code <code> | --show-code]` commits at once, with or without a code, and is never refused
     (`AGENTS.md` rule 13, DEC-158 option (c)). Without the right code it commits without step-up
     evidence, so the runtime routes the exit without the owner-exit privilege. `--show-code` prints
     the code and commits nothing. The three bid flags are given together or not at all.
   - **Only a malformed bid confirmation is refused, never the exit.** The bid, bid size and floor
     are each read as a positive fixed-point decimal, written as the owner saw it displayed
     (`501.20`, `00.50` and `495.` are well formed), and normalized to `mandate-num`'s canonical
     text (`501.2`, `0.5`, `495`) before the code binds them and the event records them. So the
     same confirmation, however it is typed, shows and commits the same code and payload. What
     cannot be a confirmation is refused `bid_invalid` before anything is committed:
     - text that is not digits with at most one decimal point;
     - a value that is not positive;
     - a price finer than nine places;
     - a floor above the bid.

     The exit without the bid flags is never refused. It commits and waits for the regular
     session (trading spec §5.5: "Without confirmation, equity sells wait for the session"). That
     keeps `AGENTS.md` rule 13: the refusal denies no exit, and paces none. It refuses only a
     confirmation the exit price ladder could not price against, and leaves the safe default of
     rule 3 one command away.
2. **What each prints.** A committed command prints `<command> <agent> committed as event <id> at
   seq <n>`, the form `killed_line` uses, with `owner_exit` naming its instrument as well. A re-run
   of a command already committed and not yet recorded reports the earlier event (DEC-290). A
   refusal prints nothing, and no message names the DSN.
3. **Refused before anything is opened.** The owner's flags are checked before the journal is opened
   or a store created, as `run_pause` does.
4. **The binary's runners are thin.** Each `run_*` opens Postgres and calls `answer` or `status`,
   which take any `ControlJournal`, so the tests drive every printed line over the in-memory
   journal.

### Rationale

The M7 brief gives the step-up for each command, and DEC-158 option (c) says exit is never refused.
The only question left was how the code reaches the owner. Showing it from the same command when
`--code` is missing is how every other gesture works. Exit cannot use that form, because an exit
without a code must still commit, so it takes an explicit `--show-code` instead.

### Alternatives considered

- **A separate `mandate agent code` subcommand.** It would need a nested subcommand ahead of the
  owner flags, and it differs from `version confirm` and `agent deploy`.
- **Exit without `--code` only shows its code.** That adds a step to the one owner command that
  reduces risk, and rule 13 asks for none.

<a id="dec-2508"></a>

## DEC-2508: The email channel's founder mail, its address, and the brief held back (E8-11)

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176: items 1 to 4 are reversible engineering readings that send no more mail than spec §4.4 allows and loosen no rule). Item 5 refuses the brief's mail until the founder and counsel supply the unsubscribe wording (DEC-79, legal text), which stays theirs |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), under DEC-79 and DEC-176 |
| **Story or claim** | E8-11, claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [notifications spec](../../specs/notifications.md) §4.4, §5.7, NT-1, NT-2, NT-4; [DEC-438](DEC-438.md) items 21 to 24, [DEC-700](DEC-700.md) item 4, [DEC-820](DEC-820.md) items 1, 3 and 4 |

### Context

E8-11 is the email channel. Spec §4.4 and DEC-820 fix most of it: the sender's domain, the body's
parts, the founder's footer, and the rule that no mail reaches anyone but the one founder address
the deployment's configuration names. A few things are left open: the sender's local part, what
counts as "one address", how the message is written, and what happens to the brief, whose
`List-Unsubscribe` endpoint does not exist and whose unsubscribe wording is counsel's.

### Decision

1. **The sender** is `no-reply@notify.owlhead.ai`: the fixed no-reply address spec §4.4 asks for,
   on DEC-820 item 1's sending domain. Nothing reads mail sent to it (NT-3).
2. **One plain address.** `FounderAddress::parse` accepts exactly `local@domain`, each side
   non-empty printable ASCII, with no space, `@`, `,`, `;`, `:`, `<`, `>`, `(`, `)`, `[`, `]`, `\`
   or `"`. That refuses a list, a display name, a comment, a quoted local part, and any line break
   that could start a `Cc` or `Bcc` header. So a second recipient, a `Cc` and a `Bcc` cannot be
   expressed through the one value the transport is bound to (rung 1; DEC-820 item 4's four
   refusals). The type has no `Debug` or `Display` (NT-2).
3. **The mail holds no recipient.** `FounderMail` is the subject, the plain text and the HTML.
   Only `FounderMail::wire` names an address, the bound one, as the message's single `To`. The
   header set is exactly `From`, `To`, `Subject`, `MIME-Version` and `Content-Type`
   (`multipart/alternative`, with a fixed boundary). Every line ends in CRLF.
4. **The body.** The subject is the notice's fixed text. The plain text is that text, a blank
   line, `Open Owlhead to see it.`, the link, a blank line, and exactly DEC-820's footer. The HTML
   holds the same four parts, with the link as both `href` and text, and fetches nothing: no
   image, stylesheet, script, frame or form. There is no greeting (NT-1).
5. **The brief is not mailed yet.** `brief_ready` is the only `info` text a push carries. Its
   mail must carry `List-Unsubscribe` pointing at the workspace's one-click endpoint (spec §5.7),
   which no route serves yet, and its unsubscribe wording is counsel's. So `founder_mail` refuses
   it as `mail_not_permitted`, and the channel adapter answers `permanent {
   recipient_not_permitted }`. That reason is not retried, marks no address `unreachable`, and
   raises no `channel_lost` (spec §5.2). The brief stays readable in the app. This is the
   conservative option; the founder may instead choose to mail the brief to their own address with
   wording they supply.

### Rationale

Each item is the narrowest reading of spec §4.4 that the tests can pin. Item 2 puts DEC-820's
refusals into a type that cannot represent the refused cases. Item 5 sends less, not more, and
leaves the legal wording to the founder and counsel.

### Alternatives considered

- **Accept RFC 5322's full address syntax.** That admits display names, groups and comments, and
  each is a way to add a recipient.
- **Mail the brief without `List-Unsubscribe`.** That breaks spec §4.4's rule for `info` mail.

<a id="dec-2510"></a>

## DEC-2510: The email channel adapter and its transports (E8-11 slice S3b)

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176: each item is a reversible engineering reading that sends no more mail than spec §4.4 allows and loosens no rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), under DEC-79 and DEC-176 |
| **Story or claim** | E8-11, claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [notifications spec](../../specs/notifications.md) §4.4, §5.2; [DEC-700](DEC-700.md) item 4, [DEC-820](DEC-820.md) item 4, [DEC-2508](notifications.md#dec-2508) |

### Context

Spec §4.4 asks for an email adapter that sends only to the founder's one address, through a
transport bound to it at construction whose send takes no recipient, and that refuses every other
recipient as `permanent { recipient_not_permitted }` before any connection. Two transports may
exist: the recorded fixture and the founder-only SMTP transport. It leaves open how the adapter
knows the founder, what it does with a handle for another channel, and how a transport's failures
map to §5.2's reasons.

### Decision

1. **The adapter holds the founder's opaque recipient and a bound transport.** `EmailProvider` is
   built from the founder's `Recipient`, the opaque id the dispatcher's records use, and a
   `MailTransport` already bound to the founder's `FounderAddress`. A send's links use the origin
   the send is given. The adapter holds no address and renders the mail itself (DEC-712).
2. **Every send it may not make is refused before the transport, with one reason.** These are
   all answered `permanent { recipient_not_permitted }`, with the transport not called:
   - a handle whose recipient is not the founder's;
   - a handle on any channel but `email`;
   - the brief, whose mail DEC-2508 item 5 holds back.

   That reason is not retried, marks no address `unreachable`, and raises no `channel_lost`
   (§5.2), so the recipient keeps every other channel.
3. **A transport answers a closed failure, and each maps to one outcome.** `MailFailure` holds no
   provider text (§5.2). The failures a later attempt may clear are `retryable`:
   - `Timeout` gives `timeout`;
   - `RateLimited` gives `rate_limited`;
   - `Unavailable` gives `provider_error`.

   The provider's refusals are `permanent`:
   - `AuthFailed` gives `auth_failed`;
   - `AddressRejected` gives `address_rejected`;
   - `TooLarge` gives `too_large`.

   `address_rejected` and `auth_failed` mark the address `unreachable` as §5.2 says. The receipt
   reasons are a later slice's.
4. **The transport takes the mail and the key, never an address.**
   `MailTransport::submit(mail, key)` writes `FounderMail::wire` to its bound address. Its outer
   error is only for a message that could not be formed. `RecordedTransport` keeps each message's
   wire form, sends nothing, and accepts as `recorded-<n>`. A test can script it to answer
   failures first.
5. **The `xtask` mail guard (spec §4.4, rung 2) lands before this adapter's implementation**, in
   its own change. It fails on any `MailTransport` implementor but these two and on a mail-sending
   crate anywhere but the founder-only transport.

6. **The fixture landed before the guard** (noted with [DEC-2511](notifications.md#dec-2511)). Spec §4.4 has
   the `xtask` guard built "in the same change as the adapter". `RecordedTransport`'s stub
   reached main with #1691, before the guard (#1697), and it sends nothing: it only keeps each
   message's wire form for a test. The guard landed before the adapter's implementation, which
   is the code that can submit anything. So no implementation of a transport ever ran without
   the guard. That is the reading DEC-2510 item 5 takes of §4.4.

### Rationale

Item 2 gives one answer for every send the adapter must not make, so no path through it can
reach the transport for another recipient. Item 3 follows §5.2's split between failures that may
clear and refusals that will not.

### Alternatives considered

- **Refusing another channel as `address_missing`.** That raises `channel_lost`, for a handle the
  dispatcher should never have routed here.
- **Passing the address to the transport.** It would let a caller choose the recipient (rung 1).

<a id="dec-2511"></a>

## DEC-2511: The mail guard (E8-11 slice S4)

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176: the check only refuses code, and its configuration can only narrow what may send mail) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), under DEC-79 and DEC-176 |
| **Story or claim** | E8-11, claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [notifications spec](../../specs/notifications.md) §4.4 (rung 2); [DEC-700](DEC-700.md) item 4, [DEC-2510](notifications.md#dec-2510) item 5 |

### Context

Spec §4.4 asks E8-11 for an `xtask` check, run by `cargo xtask ci fast`'s lint, that fails while
the general footer is still `[[EMAIL-FOOTER]]`. Its primary rule fails on any implementor of the
mail transport trait other than the recorded fixture and the founder-only transport. Its second
rule is a deny-list, kept as the check's configuration, of mail-sending crates, which may appear
only as dependencies of the founder-only transport. The spec leaves the check's name, where its
configuration lives, the founder-only transport's crate and type names, and what "appear" means.

### Decision

1. **The check is `cargo xtask mail-guard`, run first in the lint job** beside `live-feature`.
   It is active while `docs/specs/notifications.md` holds `[[EMAIL-FOOTER]]`. The change that
   supplies counsel's footer removes it, as §4.4 says.
2. **Its configuration is `xtask/mail-guard.toml`.** It names the two allowed implementors, each
   by crate and type:
   - `RecordedTransport` in `mandate-dispatcher`;
   - `FounderSmtpTransport` in `mandate-mail-smtp`, the founder-only transport's crate (a later
     slice).

   It also holds the deny-list of mail-sending crates. The guard always reads this file and the
   spec from the `xtask`'s own repository, as the decision-number check reads its script, so a
   tree without them is still guarded rather than skipped.
3. **The implementor rule reads code, not text.** Every `MailTransport for <Type>` in a workspace
   crate's Rust sources must name an allowed type in that type's own crate. The scan skips
   comments and string literals, so a doc comment or a test fixture mentioning the trait never
   trips it. A generic or blanket impl names a type parameter, which no entry allows, so it fails
   too.
4. **"Appear" means anywhere in the build.** A workspace crate other than `mandate-mail-smtp`
   that depends on a denied crate fails. So does a denied crate in `Cargo.lock` that
   `mandate-mail-smtp` does not depend on directly. A mail crate therefore cannot enter the build
   through a third-party dependency either.
5. **It lands before the adapter's implementation** (DEC-2510 item 5). §4.4's "in the same change
   as the adapter" is read as the guard being in place no later than the code it guards.

6. **Names cannot be borrowed** (the #1697 review). The guard also refuses:
   - the trait imported or re-exported under another name (`MailTransport as ...`);
   - an allowed type's name given to anything else, by `type RecordedTransport = ...` or
     `... as RecordedTransport`;
   - an impl whose self type is not a plain path ending in the allowed name. A tuple, a
     reference, a generic, `dyn` or a leading `::` all fail.

   The deny-list adds the mail-builder, SMTP and mail-provider SDK crates the review named.
7. **Where the trait may be named, and where each transport lives** (the #1716 review). Every
   `MailTransport` token stands in one of four places:
   - an `impl MailTransport for <Name>`, in the allowed type's own file (`file` in the
     configuration) and naming it bare;
   - the trait's own definition, in `trait_file`;
   - a bound: `T: MailTransport`, `+ MailTransport`, or `dyn`/`impl MailTransport`;
   - a plain `use` path with no `as`.

   Anywhere else it fails, inside any macro invocation's arguments included, so no macro can be
   handed the trait to build an impl from. In each allowed type's crate, its name is declared
   exactly once, by `struct` at its file's top level (brace depth 0, not in a nested `mod` or a
   function), and never imported into that file. No declaration, a second declaration, an
   `enum`, `union` or `type` of that name, or an `as` that gives it to something else fails. A
   crate absent from the sources owes no declaration. An import of another crate's
   type under that name can then only clash with the definition, and the impl, which names it
   bare in that file, always means the defined type. `mandate-gateway`'s unrelated
   `RecordedTransport` is in another crate and is not affected.
8. **What the guard cannot see.** A crate that reaches an HTTP mail API through a general HTTP
   client (`reqwest`, `ureq`, the dispatcher's own POST port) names no mail crate and implements
   no `MailTransport`, so neither rule sees it. That path is held at rung 1 instead: the
   dispatcher's ports take no recipient. Above that, it is held at rung 4, by review of any new
   HTTP endpoint, as the push and relay posts are reviewed today.
   Nor does it see code it cannot read as tokens: a procedural macro (none in the workspace
   today, and adding one is a new crate and dependency, which review sees), code generated by a
   `build.rs` into `OUT_DIR` and pulled in with `include!`, or a file outside a workspace crate's
   directory. Those remain review's to catch.

9. **Mail-endpoint literals** (the #1716 review). Outside the founder-only transport's crate and
   `xtask`, whose configuration and tests name them, a string literal fails when it holds, in any
   case, one of `[literals] substrings`: the `smtp://` and `smtps://` schemes, `sendmail`, and the
   mail providers' API and SMTP hosts. So does a literal that is wholly `host:port` for a mail
   submission port in `[literals] ports` (25, 465, 587, 2525), which catches
   `TcpStream::connect("relay.example:25")`. A time such as `"12:25:00"` or text such as
   `"port 25"` is not a `host:port` and passes. The lint is cheap and catches only the plain
   spelling: an endpoint built at run time, read from configuration, or split across literals
   passes, and stays item 8's rung 1 and review's to catch.

### Rationale

Each item fails closed. Allowing each implementor only in its own crate stops a second
`RecordedTransport` elsewhere from passing by name. Checking the lock as well as direct
dependencies closes the transitive path.

### Alternatives considered

- **A text search for `MailTransport for`.** It trips on comments and strings, and misses nothing
  a token scan catches.
- **Direct dependencies only.** It misses a mail crate brought in by another crate.

<a id="dec-2512"></a>

## DEC-2512: The CLI's acknowledgment and its request (journal spec §9.28's writers)

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79: a reversible engineering decision. Every rule it adds only refuses an acknowledgment, which only ever lifts a limit, and the executor still judges each copy) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), under DEC-79, with the coordinator's rulings (a) and (b) on claim #1594 (2026-10-19), items 3 and 4 |
| **Story or claim** | E8-3's CLI follow-up (DEC-279 item 10), claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [journal spec](../../specs/journal.md) §9.28; [DEC-2509](DEC-2509.md) |

### Context

DEC-2509 closed `OwnerAcknowledged`, added `OwnerAcknowledgmentRequested`, and left the writers to
check rules 239 to 241 before appending. `mandate agent acknowledge` still writes DEC-279 item 7's
six members, with `risk_clock` in integer seconds, so a real journal refuses every acknowledgment it
commits, and nothing writes a request.

### Decision

1. **`agent request-ack <agent> <event>`** commits one `OwnerAcknowledgmentRequested`, with these members:
   - `agent` and `event`;
   - `requested_by`, the owner the CLI authenticated, never a value the caller types;
   - `independent_required: true`;
   - `submitted_at` in whole seconds and `risk_clock` as §9.2's timestamp;
   - `step_up`.

   It needs the code the CLI prints, bound to the agent, the event and the control stream's head.
   Without the right code it is refused `step_up_missing` and nothing is committed: a request
   carries step-up (rule 239).
2. **`agent acknowledge <agent> <event> [--request <id>]`** commits one `OwnerAcknowledged` with
   §9.28's nine members, `risk_clock` as a timestamp. Its code also binds the request named, or
   none.
   - **With `--request`:** the CLI reads the control stream up to the head the code is bound to and
     refuses, committing nothing:
     - `request_invalid` when no earlier `OwnerAcknowledgmentRequested` has that `event_id`, or it
       names another `agent` or `event` (rule 240);
     - `request_invalid` when an earlier `OwnerAcknowledged` names that request (rule 241);
     - `not_independent` when the request's `requested_by` is the acknowledging owner (rule 239).

     Otherwise it writes `user` as the owner, copies `requested_by` from the request, and sets
     `independent_required: true` and `request` to the request's id.
   - **Without `--request`:** it writes `requested_by` equal to `user`, `independent_required: false`
     and `request: null`.
3. **A plain acknowledgment writes `independent_required: false` whatever the policy** (the
   coordinator's ruling (a)). Nothing folds `PolicyChanged` yet (DEC-257 item 12), so the CLI cannot
   read the workspace's `independent_approval_required`. That adds no risk: the executor judges
   under the stricter of the copy's requirement and the overlay's at processing (§9.28 check 4), so
   under a requiring policy it refuses the copy `not_independent` and lifts nothing. A test pins the
   CLI's payload to `reference/journal`'s `plain_under_overlay` case, which the vectors' oracle
   refuses so.
4. **The local refusals reuse the executor's reasons and commit nothing** (the coordinator's ruling
   (b)): `request_invalid` for rules 240 and 241 and `not_independent` for rule 239, so an owner
   reads the same reason whether the CLI or the executor refused.

### Rationale

The CLI checks what it can see on the control stream, which is rules 239 to 241. Everything else, the trigger, its
supersession, step-up freshness and the overlay, the executor judges from the account stream when
it processes the copy.

**The residual** is item 3's: under a requiring policy, a plain acknowledgment is committed and
then refused by the executor, not refused locally. When a policy fold lands, the CLI should refuse
it locally instead.

### Alternatives considered

- **Refusing every plain acknowledgment until the overlay is folded.** Rejected: the founder's
  one-user workspace requires no independence and could then lift no limit at all.
- **New CLI-only reasons for rules 240 and 241.** Rejected: the executor already refuses the same
  cases as `request_invalid`, and one reason per case is clearer to the owner.

### Follow-ups

- The tests PR (DEC-77), then the implementation and the `clap` wiring of both subcommands.
- Once `PolicyChanged` is folded: refuse a plain acknowledgment under a requiring policy, locally.

### Amends

- [DEC-279](../04-decision-log.md#decisions) item 7, as [DEC-2509](DEC-2509.md) item 1 amended it:
  the CLI's acknowledgment members.

<a id="dec-2513"></a>

## DEC-2513: What the CLI's §9.28 checks read, and in which order they refuse

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79: it corrects DEC-2512's wording to what the CLI does and makes the read fail closed; it loosens no rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane M7), on the coordinator's review of #1822 |
| **Story or claim** | E8-3's CLI follow-up, claim [#1594](https://github.com/kunwarshivam/mandate/issues/1594); [journal spec](../../specs/journal.md) §9.28; [DEC-2512](notifications.md#dec-2512) |

### Context

DEC-2512 item 2 says an acknowledgment naming a request is checked against the control stream "up to
the head the code is bound to". The CLI reads every row the journal holds when the command runs. A
row that lands after that read and before the append is not checked by the CLI. In review, the
first implementation also skipped a row it could not parse, so an acknowledgment it could not read
left its request looking unspent.

### Decision

1. **What is read.** The CLI checks rules 239 to 241 against every control-stream row the journal
   holds when the command runs. A row appended between that read and the CLI's append is judged only
   by the executor (§9.28 check 4), which refuses a second acknowledgment of a request
   `request_invalid`.
2. **Unreadable rows fail closed.** If any row on the control stream does not parse, the
   acknowledgment is refused with a journal error and nothing is committed. A rule never passes on
   a stream the CLI could not read in full.
3. **The order.** Rule 239 is judged before rule 240. The owner's own request is refused
   `not_independent` even when it names another agent or event, as §9.28 check 4 orders
   `not_independent` before `request_invalid` (the vectors' `independence_before_the_request`).

### Amends

- [DEC-2512](notifications.md#dec-2512) item 2: the "up to the head the code is bound to" wording, replaced by
  item 1 here.

<a id="dec-2516"></a>

## DEC-2516: The cell's dispatcher process: its subject streams from the control stream, and email only through the recorded transport

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering readings under DEC-79 and DEC-176. Item 1 fills a gap DEC-2500 item 7 left, the store's list of streams, by the reading that drops no stream the control stream names; items 2 and 3 only narrow what the process may send. Nothing loosens a rule) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | Product-flow blocker X8 part 2; E8-10 slice D10; DEC-2500 items 3, 4, 6 and 7; DEC-2504, DEC-2511; notifications spec §5.1, §5.9 |

### Context

DEC-2500 fixes the dispatcher's process: its own binary, the writer lifecycle, the probes, the
drain, and one configuration file whose subject streams are "a floor, never a filter", with the
process also reading "the journal store's list of its workspace's agent, account, and control
streams". The journal store has no such list: `mandate-journal-pg` offers rows, heads and
ownership, not a listing, and adding one would add SQL and grants to a safety-critical crate. In
the cell the founder's agents are deployed after the chart is installed, so a dispatcher limited
to the file's streams would never see their alerts. And no transport may mail anyone but the
founder, and only the recorded fixture may run outside the founder's own deployment (DEC-2511).

### Decision

1. **The control stream names the subject streams.** At each step the process reads the
   workspace's control stream and follows the following, in its environment:
   - the control stream itself;
   - the agent stream of every `AgentDeployed`'s `agent_id`, whatever its version;
   - the account stream of every `ConnectionRequested`'s and `ConnectionEstablished`'s
     `account_ref`: rule 131 opens the account stream at the request, and its executor may alert
     on it before the connection is established, or if it never is. A `ConnectionEstablished` at
     version 1, from before DEC-800 named the account, names no stream; its connection's
     `ConnectionRequested` does.
   The configured streams stay a floor. A stopped agent stays followed, since its stream may
   still raise an alert. A row that does not read names nothing and drops nothing. A read of the
   control stream that fails is a failed read: the step ends with nothing sent and is no heartbeat
   (DEC-2500 item 3).
2. **The configuration is closed, and refuses what the cell's process cannot honour.** The file's
   members are DEC-2500 item 7's. An unknown member is refused, and so are:
   - `web_push` anything but `false` (DEC-2504's interim), and with it any `push_allowlist` entry
     or a `vapid_subject` other than `null`, which only web push reads and which would put an
     address in the file;
   - a channel other than `email`, which has no adapter yet;
   - more or fewer than one email recipient, since the founder-only transport mails the founder
     alone.
   A refusal names its member, never its value, and stops the start (exit 2).
3. **Email goes only through the recorded transport.** The binary binds `RecordedTransport` to a
   fixture address on the reserved `.invalid` domain. It keeps each message and sends nothing, so
   the cell's dispatcher journals every notice and attempt as a deployment's would, and no mail
   leaves the cell. The founder's SMTP transport is the founder's deployment's, not this one's.
4. **One step, its outcome, and the lifecycle.** Each tick reads the replay (until it reaches the
   head), then the control stream, then runs one step. The step's result maps as DEC-2500 item 4
   says:
   - success is a heartbeat;
   - a failed read, or an append answered `Unavailable`, `Ambiguous` or `HeadMismatch`, is nothing,
     and the next tick tries again;
   - `Fenced` is fenced;
   - every other refusal, an `Invalid` append included, is `Failed`.
   A step runs to its end before the next tick, so a drain settles at once.

5. **In the cell.** The dispatcher is one Deployment per workspace, `processes.json`'s
   `dispatcher`: its own image, its own journal role `mandate_dispatcher`, Postgres its one
   destination, and no guaranteed share. Its configuration file is the ConfigMap
   `dispatcher-config`, mounted read-only, rendered from `api.workspace`, `api.appOrigin`,
   `environment` (paper only: a backtest release fails the render) and the `dispatcher` values,
   whose one recipient goes on the email channel. Its `build` is `dispatcher.build`, which
   `cargo xtask cell up` sets to the binary's own SHA-256, or else the digest the image is pinned
   by, which fixes the binary; an image with neither fails the render. The two are not the same
   digest: a local cell's `build` names its binary, a pinned deployment's its image. Each names
   exactly one build within its own kind of deployment, which is what a notice's actor needs; they
   are not compared across the two.

### Consequences

- An agent deployed after the dispatcher started is followed from its deployment on, so its
  executor's and runtime's alerts (DEC-2514) become notices.
- The local cell runs the dispatcher, so an owner alert recorded on any stream the control stream
  names becomes a journaled notice and attempt; no mail leaves the cell.
- Infrastructure §3.1 gains the dispatcher's row.
- Each tick reads every followed stream from its start. That is correct and fits the cell; reading
  from a kept position is backlog row E8-10y.

<a id="dec-2517"></a>

## DEC-2517: A user's kill switch is alerted by its command's writer; an operator's keeps the executor's alert

| | |
|---|---|
| **Status** | Accepted (the coordinator, 2026-10-26: it removes no alert any switch has today) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code` (coordinator), on lane L1's proposal |
| **Story or claim** | Backlog E8-9k and E8-9l; [notifications spec](../../specs/notifications.md) §3.2 and §3.4; [DEC-2514](DEC-2514.md) item 2 |

### Context

Notifications spec §3.4 makes a user's kill switch one notice, from the `OwnerAlertSent` its
`OwnerCommandIssued` commits beside it. DEC-2514 item 2 had the executor alert every switch it
applied until that alert existed, and said E8-9k removes the executor's alert for a user's **and an
operator's** switch. But §3.2 gives an operator's switch its own cause, `PlatformOperatorAction`,
and nothing writes that alert yet. Removing the executor's alert for it now would leave an
operator's switch with no notice, and removing an owner-facing alert is the founder's call
(DEC-723's reasoning). The E8-9k review also found that a user's switch has two writers, the API and
`mandate agent kill`, and that the connection revoke plans a third (`RevokeEffect::KillSwitch`).

### Decision

1. **Every writer of a user's kill switch commits its alert.** Whatever appends an
   `OwnerCommandIssued` with `command` `kill_switch` appends, in the same batch and right after it,
   the `OwnerAlertSent` (`kind` `kill_switch`) whose `subject`, `causation_id` and
   `owner_command` are the command. Today those writers are the API's `POST` kill switch and the
   CLI's `mandate agent kill`. A writer added later, the connection revoke's included, does the
   same in the same change.
2. **A catalogue test holds item 1.** `mandate-api-server`'s `kill_switch_writers` test lists every
   non-test source file that names both `OwnerCommandIssued` and `kill_switch`, each as a writer,
   with the test that pins its alert, or as a reader. A new file fails the test until it is listed.
3. **The executor alerts every switch but a user's.** It alerts a risk limit's switch and, until
   E8-9l writes the operator's alert from its `PlatformOperatorAction`, the operator's. This
   deviates from DEC-2514 item 2 only in keeping the operator's alert. It removes no alert any
   switch has today.

### Consequences

- One user kill switch is one notice, whichever surface issued it.
- An operator's switch stays one notice for each stream owner that applies it, as before, until
  E8-9l.
- The spec's delivery-records row (notifications spec §11, "no other writer yet") is updated in
  its next spec change.
