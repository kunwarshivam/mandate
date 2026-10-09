//! E12-2 slice T1a: the per-agent timeline's core read (workspace API §4.8.1 "Timeline", AU-5,
//! DEC-764, DEC-777): the merge, the filters that still consume, `limit`, and the 10,000-event cap.
//! Each expectation is written from the fixture's own appends, never from the code under test.

mod common;

use common::{ACCOUNTS, Fixture, T, WS_A, acct, actor, event_id, limit, permitted, tenant, text};
use mandate_audit::{MAX_CONSUMED_PER_STREAM, MemoryRead, StreamCursor, Timeline};
use mandate_audit::{TimelineQuery, TimelineRead};
use mandate_time::UtcNanos;

const ME: &str = "AG1";
const OBS: &str = r#"{"source":"bars","instrument_id":null,"as_of":"2026-09-21T14:00:00.000000000Z","data_ref":"sha256:7777777777777777777777777777777777777777777777777777777777777777"}"#;
const MARK: &str = r#"{"instrument_id":"inst","price":"1","source":"quote","feed":"iex","risk_clock":"2026-09-21T14:00:00.000000000Z"}"#;
const EARLY: &str = "2026-09-21T13:00:00.000000000Z";
const REFS: [&str; 6] = [
    "fee_config",
    "instrument_snapshot",
    "mandate_version",
    "rule_set",
    "settlement_calendar",
    "trading_calendar",
];

/// An `AgentModeApplied` naming this agent: on an account stream, the agent's (DEC-764 item 2).
fn mode() -> String {
    format!(
        r#"{{"agent":"{ME}","to":"normal","restriction":"none","originated":false,"risk_clock":"{T}"}}"#
    )
}

/// `s` seconds after 15:00, an hour after every stream's `StreamOpened` at [`T`].
fn at(s: u64) -> String {
    format!("2026-09-21T15:00:{s:02}.000000000Z")
}

/// `WS_A`'s account stream `account`, or with `None` the agent's own stream.
fn stream(account: Option<&str>) -> String {
    let a = text(WS_A);
    account.map_or(format!("agent:{a}:{ME}"), |account| acct(&a, account))
}

/// Every workspace's control and [`ACCOUNTS`] streams, and `WS_A`'s agent stream for `ME`.
struct Journal {
    fx: Fixture,
    next: u64,
}

impl Journal {
    fn new() -> Self {
        let mut fx = Fixture::new(0);
        let a = text(WS_A);
        let opened = format!(r#"{{"stream_type":"agent","workspace_id":"{a}","agent_id":"{ME}"}}"#);
        fx.open(&stream(None), &opened);
        Self {
            fx,
            next: 1_000_000,
        }
    }

    /// Appends one batch of `count` events of `typed` at `recorded_at`; returns their ids.
    fn put(&mut self, to: &str, at: &str, typed: &str, payload: &str, count: usize) -> Vec<String> {
        let r = format!("sha256:{}", "4".repeat(64));
        let refs = REFS.map(|k| format!(r#""{k}":"{r}""#)).join(",");
        let arts = if payload.contains("data_ref") {
            format!("\"sha256:{}\"", "7".repeat(64))
        } else {
            String::new()
        };
        let drafts: Vec<(String, String)> = (0..count)
            .map(|_| {
                self.next += 1;
                let id = event_id(self.next);
                let body = format!(
                    r#"{{"envelope_version":1,"environment":"paper","event_id":"{id}","stream_id":"{to}",
                    "event_type":"{typed}","schema_version":1,"event_time":"{T}","clock_source":"local",
                    "causation_id":null,"correlation_id":null,"actor":{},"config_refs":{{{refs}}},
                    "payload":{payload},"artifact_refs":[{arts}],"pii_refs":[]}}"#,
                    actor()
                );
                (id, body)
            })
            .collect();
        let batch: Vec<(String, &[u8])> = drafts
            .iter()
            .map(|(id, d)| (id.clone(), d.as_bytes()))
            .collect();
        self.fx.append_batch_at(to, at, &batch);
        drafts.into_iter().map(|(id, _)| id).collect()
    }

    fn opened(&self, to: &str) -> String {
        self.fx.appended[to].event_ids[0].clone()
    }

    fn page(&self, q: &TimelineQuery<'_>) -> Timeline {
        let read = MemoryRead::new(&self.fx.journal);
        read.timeline(&permitted(&tenant(WS_A)), q).unwrap()
    }

    /// Every timeline stream at its head (DEC-777 item 5).
    fn heads(&self) -> Vec<StreamCursor> {
        let len = |s: String| self.fx.appended[&s].event_ids.len() as u64;
        cursors(&[
            len(stream(Some("ACCT1"))),
            len(stream(Some("ACCT2"))),
            len(stream(None)),
        ])
    }
}

/// The ACCT1, ACCT2, and agent cursors at `seqs`: every timeline stream, ascending `stream_id`.
fn cursors(seqs: &[u64; 3]) -> Vec<StreamCursor> {
    [stream(Some("ACCT1")), stream(Some("ACCT2")), stream(None)]
        .into_iter()
        .zip(seqs)
        .map(|(stream_id, seq)| StreamCursor {
            stream_id,
            seq: *seq,
        })
        .collect()
}

fn query(after: &[StreamCursor], n: u64) -> TimelineQuery<'_> {
    TimelineQuery {
        agent_id: ME,
        account_refs: &ACCOUNTS,
        after,
        types: None,
        from: None,
        to: None,
        limit: limit(n),
    }
}

