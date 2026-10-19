//! DEC-2249 items 1 and 2: the cell's source folds the agent stream's own research records as the
//! core judged them. Lineages replay the recorded decisions, each admitted thesis is its
//! instrument's one entry, invalidations mark entries, and `expire_theses` at the risk clock leaves
//! the universe and the active theses. The hand cases are worked out from §8.6; the property's
//! accumulator rebuilds the same state from the generated events alone, never from the rows or the
//! crate's own functions.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_canon::Digest;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::source::{FoldError, ResearchFold, fold_research};
use mandate_journal::StoredEvent;
use mandate_research::{AssetId, Lineage, LineageId, LineageState, WorkingUniverse};
use mandate_research_run::Symbol;
use mandate_runtime::{ActiveThesis, RiskClock};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
/// An instrument the bundle names no symbol for.
const NVDA: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
/// 2026-10-16T15:00:00Z: when the hand cases' theses are recorded, and the clock they fold at.
const T: i64 = 1_792_162_800;

fn bundle() -> Bundle {
    Bundle::load(Path::new(FIXTURE)).unwrap()
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// `secs` as the journal writes a timestamp.
fn stamp(secs: i64) -> String {
    at(secs).to_string()
}

/// The agent stream a test lays down, in order.
#[derive(Default)]
struct Stream(Vec<StoredEvent>);

impl Stream {
    fn push(&mut self, event_type: &str, payload: &str) {
        let seq = u64::try_from(self.0.len()).unwrap() + 1;
        let event_id = format!("01J8Z3MT{seq:018}");
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"agent:ws1:agent-a","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T15:00:00.000000000Z","clock_source":"risk","causation_id":null,"correlation_id":"01RUN","actor":{{"kind":"agent","id":"agent-a","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
        );
        self.0.push(StoredEvent {
            stream_id: "agent:ws1:agent-a".into(),
            seq,
            event_id,
            event_type: event_type.into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: "2026-10-16T15:00:00.000000000Z".into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        });
    }

    /// A thesis record (journal spec §9.4) with every member the schema names. `reason` is JSON:
    /// `null` or a quoted code.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument a folded member, so each case reads as the record it journals"
    )]
    fn thesis(
        &mut self,
        thesis: &str,
        lineage: &str,
        revision: u32,
        instrument: &str,
        admitted: bool,
        reason: &str,
        expires_at: &str,
    ) {
        let (kind, predecessor) = if revision == 0 {
            ("ThesisProposed", "null".to_owned())
        } else {
            ("ThesisRevised", format!(r#""{lineage}""#))
        };
        let payload = format!(
            r#"{{"model_id":"llm.research_agent","model_version":"0.1.0","content_hash":"sha256:{hash}","thesis_id":"{thesis}","lineage_id":"{lineage}","revision":{revision},"predecessor_thesis_id":{predecessor},"autopsy_ref":null,"instrument_id":"{instrument}","asset_class":"us_equity","direction":"long","as_of":"2026-10-16T14:00:00.000000000Z","expires_at":"{expires_at}","horizon_s":3600,"conviction":"0.6","confidence":"0.7","evidence_ref":null,"evidence_sources":["src.newswire"],"corroboration":"independent_source","invalidation":"a guidance cut","allowlist_version":3,"prompt_ref":"sha256:{hash}","response_ref":"sha256:{hash}","admitted":{admitted},"reason":{reason}}}"#,
            hash = Digest::of(b"x").to_hex(),
        );
        self.push(kind, &payload);
    }

    fn admitted(
        &mut self,
        thesis: &str,
        lineage: &str,
        revision: u32,
        instrument: &str,
        expires: i64,
    ) {
        self.thesis(
            thesis,
            lineage,
            revision,
            instrument,
            true,
            "null",
            &stamp(expires),
        );
    }

    fn refused(
        &mut self,
        thesis: &str,
        lineage: &str,
        revision: u32,
        instrument: &str,
        code: &str,
    ) {
        let reason = format!(r#""{code}""#);
        self.thesis(
            thesis,
            lineage,
            revision,
            instrument,
            false,
            &reason,
            &stamp(T + 3_600),
        );
    }

    /// Th-1's admitted `ThesisProposed` on AAPL, with each member in `changed` set to its JSON value,
    /// or left out when the value is `None`.
    fn edited(&mut self, changed: &[(&str, Option<&str>)]) {
        let expires = format!(r#""{}""#, stamp(T + 60));
        let members = [
            ("thesis_id", r#""th-1""#),
            ("lineage_id", r#""th-1""#),
            ("revision", "0"),
            ("predecessor_thesis_id", "null"),
            ("instrument_id", &format!(r#""{AAPL}""#)),
            ("expires_at", &expires),
            ("admitted", "true"),
            ("reason", "null"),
        ];
        let payload = members
            .iter()
            .filter_map(|&(member, value)| {
                let value = changed
                    .iter()
                    .find(|(name, _)| *name == member)
                    .map_or(Some(value), |(_, changed)| *changed)?;
                Some(format!(r#""{member}":{value}"#))
            })
            .collect::<Vec<_>>()
            .join(",");
        self.push("ThesisProposed", &format!("{{{payload}}}"));
    }

    /// A model's `ThesisInvalidated` of `thesis` (journal spec §9.22).
    fn invalidated(&mut self, thesis: &str, lineage: &str, instrument: &str) {
        self.push(
            "ThesisInvalidated",
            &format!(
                r#"{{"thesis_id":"{thesis}","lineage_id":"{lineage}","instrument_id":"{instrument}","cause":"model","candidate":0,"evidence":["01OBS"],"source":"src.newswire"}}"#
            ),
        );
    }

    fn fold(&self, now: i64) -> Result<ResearchFold, FoldError> {
        fold_research(&self.0, at(now), &bundle().plane.instruments)
    }
}

fn asset(id: &str) -> AssetId {
    AssetId::new(id).unwrap()
}

fn lineage_id(id: &str) -> LineageId {
    LineageId::new(id).unwrap()
}

fn universe(ids: &[&str]) -> WorkingUniverse {
    WorkingUniverse::Known {
        instruments: ids.iter().map(|id| asset(id)).collect(),
        pinned: false,
    }
}

fn active(
    thesis: &str,
    lineage: &str,
    revision: u32,
    instrument: &str,
    symbol: &str,
    expires: i64,
) -> ActiveThesis {
    ActiveThesis {
        thesis_id: thesis.to_owned(),
        lineage_id: lineage.to_owned(),
        revision,
        instrument: asset(instrument),
        symbol: Symbol::new(symbol).unwrap(),
        expires_at: RiskClock::from_secs(expires),
    }
}

fn lineage(revisions: u32, admitted: u32, retired: bool) -> Lineage {
    Lineage {
        revisions,
        admitted,
        retired,
    }
}

/// An admitted proposal and its admitted revision hold the instrument, the revision being its
/// active thesis, until the revision's horizon; at it the instrument is removed (§8.6, DEC-118),
/// and the lineage is unchanged. An empty stream folds to nothing at all.
#[test]
fn a_proposal_and_its_revision_hold_the_instrument_until_the_revision_s_horizon() {
    let empty = Stream::default().fold(T).unwrap();
    assert_eq!(empty.universe, universe(&[]));
    assert_eq!(empty.lineages, LineageState::default());
    let mut stream = Stream::default();
    stream.admitted("th-1", "th-1", 0, AAPL, T + 3_600);
    stream.admitted("th-2", "th-1", 1, AAPL, T + 7_200);
    let fold = stream.fold(T + 3_600).unwrap();
    assert_eq!(fold.universe, universe(&[AAPL]));
    assert_eq!(
        fold.active,
        [active("th-2", "th-1", 1, AAPL, "AAPL", T + 7_200)]
    );
    assert_eq!(
        fold.lineages.lineage(&lineage_id("th-1")),
        Some(&lineage(1, 2, false))
    );
    assert_eq!(
        fold.lineages.holder_of(&lineage_id("th-1")),
        Some(&asset(AAPL))
    );
    let later = stream.fold(T + 7_200).unwrap();
    assert_eq!(later.universe, universe(&[]), "at its horizon, removed");
    assert!(later.active.is_empty());
    assert_eq!(later.lineages, fold.lineages, "expiry changes no lineage");
}

/// A `lineage_retired` refusal retires the lineage, which removes the instrument it holds; an
/// invalidation removes its thesis's instrument. Each lineage keeps what it admitted.
#[test]
fn a_retiring_refusal_and_an_invalidation_each_remove_their_instrument() {
    let mut stream = Stream::default();
    stream.admitted("th-1", "th-1", 0, AAPL, T + 3_600);
    stream.admitted("th-3", "th-3", 0, MSFT, T + 3_600);
    stream.refused("th-2", "th-1", 4, AAPL, "lineage_retired");
    stream.invalidated("th-3", "th-3", MSFT);
    let fold = stream.fold(T).unwrap();
    assert_eq!(fold.universe, universe(&[]));
    assert!(fold.active.is_empty());
    assert_eq!(
        fold.lineages.lineage(&lineage_id("th-1")),
        Some(&lineage(0, 1, true))
    );
    assert_eq!(
        fold.lineages.lineage(&lineage_id("th-3")),
        Some(&lineage(0, 1, false))
    );
}

/// Another lineage's admission takes the instrument: its thesis is the active one, the first
/// lineage loses its holder, and an invalidation of the first lineage's thesis, which no longer
/// holds the instrument, removes nothing.
#[test]
fn another_lineage_s_admission_takes_the_instrument_and_a_stale_invalidation_removes_nothing() {
    let mut stream = Stream::default();
    stream.admitted("th-1", "th-1", 0, AAPL, T + 3_600);
    stream.admitted("th-5", "th-5", 0, AAPL, T + 5_400);
    stream.invalidated("th-1", "th-1", AAPL);
    let fold = stream.fold(T).unwrap();
    assert_eq!(fold.universe, universe(&[AAPL]));
    assert_eq!(
        fold.active,
        [active("th-5", "th-5", 0, AAPL, "AAPL", T + 5_400)]
    );
    assert_eq!(fold.lineages.holder_of(&lineage_id("th-1")), None);
    assert_eq!(
        fold.lineages.held_by(&asset(AAPL)),
        Some(&lineage_id("th-5"))
    );
}

/// A refusal for any other reason admits nothing, though its lineage becomes known (MC-N19), and
/// active theses are ordered by instrument.
#[test]
fn another_refusal_admits_nothing_and_the_active_theses_are_in_instrument_order() {
    let mut stream = Stream::default();
    stream.admitted("th-7", "th-7", 0, MSFT, T + 3_600);
    stream.refused("th-8", "th-8", 0, MSFT, "not_in_data_universe");
    stream.admitted("th-9", "th-9", 0, AAPL, T + 1_800);
    let fold = stream.fold(T).unwrap();
    assert_eq!(fold.universe, universe(&[AAPL, MSFT]));
    assert_eq!(
        fold.active,
        [
            active("th-9", "th-9", 0, AAPL, "AAPL", T + 1_800),
            active("th-7", "th-7", 0, MSFT, "MSFT", T + 3_600),
        ]
    );
    assert_eq!(
        fold.lineages.lineage(&lineage_id("th-8")),
        Some(&lineage(0, 0, false))
    );
}

/// Fail closed (DEC-2210 item 4, DEC-2251 item 1): a record missing any folded member, a
/// non-boolean `admitted`, a revision that is no integer, a time it cannot read, a reason that is
/// no refusal's code, an admission with a reason or a refusal without one, and a surviving admitted
/// instrument the bundle names no symbol for, each give no fold, naming the member.
#[test]
fn a_record_it_cannot_read_gives_no_fold() {
    let mut cases: Vec<(Stream, &str)> = Vec::new();
    let mut s = Stream::default();
    s.thesis("th-1", "th-1", 0, AAPL, true, "null", "tomorrow");
    cases.push((s, "expires_at"));
    let mut s = Stream::default();
    s.refused("th-1", "th-1", 0, AAPL, "made_up");
    cases.push((s, "reason"));
    let mut s = Stream::default();
    s.thesis(
        "th-1",
        "th-1",
        0,
        AAPL,
        true,
        r#""universe_full""#,
        &stamp(T + 60),
    );
    cases.push((s, "reason"));
    let mut s = Stream::default();
    s.thesis("th-1", "th-1", 0, AAPL, false, "null", &stamp(T + 60));
    cases.push((s, "reason"));
    let mut s = Stream::default();
    s.admitted("th-1", "th-1", 0, NVDA, T + 60);
    cases.push((s, "symbol"));
    let mut s = Stream::default();
    s.push("ThesisProposed", r#"{"lineage_id":"th-1"}"#);
    cases.push((s, "thesis_id"));
    let mut s = Stream::default();
    s.push("ThesisInvalidated", r#"{"lineage_id":"th-1"}"#);
    cases.push((s, "thesis_id"));
    for member in [
        "thesis_id",
        "lineage_id",
        "instrument_id",
        "revision",
        "expires_at",
        "admitted",
        "reason",
    ] {
        let mut s = Stream::default();
        s.edited(&[(member, None)]);
        cases.push((s, member));
    }
    let mut s = Stream::default();
    s.edited(&[("admitted", Some(r#""yes""#))]);
    cases.push((s, "admitted"));
    let mut s = Stream::default();
    s.edited(&[("revision", Some(r#""0""#))]);
    cases.push((s, "revision"));
    for (stream, member) in cases {
        assert_eq!(
            stream.fold(T),
            Err(FoldError::Unreadable { member }),
            "{member}"
        );
    }
}

/// One generated research record.
#[derive(Debug, Clone)]
enum Event {
    /// A thesis of lineage `lineage` at `revision` on instrument `instrument` (0 or 1), admitted, or
    /// refused `lineage_retired` when `retires`, else `not_in_data_universe`, expiring `expires`
    /// seconds after `T`.
    Thesis {
        lineage: u8,
        revision: u32,
        instrument: u8,
        admitted: bool,
        retires: bool,
        expires: i64,
    },
    /// An invalidation of the `n`th thesis so far, modulo how many there are.
    Invalidated { n: usize },
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        4 => (0_u8..3, 0_u32..4, 0_u8..2, any::<bool>(), any::<bool>(), 1_i64..7_200).prop_map(
            |(lineage, revision, instrument, admitted, retires, expires)| Event::Thesis {
                lineage,
                revision,
                instrument,
                admitted,
                retires,
                expires,
            }
        ),
        1 => (0_usize..16).prop_map(|n| Event::Invalidated { n }),
    ]
}

/// An instrument's entry in the accumulator: its thesis, lineage, revision, horizon in seconds,
/// and whether it was invalidated.
type Entry = (String, String, u32, i64, bool);

/// The independent accumulator: §8.6 rebuilt over the generated events, with plain maps.
#[derive(Default)]
struct Oracle {
    /// lineage → (highest admitted revision, admissions, retired)
    lineages: BTreeMap<String, (u32, u32, bool)>,
    holders: BTreeMap<String, &'static str>,
    /// instrument → (thesis, lineage, revision, expires, invalidated)
    entries: BTreeMap<&'static str, Entry>,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// DEC-2249 items 1 and 2, over random journaled runs: the fold's universe, lineages and
    /// active theses at a random clock equal the accumulator's over the same events.
    #[test]
    fn the_fold_equals_an_independent_accumulator_over_random_runs(
        events in prop::collection::vec(event(), 0..24),
        now in 0_i64..7_200,
    ) {
        let (mut stream, mut oracle) = (Stream::default(), Oracle::default());
        let mut theses: Vec<(String, String, &'static str)> = Vec::new();
        for event in &events {
            match *event {
                Event::Thesis { lineage, revision, instrument, admitted, retires, expires } => {
                    let thesis = format!("th-{}", theses.len());
                    let lineage = format!("ln-{lineage}");
                    let instrument = [AAPL, MSFT][usize::from(instrument)];
                    if admitted {
                        stream.admitted(&thesis, &lineage, revision, instrument, T + expires);
                        let entry = oracle.lineages.entry(lineage.clone()).or_default();
                        entry.0 = entry.0.max(revision);
                        entry.1 += 1;
                        oracle.holders.retain(|_, held| *held != instrument);
                        oracle.holders.insert(lineage.clone(), instrument);
                        oracle.entries.insert(
                            instrument,
                            (thesis.clone(), lineage.clone(), revision, T + expires, false),
                        );
                    } else {
                        let code = if retires { "lineage_retired" } else { "not_in_data_universe" };
                        stream.refused(&thesis, &lineage, revision, instrument, code);
                        let entry = oracle.lineages.entry(lineage.clone()).or_default();
                        if retires {
                            entry.2 = true;
                        }
                    }
                    theses.push((thesis, lineage, instrument));
                }
                Event::Invalidated { n } => {
                    let Some((thesis, lineage, instrument)) =
                        theses.get(n % theses.len().max(1)).cloned()
                    else {
                        continue;
                    };
                    stream.invalidated(&thesis, &lineage, instrument);
                    for entry in oracle.entries.values_mut() {
                        if entry.0 == thesis {
                            entry.4 = true;
                        }
                    }
                }
            }
        }
        let clock = T + now;
        let fold = stream.fold(clock).expect("every generated record folds");
        let survivors: Vec<(&&str, &Entry)> = oracle
            .entries
            .iter()
            .filter(|(_, (_, lineage, _, expires, invalidated))| {
                !invalidated && !oracle.lineages[lineage].2 && clock < *expires
            })
            .collect();
        let instruments: Vec<&str> = survivors.iter().map(|(i, _)| **i).collect();
        prop_assert_eq!(&fold.universe, &universe(&instruments));
        let expected: Vec<ActiveThesis> = survivors
            .iter()
            .map(|(instrument, (thesis, lineage, revision, expires, _))| {
                let symbol = if **instrument == AAPL { "AAPL" } else { "MSFT" };
                active(thesis, lineage, *revision, instrument, symbol, *expires)
            })
            .collect();
        prop_assert_eq!(&fold.active, &expected);
        let lineages = LineageState::from_parts(
            oracle
                .lineages
                .iter()
                .map(|(l, (r, a, x))| (lineage_id(l), lineage(*r, *a, *x)))
                .collect(),
            oracle
                .holders
                .iter()
                .map(|(l, i)| (lineage_id(l), asset(i)))
                .collect(),
        );
        prop_assert_eq!(&fold.lineages, &lineages);
    }
}
