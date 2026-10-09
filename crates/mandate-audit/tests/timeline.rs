//! E12-2 slices T1a and T1b: the per-agent timeline's core read (workspace API §4.8.1 "Timeline",
//! AU-1, AU-5, DEC-760, DEC-764, DEC-777): the merge, the filters that still consume, `limit`, the
//! 10,000-event cap, paging under appends, and the one `NotFound`. Each expectation is written from
//! the fixture's own appends, never from the code under test.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNTS, Fixture, T, WS_A, acct, actor, event_id, limit, permitted, tenant, text};
use common::{ctl, workspaces};
use mandate_audit::{AuditError, MAX_CONSUMED_PER_STREAM, MemoryRead, StreamCursor, Timeline};
use mandate_audit::{TimelineQuery, TimelineRead};
use mandate_canon::Digest;
use mandate_journal::StreamId;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

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

impl Journal {
    /// Opens `name`'s agent stream in the workspace segment `ws`.
    fn open_agent(&mut self, ws: &str, name: &str) {
        let opened =
            format!(r#"{{"stream_type":"agent","workspace_id":"{ws}","agent_id":"{name}"}}"#);
        self.fx.open(&format!("agent:{ws}:{name}"), &opened);
    }

    fn read(&self, q: &TimelineQuery<'_>) -> Result<Timeline, AuditError> {
        MemoryRead::new(&self.fx.journal).timeline(&permitted(&tenant(WS_A)), q)
    }
}

/// `WS_A`'s timeline streams for `ME`, in ascending `stream_id`.
fn timeline() -> [String; 3] {
    [stream(Some("ACCT1")), stream(Some("ACCT2")), stream(None)]
}

/// One appended event as the test recorded it: its `recorded_at`, its type, and whether it is the
/// agent's (on its own stream, or an `AgentModeApplied` naming it; never a mark, DEC-764 item 2).
type Seen = (String, &'static str, bool);

/// A [`Journal`] and the test's own record of every event appended to it, which [`expect`] reads.
struct Model {
    j: Journal,
    seen: BTreeMap<String, Seen>,
}

impl Model {
    /// [`Journal::new`], and `ME`'s agent stream in each other workspace of [`workspaces`]: one
    /// whose segment has `WS_A`'s as a prefix, and `WS_B`.
    fn new() -> Self {
        let mut j = Journal::new();
        for ws in &workspaces()[1..] {
            j.open_agent(ws, ME);
        }
        let seen = timeline()
            .map(|s| {
                (
                    j.opened(&s),
                    (T.to_owned(), "StreamOpened", s.starts_with("agent:")),
                )
            })
            .into();
        Self { j, seen }
    }

    /// Appends at `at(s)`, in every workspace alike, an observation to the agent's stream
    /// (`which` 0) or to ACCT1 or ACCT2 (`which` 1 or 2) a mark or an `AgentModeApplied`.
    fn put(&mut self, which: usize, mark: bool, s: u64) {
        for ws in &workspaces() {
            let account = || acct(ws, ACCOUNTS[which - 1]);
            let (to, typed, payload, mine) = match (which, mark) {
                (0, _) => (
                    format!("agent:{ws}:{ME}"),
                    "ObservationRecorded",
                    OBS.into(),
                    true,
                ),
                (_, true) => (account(), "MarkUpdated", MARK.to_owned(), false),
                (_, false) => (account(), "AgentModeApplied", mode(), true),
            };
            let id = self.j.put(&to, &at(s), typed, &payload, 1).remove(0);
            self.seen.insert(id, (at(s), typed, mine));
        }
    }
}

/// Whether `q` serves the event `seen` records: the agent's, and passing `types`, `from`, `to`.
fn matches(q: &TimelineQuery<'_>, (at, typed, mine): &Seen) -> bool {
    let at = UtcNanos::parse(at).unwrap();
    *mine
        && q.types.is_none_or(|t| t.contains(typed))
        && q.from.is_none_or(|f| at >= f)
        && q.to.is_none_or(|t| at < t)
}

/// One page as the test's record gives it now: `(events, next, more, as_of)`.
type Page = (
    Vec<String>,
    Vec<StreamCursor>,
    bool,
    Vec<(String, u64, Digest, String)>,
);

/// The page `q` must give, `n` its `limit`, from the test's record alone. The merge over the raw
/// heads from `q.after` (DEC-777 item 1) is computed as a sort, not a merge: each remaining event's
/// key is the latest `recorded_at` its stream holds from the cursor through that event, then the
/// `stream_id`, then the `seq`. Taking the least head at each step yields exactly that order, clocks
/// that step back included, since no event leaves before an earlier one of its stream. The walk
/// consumes in that order, serves what [`matches`], and stops after the `n`-th served event (item 2)
/// or the 10,000th consumed from one stream (item 3). `next` names all three streams (item 5),
/// `more` is whether any holds an event past it (item 4), and `as_of` is each stream's last event,
/// hashed here from the journal's stored body.
fn expect(m: &Model, q: &TimelineQuery<'_>, n: u64) -> Page {
    let streams = timeline();
    let ids = streams
        .clone()
        .map(|s| m.j.fx.appended[&s].event_ids.clone());
    let start = streams.clone().map(|s| {
        let cursor = q.after.iter().find(|c| c.stream_id == s);
        cursor.map_or(0, |c| c.seq as usize)
    });
    let mut order = Vec::new();
    for (i, list) in ids.iter().enumerate() {
        let mut latest = String::new();
        for (k, id) in list.iter().enumerate().skip(start[i]) {
            latest = latest.max(m.seen[id].0.clone());
            order.push((latest.clone(), i, k));
        }
    }
    order.sort();
    let (mut pos, mut taken, mut events) = (start.map(|p| p as u64), [0; 3], Vec::new());
    for (_, i, k) in order {
        pos[i] = k as u64 + 1;
        taken[i] += 1;
        let id = &ids[i][k];
        events.extend(matches(q, &m.seen[id]).then(|| id.clone()));
        if events.len() as u64 == n || taken[i] == MAX_CONSUMED_PER_STREAM {
            break;
        }
    }
    let more = (0..3).any(|i| pos[i] < ids[i].len() as u64);
    let as_of = streams
        .iter()
        .zip(&ids)
        .map(|(s, list)| {
            let rows = m.j.fx.journal.rows(&StreamId::parse(s).unwrap());
            let last = Digest::of(&rows.last().unwrap().body);
            let at = m.seen[list.last().unwrap()].0.clone();
            (s.clone(), list.len() as u64, last, at)
        })
        .collect();
    (events, cursors(&pos), more, as_of)
}

/// Pages `q` from `after`, at most `bound` pages and while `more`, each checked whole against
/// [`expect`] in the journal's state at its read; the ids served, the last `next`, and `more`.
fn pages(
    m: &Model,
    q: &TimelineQuery<'_>,
    n: u64,
    mut after: Vec<StreamCursor>,
    bound: usize,
) -> Result<(Vec<String>, Vec<StreamCursor>, bool), TestCaseError> {
    let (mut served, mut more) = (Vec::new(), true);
    for _ in 0..bound {
        let q = TimelineQuery {
            after: &after,
            ..q.clone()
        };
        let page = m.j.page(&q);
        let mut as_of: Vec<_> = (page.as_of.iter())
            .map(|w| (w.stream_id.clone(), w.seq, w.hash, w.recorded_at.clone()))
            .collect();
        as_of.sort();
        let got = (ids(&page), page.next.clone(), page.more, as_of);
        prop_assert_eq!(got, expect(m, &q, n), "the page from {:?}", q.after);
        served.extend(ids(&page));
        (after, more) = (page.next, page.more);
        if !more {
            break;
        }
    }
    Ok((served, after, more))
}

/// One random append: the stream (`which` of [`Model::put`]), a mark or not, the clock's step
/// forward, and how far this event's `recorded_at` steps back from the clock.
type Step = (usize, bool, u64, u64);

/// One random case: the appends, the `types` mask with `from` and `to` in seconds, the `limit`,
/// and the append before which the first two pages are read.
type Case = (Vec<Step>, (usize, (Option<u64>, Option<u64>)), u64, usize);

const TYPES: [&str; 4] = [
    "StreamOpened",
    "ObservationRecorded",
    "AgentModeApplied",
    "MarkUpdated",
];

/// AU-5, AU-1, and DEC-777 items 1 to 5: random journals in three workspaces (`WS_A`, one whose
/// segment has `WS_A`'s as a prefix, and `WS_B`, each given the same appends), clocks that often
/// step back by up to 3 s, random `types`, `from`, `to`, and `limit`, and appends between pages.
/// Every page is [`expect`]'s exactly: events, `next`, `more`, and `as_of`. Over the whole walk
/// each stream's matching events come once each in `seq` order, nothing else comes, and the walk
/// ends at every head; a fresh walk serves the one unbounded merge, filtered.
#[test]
fn every_walk_serves_each_matching_event_once_in_merge_order() {
    let event = (0..3_usize, any::<bool>(), 0..2_u64, 0..4_u64);
    let bounds = (
        proptest::option::of(0..34_u64),
        proptest::option::of(0..34_u64),
    );
    let strategy = (
        proptest::collection::vec(event, 1..30),
        (0..16_usize, bounds),
        1..5_u64,
        0..30_usize,
    );
    let config = ProptestConfig {
        cases: 48,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let body = |(events, (mask, (from, to)), n, split): Case| -> Result<(), TestCaseError> {
        let mut m = Model::new();
        let types: Vec<&str> = (TYPES.iter().enumerate())
            .filter(|(i, _)| mask >> i & 1 == 1)
            .map(|(_, t)| *t)
            .collect();
        let time = |s: Option<u64>| s.map(|s| UtcNanos::parse(&at(s)).unwrap());
        let q = TimelineQuery {
            types: (mask != 0).then_some(&types[..]),
            from: time(from),
            to: time(to),
            ..query(&[], n)
        };
        let (mut served, mut after, mut clock) = (Vec::new(), Vec::new(), 3);
        for (i, &(which, mark, step, back)) in events.iter().enumerate() {
            if i == split.min(events.len() - 1) {
                let (early, next, _) = pages(&m, &q, n, after, 2)?;
                (served, after) = (early, next);
            }
            clock += step;
            m.put(which, mark, clock - back);
        }
        let (rest, last, more) = pages(&m, &q, n, after, 200)?;
        served.extend(rest);
        prop_assert!(!more, "the walk ends within 200 pages");
        let heads = timeline().map(|s| m.j.fx.appended[&s].event_ids.len() as u64);
        prop_assert_eq!(last, cursors(&heads), "the last page is at every head");
        let mut matching = 0;
        for s in timeline() {
            let list = &m.j.fx.appended[&s].event_ids;
            let want: Vec<&String> = list.iter().filter(|id| matches(&q, &m.seen[*id])).collect();
            let got: Vec<&String> = served.iter().filter(|id| list.contains(id)).collect();
            prop_assert_eq!(&got, &want, "{} once each, in seq order", s);
            matching += want.len();
        }
        prop_assert_eq!(served.len(), matching, "nothing else is served");
        let (fresh, _, _) = pages(&m, &q, n, Vec::new(), 200)?;
        prop_assert_eq!(fresh, expect(&m, &q, u64::MAX).0, "pages are one merge");
        Ok(())
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// AU-1 and DEC-760 item 4: an unknown agent, one only another workspace holds (`WS_B`, or one whose
/// segment has `WS_A`'s as a prefix), and a cursor naming any stream that is not one of this
/// timeline's, alone or beside a valid one, are the one `NotFound`; a valid cursor is not.
#[test]
fn foreign_and_unknown_ids_are_the_one_not_found() {
    let mut j = Journal::new();
    let [a, trap, b] = workspaces();
    for (ws, name) in [
        (&a, "AG2"),
        (&trap, ME),
        (&trap, "AGP"),
        (&b, ME),
        (&b, "AGB"),
    ] {
        j.open_agent(ws, name);
    }
    for agent_id in ["AG9", "AGP", "AGB", "", "AG1x", "agent:x"] {
        let q = TimelineQuery {
            agent_id,
            ..query(&[], 10)
        };
        assert_eq!(j.read(&q), Err(AuditError::NotFound), "agent {agent_id:?}");
    }
    let own = StreamCursor {
        stream_id: stream(Some("ACCT2")),
        seq: 1,
    };
    for stream_id in [
        ctl(&a),
        format!("agent:{a}:AG2"),
        format!("agent:{trap}:{ME}"),
        acct(&trap, "ACCT1"),
        format!("agent:{b}:{ME}"),
        acct(&b, "ACCT1"),
        acct(&a, "ACCT3"),
        format!("{}x", stream(None)),
        "garbage".to_owned(),
    ] {
        let foreign = StreamCursor { stream_id, seq: 0 };
        for after in [vec![foreign.clone()], vec![own.clone(), foreign.clone()]] {
            let q = query(&after, 10);
            assert_eq!(j.read(&q), Err(AuditError::NotFound), "{after:?}");
        }
    }
    assert!(j.read(&query(&[own], 10)).is_ok());
}

/// One draft of [`Journal::batch`]: its type, `schema_version`, `causation_id`, and payload.
type Draft<'d> = (&'d str, u64, Option<&'d str>, &'d str);

impl Journal {
    /// The id the `k`-th next appended event takes, so a batch can name its own companion.
    fn upcoming(&self, k: u64) -> String {
        event_id(self.next + k)
    }

    /// Appends `drafts` to `to` in one batch at `at(s)`, each with its own `schema_version` and
    /// `causation_id`; returns their ids.
    fn batch(&mut self, to: &str, s: u64, drafts: &[Draft<'_>]) -> Vec<String> {
        let r = format!("sha256:{}", "4".repeat(64));
        let refs = REFS.map(|k| format!(r#""{k}":"{r}""#)).join(",");
        let bodies: Vec<(String, String)> = drafts
            .iter()
            .map(|(typed, version, cause, payload)| {
                self.next += 1;
                let id = event_id(self.next);
                let cause = cause.map_or("null".to_owned(), |c| format!("\"{c}\""));
                let body = format!(
                    r#"{{"envelope_version":1,"environment":"paper","event_id":"{id}","stream_id":"{to}",
                    "event_type":"{typed}","schema_version":{version},"event_time":"{T}","clock_source":"local",
                    "causation_id":{cause},"correlation_id":null,"actor":{},"config_refs":{{{refs}}},
                    "payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
                    actor()
                );
                (id, body)
            })
            .collect();
        let batch: Vec<(String, &[u8])> = (bodies.iter())
            .map(|(id, b)| (id.clone(), b.as_bytes()))
            .collect();
        self.fx.append_batch_at(to, &at(s), &batch);
        bodies.into_iter().map(|(id, _)| id).collect()
    }

    /// One draft appended alone at `at(s)`; its id.
    fn one(&mut self, to: &str, s: u64, draft: Draft<'_>) -> String {
        self.batch(to, s, &[draft]).remove(0)
    }
}

/// An `AgentModeApplied` naming `agent` in its `agent` member.
fn applied(agent: &str) -> String {
    mode().replace(
        &format!(r#""agent":"{ME}""#),
        &format!(r#""agent":"{agent}""#),
    )
}

/// An `IntentReceived` (version 1) for `intent`, naming `agent`.
fn received(intent: &str, agent: &str) -> String {
    format!(
        r#"{{"intent_id":"{intent}","agent_id":"{agent}","instrument_id":"inst","side":"buy","type":"limit","tif":"day","qty":"1","limit_price":"10","purpose":"open"}}"#
    )
}

/// A `GateDecided` (version 1) for `intent`.
fn gated(intent: &str) -> String {
    format!(
        r#"{{"intent_id":"{intent}","verdict":"allow","reason_code":null,"data_profile":"full","quotes_used":[],"marks_used":[],"checks":[]}}"#
    )
}

/// The batch of one order: its `OrderRequestRecorded` naming `agent`, no intent, then its version-2
/// `OrderSubmitted` as `order`, whose `causation_id` names the companion (journal spec rule 45).
fn order(j: &mut Journal, to: &str, s: u64, agent: &str, order: &str) -> Vec<String> {
    let request = format!(
        r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"{T}"}}"#
    );
    let submitted = format!(
        r#"{{"client_order_id":"{order}","attempt":1,"instrument_id":"inst","side":"buy","type":"limit","tif":"day","qty":"1","limit_price":"10","risk_clock":"{T}"}}"#
    );
    let companion = j.upcoming(1);
    let drafts = [
        ("OrderRequestRecorded", 1, None, request.as_str()),
        ("OrderSubmitted", 2, Some(companion.as_str()), &submitted),
    ];
    j.batch(to, s, &drafts)
}

/// A `FillApplied` of `order`.
fn filled(order: &str) -> String {
    format!(
        r#"{{"fill_id":"F-{order}","client_order_id":"{order}","instrument_id":"inst","side":"buy","qty_gross":"1","price":"10","trade_date":"2026-09-21","risk_clock":"{T}","fees":[]}}"#
    )
}

/// The ids served walking from the start, `limit` 1,000; with the cursors at the end.
fn served(j: &Journal) -> (Vec<String>, Vec<StreamCursor>) {
    let page = j.page(&query(&[], 1_000));
    assert!(!page.more, "one page holds this journal");
    (ids(&page), page.next)
}

/// DEC-764 item 2, with DEC-777 item 6's reach: which account-stream events are the agent's. One
/// page walks the whole journal, and the expected list is written here, one verdict per append: the
/// agent's stream, then ACCT2's `IntentReceived` naming the agent, then ACCT1's members in `seq`
/// order. Every lookup reads only this timeline's streams: an `IntentReceived` or an order on
/// ACCT3, which the agent was not deployed on, and an event of `ME`'s stream in another workspace,
/// make nothing the agent's. A causation link counts only into the agent's own stream. Everything
/// else, marks among them, is consumed unserved, so `next` ends at every head.
#[test]
fn account_events_are_the_agents_by_dec_764() {
    let mut j = Journal::new();
    let [a, trap, _] = workspaces();
    let (s1, s3) = (stream(Some("ACCT1")), acct(&a, "ACCT3"));
    let opened = format!(
        r#"{{"stream_type":"account","workspace_id":"{a}","broker":"alpaca","account_ref":"ACCT3"}}"#
    );
    j.fx.open(&s3, &opened);
    j.open_agent(&a, "AG2");
    j.open_agent(&trap, ME);
    let [i1, i2, i3, i4, i5] = [1, 2, 3, 4, 5].map(|n| event_id(900_000 + n));
    let mut observe = |to: &str| j.put(to, &at(1), "ObservationRecorded", OBS, 1).remove(0);
    let own = observe(&stream(None));
    let theirs = observe(&format!("agent:{a}:AG2"));
    let elsewhere = observe(&format!("agent:{trap}:{ME}"));
    j.one(&s3, 1, ("IntentReceived", 1, None, &received(&i3, ME)));
    order(&mut j, &s3, 1, ME, "O3");
    let via_acct2 = j.one(
        &stream(Some("ACCT2")),
        2,
        ("IntentReceived", 1, None, &received(&i2, ME)),
    );
    let mut expected = vec![j.opened(&stream(None)), own.clone(), via_acct2];
    let mode_me = j.upcoming(2);
    let rows: [(&str, Option<&str>, String, bool); 15] = [
        ("MarkUpdated", None, MARK.to_owned(), false),
        ("AgentModeApplied", None, applied(ME), true),
        ("AgentModeApplied", None, applied("*"), true),
        ("AgentModeApplied", None, applied("AG2"), false),
        ("IntentReceived", None, received(&i1, ME), true),
        ("GateDecided", None, gated(&i1), true),
        ("GateDecided", None, gated(&i2), true),
        ("IntentReceived", None, received(&i4, "AG2"), false),
        ("GateDecided", None, gated(&i4), false),
        ("GateDecided", None, gated(&i3), false),
        ("GateDecided", None, gated(&i5), false),
        ("MarkUpdated", Some(&own), MARK.to_owned(), true),
        ("MarkUpdated", Some(&theirs), MARK.to_owned(), false),
        ("MarkUpdated", Some(&elsewhere), MARK.to_owned(), false),
        ("MarkUpdated", Some(&mode_me), MARK.to_owned(), false),
    ];
    for (typed, cause, payload, mine) in &rows {
        let id = j.one(&s1, 3, (typed, 1, *cause, payload));
        expected.extend(mine.then_some(id));
    }
    for (agent, o, mine) in [(ME, "O1", true), ("AG2", "O2", false), (ME, "O3", false)] {
        let placed = if o == "O3" {
            Vec::new()
        } else {
            order(&mut j, &s1, 4, agent, o)
        };
        let fill = j.one(&s1, 4, ("FillApplied", 1, None, &filled(o)));
        expected.extend(placed.into_iter().chain([fill]).filter(|_| mine));
    }
    assert_eq!(served(&j), (expected, j.heads()));
}

/// DEC-777 item 6 and AU-5: membership lookups read the page's own snapshot, whole. ACCT1's
/// `GateDecided` and `FillApplied` come before any record that links them to the agent, so the first
/// page consumes them unserved. The `IntentReceived` and the order that name the agent, appended
/// after that page, never bring them back: the next page serves only the new records. A fresh walk,
/// whose snapshot holds those records, serves both, though their links come later in `seq`.
#[test]
fn membership_lookups_read_only_the_pages_snapshot() {
    let mut j = Journal::new();
    let s1 = stream(Some("ACCT1"));
    let intent = event_id(900_006);
    let gate = j.one(&s1, 2, ("GateDecided", 1, None, &gated(&intent)));
    let fill = j.one(&s1, 2, ("FillApplied", 1, None, &filled("O6")));
    let opened = j.opened(&stream(None));
    assert_eq!(served(&j), (vec![opened.clone()], j.heads()));
    let first = j.heads();
    let linked = j.one(&s1, 3, ("IntentReceived", 1, None, &received(&intent, ME)));
    let placed = order(&mut j, &s1, 3, ME, "O6");
    let later = [vec![linked], placed].concat();
    let page = j.page(&query(&first, 1_000));
    assert_eq!(
        (ids(&page), page.next, page.more),
        (later.clone(), j.heads(), false)
    );
    let fresh = [vec![opened, gate, fill], later].concat();
    assert_eq!(served(&j), (fresh, j.heads()));
}

/// A version-1 `OrderSubmitted` of `order`: §9's members without `risk_clock`. It names no intent
/// and no agent, so only the `client_order_id` rule could make it, or a fill of it, the agent's.
fn submitted_v1(order: &str) -> String {
    format!(
        r#"{{"client_order_id":"{order}","attempt":1,"instrument_id":"inst","side":"buy","type":"limit","tif":"day","qty":"1","limit_price":"10"}}"#
    )
}

/// DEC-778 items 1 to 3, with DEC-777 item 6. O5 is requested for the agent and submitted at
/// version 2 on ACCT2, and filled on ACCT1: its companion, its submission, and the fill are the
/// agent's, since the order lookup reads every timeline stream. O4 is a version-1 `OrderSubmitted`
/// on ACCT1 with no companion and no `causation_id`; O6 is one whose `causation_id` names O5's
/// companion, which names the agent. Neither version-1 record carries an `intent_id` or an agent,
/// and each `causation_id` is null or names an account-stream event, so only the `client_order_id`
/// rule is in play: a version-1 order is no one's, and neither it nor its fill is served.
#[test]
fn order_links_are_version_two_and_read_every_timeline_stream() {
    let mut j = Journal::new();
    let s1 = stream(Some("ACCT1"));
    let placed = order(&mut j, &stream(Some("ACCT2")), 2, ME, "O5");
    let companion = placed[0].clone();
    j.one(&s1, 3, ("OrderSubmitted", 1, None, &submitted_v1("O4")));
    j.one(&s1, 3, ("FillApplied", 1, None, &filled("O4")));
    let caused = Some(companion.as_str());
    j.one(&s1, 3, ("OrderSubmitted", 1, caused, &submitted_v1("O6")));
    j.one(&s1, 3, ("FillApplied", 1, None, &filled("O6")));
    let fill = j.one(&s1, 4, ("FillApplied", 1, None, &filled("O5")));
    let expected = [vec![j.opened(&stream(None))], placed, vec![fill]].concat();
    assert_eq!(served(&j), (expected, j.heads()));
}