fn ids(page: &Timeline) -> Vec<String> {
    page.events.iter().map(|e| e.event_id.clone()).collect()
}

/// Pages from `q.after` until `more` is false, at most `bound` pages; the ids served, in order.
fn walk(j: &Journal, q: &TimelineQuery<'_>, bound: usize) -> Vec<String> {
    let (mut after, mut served) = (q.after.to_vec(), Vec::new());
    for _ in 0..bound {
        let page = j.page(&TimelineQuery {
            after: &after,
            ..q.clone()
        });
        served.extend(ids(&page));
        if !page.more {
            return served;
        }
        after = page.next;
    }
    panic!("the walk ends within {bound} pages");
}

/// DEC-764 item 3 and DEC-777 item 1: the agent's clock steps back (15:00:05, then 15:00:01). The
/// merge over the raw heads serves ACCT1's 15:00:03 first, breaks the 15:00:05 tie for ACCT2's
/// lesser `stream_id`, and keeps the agent's stream in `seq` order. Under `to` = 15:00:05 the
/// excluded heads still take their turn, so the served order is the same order, filtered; a merge
/// over filtered heads would serve the agent's 15:00:01 before ACCT1's 15:00:03.
#[test]
#[ignore = "pending E12-2"]
fn a_clock_step_back_keeps_each_stream_in_seq_order() {
    let mut j = Journal::new();
    let (agent, mode) = (stream(None), mode());
    let first = j.put(&agent, &at(5), "ObservationRecorded", OBS, 1);
    let second = j.put(&agent, &at(1), "ObservationRecorded", OBS, 1);
    let theirs = j.put(&stream(Some("ACCT1")), &at(3), "AgentModeApplied", &mode, 1);
    let tied = j.put(&stream(Some("ACCT2")), &at(5), "AgentModeApplied", &mode, 1);
    let opened = j.opened(&agent);
    let all = [&opened, &theirs[0], &tied[0], &first[0], &second[0]].map(String::clone);
    let early = [&opened, &theirs[0], &second[0]].map(String::clone);
    let to = Some(UtcNanos::parse(&at(5)).unwrap());
    for n in [1, 2, 100] {
        assert_eq!(walk(&j, &query(&[], n), 10), all, "limit {n}");
        let q = TimelineQuery {
            to,
            ..query(&[], n)
        };
        assert_eq!(walk(&j, &q, 10), early, "to 15:00:05, limit {n}");
    }
}

