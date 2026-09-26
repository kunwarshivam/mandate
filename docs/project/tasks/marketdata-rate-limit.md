# Task: mandate-marketdata proactive rate limiting (E2-1 follow-up)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Claim
[#115](https://github.com/kunwarshivam/mandate/issues/115), coordinator `cursor`.

## Story

- **Story:** a follow-up to E2-1's client, found in the real-data rehearsal on #96's branch. The
  client's rate limiting was purely reactive: it sent requests until Alpaca answered 429, then
  backed off 1, 2, 4, 8, 16, and 32 s. The six attempts span 63 s against a 60 s window, so a window
  that slips fails the run as `exhausted`.
- **Acceptance criteria:** the client paces itself from the `X-Ratelimit-*` headers; a 429 without
  headers waits for the next window rather than a short ladder; the retry budget spans more than
  one window; a client-side token bucket at the configured limit is the floor when headers are
  absent or malformed; tests use an injected clock and never sleep; a live download shows no 429
  exhaustion.
- **PRD / HLD / spec anchors:** PRD FR-4.5 (backtests from a data snapshot, which needs the
  download to finish); HLD §9 "Market data service"; ADR-0001 ES-05 (time arrives as an input),
  ES-19 (injected transport, no network in CI).
- **Decisions that apply:** DEC-88, DEC-89 (unchanged). None taken here; no DEC is needed.

## What the host does

Alpaca answers every success with `X-Ratelimit-Limit: 200`, `X-Ratelimit-Remaining`, and
`X-Ratelimit-Reset` in Unix seconds, and a 429 with none of them. Probing the paper key's data
host showed the limiter is a token bucket, not a fixed window: with 199 left the reset is the
current second, and it moves about 0.3 s later per request spent, so the reset is when the bucket
is full again. The design below works for either shape, and the tests model both.

## Design

`crates/mandate-marketdata/src/rate.rs` (private) holds the pacer; `client.rs` shares one pacer per
`Client` behind a mutex and consults it before every attempt, including retries.

1. **Header budget.** Each response's headers, when all three parse and `remaining <= limit`,
   record a budget: requests left and the reset instant. While the reset is in the future, a
   request is admitted if more than `low_water` (5) remain, and the count goes down by one; at the
   low-water mark the next request waits until the reset plus jitter. The margin covers other
   clients of the same account and clock skew.
2. **In flight.** Requests that were admitted and have not come back are counted. A response's
   count is reduced by the others still in flight, which the host may not have counted yet, so
   concurrent requests through one client never overshoot. For the same reset the lower count
   wins (a stale response never raises it, another client's spending lowers it); an earlier reset
   is an older response and is ignored; a later reset is a new budget.
3. **Bounds.** A reset at or before now is ignored. A reset more than one window away is taken as
   one window away, so one wait never exceeds a window. Jitter is the sub-second part of now
   scaled to `max_jitter` (500 ms): no random-number dependency, and tests pick the fraction.
4. **Bucket floor.** Without a usable budget a token bucket admits a burst of a tenth of the limit
   (20) and refills the rest (180) per window. `burst + refill = limit`, so no 60 s span, sliding or
   fixed, admits more than the limit. A limit below 2 counts as 2. All arithmetic is integer
   nanoseconds; there is no floating point.
5. **429.** A 429 zeroes the budget until the last known reset if it is still ahead, else for a
   full window from now, so every request through the client waits. A request gives up with
   `Exhausted` only after `rate_limit_windows` (3) such waits, so its attempts span more than two
   windows. Server errors and transport failures keep the exponential ladder under their own
   `max_attempts` budget; the two budgets are counted separately.
6. **Clock.** `Pause` gains `now()`. `TokioPause` reads the system clock (the one
   `SystemTime::now` in the crate, allowed with a reason); tests use `RecordingPause`, a clock
   that moves only when paused, so nothing sleeps.

`Response` gains `rate: RateHeaders` (raw header strings), filled by `http::rate_headers` from
the reqwest response; `RetryPolicy` gains `rate_limit_windows`; `Client::with_rate` sets a
`RatePolicy`.

## Tests

In `crates/mandate-marketdata/tests/rate_limit.rs`, unless named. The fake host (`Host`) keeps its
own request log with arrival times from the shared fake clock, and each oracle recounts from that
log: per fixed window, per sliding 60 s span, or requests per minute.

| Clause | Test |
|---|---|
| Header-driven pacing: a fixed-window host with accurate headers, 1,000 requests, no 429; every full window carries exactly `limit - low_water` (195) requests and each wait ends at a reset | `header_budget_is_spent_to_the_low_water_mark_then_waits_for_the_reset` |
| Against a token-bucket host like Alpaca's, 1,200 requests, no 429, at least 195 a minute, every wait at most a window plus jitter | `against_alpacas_bucket_the_client_keeps_near_the_limit_without_a_429` |
| Without headers the bucket alone never trips a 200-a-minute fixed window, and no sliding minute holds more than 200 | `without_headers_the_bucket_alone_never_trips_a_fixed_window` |
| Malformed or missing headers (each of the three absent, not a number, a negative remaining, remaining above the limit) fall back to the bucket: a burst of 20, then 180 a minute | `malformed_or_missing_headers_fall_back_to_the_bucket` |
| A 429 without headers waits a full window | `a_429_without_headers_waits_a_full_window` |
| A 429 waits until the last known reset; at the reset itself, a full window | `a_429_waits_until_the_last_known_reset` |
| Rate-limit retries span more than two windows before `Exhausted`; three slips still succeed; a budget of zero windows gives up at once | `rate_limit_retries_span_several_windows_before_giving_up` |
| 429s and server errors count against separate budgets | `a_429_and_server_errors_count_against_separate_budgets` |
| A reset in the past, or equal to now, is ignored | `a_reset_in_the_past_or_now_is_ignored` |
| A full budget (remaining equal to the limit) is spent down to the low-water mark | `a_full_budget_is_spent_down_to_the_low_water_mark` |
| Remaining is never spent below zero (low-water 0) | `remaining_is_never_spent_below_zero` |
| A stale response never raises the count; a lower count for the same reset is taken; an earlier reset is ignored; a later reset is a new budget | `a_stale_response_never_raises_the_remaining_count` |
| A wait is bounded by one window and jittered by the sub-second part of now | `a_wait_for_the_reset_is_bounded_by_a_window_and_jittered` |
| A limit below 2 paces as 2; a partly refilled bucket waits only for the rest of a request | `a_limit_below_two_paces_as_two`, `a_partly_refilled_bucket_waits_only_for_the_rest_of_a_request` |
| Concurrency: eight fetches through one shared client against a host that yields before replying (so requests are in flight together), no 429 and no window over the limit | `concurrent_requests_through_one_client_share_the_budget` |
| The system clock reads as UTC | `the_system_clock_is_read_as_utc` |
| The three headers are read whatever their case; a non-ASCII value is `None` | `rate_limit_headers_are_read_whatever_their_case` (`tests/http.rs`) |
| The existing ladder tests, updated: a 429 now waits 60 s, server errors keep 1, 2, 4 s | `tests/client.rs`, `tests/quotes.rs` |

The tests were committed first, against a pacer stub that admitted everything: 14 of the 16 `rate_limit.rs`
tests of that commit failed on it, and the two ladder tests failed. The two that passed (a past
reset, the system clock) are guards that hold with no pacing.

## Planted bugs

Each bug was planted in `src/rate.rs` or `src/client.rs`, the tests run with
`cargo test -p mandate-marketdata --no-fail-fast --test rate_limit --test client --test quotes`,
and the change reverted.

| Planted bug | Caught by |
|---|---|
| P1: in-flight requests not subtracted from a response's count | `concurrent_requests_through_one_client_share_the_budget` |
| P2: no decrement when a request is admitted | full budget, never below zero, stale response |
| P3: bucket refills the whole limit on top of its burst (220 in a window) | fixed-window floor, malformed headers, limit below 2, partly refilled |
| P4: bucket burst of `limit - 1` with the refill still `limit - burst` | malformed headers (the burst is the tenth the floor test pins) |
| P5: a 429 ignores the window and backs off on the old ladder | 429 without headers, 429 until the known reset, several windows, the ladder tests in `client.rs` and `quotes.rs` |
| P6: no jitter | bounded and jittered |
| P7: a reset more than a window away not clamped (waits it out in 60 s steps) | bounded and jittered |
| P8: the same reset takes the newest count instead of the lower | stale response |
| P9: a retry budget of one window | several windows |

## Mutation testing

`mandate-marketdata` is not safety-critical, so CI's diff gate skips it; this run was made by hand
with cargo-mutants 27.1.0 and nextest (`cargo mutants --in-diff <diff of src/> -p
mandate-marketdata --test-tool nextest --jobs 2`).

- First run: 81 mutants, 4 missed. Two were equivalent (a 429 over budget reaching a shared
  exhaustion check, and `>` against `>=` in a match guard that an earlier arm made unreachable);
  the code was restructured so neither exists. Two were gaps, closed by new cases: a lower count for
  the same reset, and a partly refilled bucket.
- Final run: 67 mutants, 45 caught, 14 timeouts, 8 unviable, 0 missed. Every timeout is a mutant
  that makes the pacer answer a zero wait forever (`Some(Default::default())`, `<` to `<=` at the
  reset, the bucket never refilling, or `nanos` returning a constant): the request loop never
  ends, which the test harness reports as a timeout, so each is caught. The real code never
  returns a zero wait: a wait for a reset is at least a nanosecond, and a bucket wait rounds up.

## Live validation

The same download before and after, through a harness that wraps `AlpacaDataHttp` to log each
request's time and status and calls `download::download` as `mandate download` does: the ten
stock and ETF symbols of `config/research-basket.toml`, 1-minute SIP bars, 2026-03-01 to
2026-07-31 (153 days, 1,530 day pages plus 10 corporate-action pages), paper keys, market-data
endpoints only, one run at a time.

| | `main` (53a562b) | this branch |
|---|---|---|
| Requests sent | 1,916 | 1,540 |
| HTTP 429 | 376 (20 percent) | 0 |
| Exhausted | no | no |
| Elapsed | 402 s | 453 s |
| Successes a minute, whole run | 230 | 204 |
| Successes per wall-clock minute | 399, 198, 201, 201, 198, 202, 141 | 204, 208, 229, 224, 214, 206, 217, 38 |

Both are held to the host's refill of 200 a minute. `main` spends the full bucket in the first
minute and then takes each refilled token by retrying into it, one request in five refused. This
branch spends the budget to the low-water mark, waits for the reset, and sends nothing the host
refuses; it gives up the first minute's head start because it waits for a full bucket rather than
racing the refill. A shorter run on `main` (six weeks, 430 pages) sent 493 requests for them, 63
refused. The rehearsal's 115 to 165 a minute was not reproduced on this account; the old client's
exposure is the 63 s ladder, which the three-window budget removes.

## Scope

- **Reference cases:** none (no market-data reference cases exist).
- **Crates in scope:** `mandate-marketdata`, the request and retry path only (`client.rs`,
  `rate.rs`, and `http.rs` reading the headers); `mandate-cli`'s download test gains the new
  `Response` field and a clock on its fake pause.
- **New dependencies allowed:** none.
- **Safety-critical:** no (market data), so this ships as one PR, tests first.

## Not done here

- Pacing across processes: two `mandate download` runs on one account each keep their own pacer,
  and only the low-water margin and the 429 wait protect them.
- Continuous pacing against the bucket's refill (about 0.3 s a request once the budget is low)
  would keep the first minute's head start on a token-bucket host but would trip a fixed-window
  host, so the client waits for the reset as the headers say.
- The host's `Date` header could correct local clock skew against the reset; the low-water margin
  and the 429 wait cover skew today.