/// AU-5 and DEC-764 item 4: events every filter excludes are consumed, so a page that serves
/// nothing still reaches every head, `more` false (DEC-777 item 4), and `next` names every stream
/// in ascending `stream_id` (DEC-777 item 5).
#[test]
#[ignore = "pending E12-2"]
fn excluded_events_are_still_consumed() {
    let mut j = Journal::new();
    for s in 1..4 {
        j.put(&stream(None), &at(s), "ObservationRecorded", OBS, 1);
        j.put(
            &stream(Some("ACCT1")),
            &at(s),
            "AgentModeApplied",
            &mode(),
            1,
        );
    }
    let from = Some(UtcNanos::parse(&at(50)).unwrap());
    let types: &[&str] = &["IntentReceived"];
    let by_type = TimelineQuery {
        types: Some(types),
        ..query(&[], 1)
    };
    for q in [
        by_type,
        TimelineQuery {
            from,
            ..query(&[], 1)
        },
    ] {
        let page = j.page(&q);
        let nothing: Vec<String> = Vec::new();
        assert_eq!(
            (ids(&page), page.next, page.more),
            (nothing, j.heads(), false)
        );
    }
}

/// DEC-777 items 2 and 4: a page serves exactly `limit` events while more match and stops right
/// after the `limit`-th, consuming none of ACCT1's marks behind it, so `next` is just past it; the
/// next page resumes there. Merge: the three `StreamOpened` at 14:00 (ACCT1, ACCT2, agent), then
/// each second ACCT1's mark (not the agent's) before the agent's observation.
#[test]
#[ignore = "pending E12-2"]
fn a_page_holds_exactly_limit_events_while_more_match() {
    let mut j = Journal::new();
    let mut mine = vec![j.opened(&stream(None))];
    for s in 1..6 {
        j.put(&stream(Some("ACCT1")), &at(s), "MarkUpdated", MARK, 1);
        mine.extend(j.put(&stream(None), &at(s), "ObservationRecorded", OBS, 1));
    }
    let first = j.page(&query(&[], 4));
    let expected = (mine[..4].to_vec(), cursors(&[4, 1, 4]), true);
    assert_eq!((ids(&first), first.next.clone(), first.more), expected);
    let rest = j.page(&query(&first.next, 4));
    let expected = (mine[4..].to_vec(), cursors(&[6, 1, 6]), false);
    assert_eq!((ids(&rest), rest.next, rest.more), expected);
}

/// DEC-764 item 4 and DEC-777 items 3 and 5: ACCT1's marks, recorded at 13:00 behind its
/// `StreamOpened`, take the merge's first 10,000 turns. The page stops there for every stream,
/// serving nothing, `more` true, ACCT2 and the agent's stream named at 0. The next page resumes
/// the same merge: ACCT1's last mark and the agent's event behind it, then the `StreamOpened`s.
#[test]
#[ignore = "pending E12-2"]
fn one_page_consumes_at_most_ten_thousand_events_per_stream() {
    let mut j = Journal::new();
    let acct1 = stream(Some("ACCT1"));
    for _ in 0..10 {
        j.put(&acct1, EARLY, "MarkUpdated", MARK, 1_000);
    }
    let mine = j.put(&acct1, EARLY, "AgentModeApplied", &mode(), 1);
    let first = j.page(&query(&[], 1_000));
    let capped = cursors(&[MAX_CONSUMED_PER_STREAM, 0, 0]);
    let nothing: Vec<String> = Vec::new();
    assert_eq!(
        (ids(&first), first.next.clone(), first.more),
        (nothing, capped, true)
    );
    let rest = j.page(&query(&first.next, 1_000));
    let expected = (
        vec![mine[0].clone(), j.opened(&stream(None))],
        j.heads(),
        false,
    );
    assert_eq!((ids(&rest), rest.next, rest.more), expected);
}
