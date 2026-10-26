//! DEC-2827: an unpinned mandate's quant models are pinned on its working universe, read from the
//! agent stream's thesis records at each tick as the executor reads them. A `Long` names only the
//! one instrument a surviving thesis holds; an exit names any instrument a record ever admitted, so
//! a position the agent holds keeps its exit signal. Each instrument's symbol is its own latest
//! registered snapshot's (DEC-2819). An oracle replays the records its own way.

#[allow(
    dead_code,
    reason = "the shared fixtures serve the composer's other tests too, which use the rest"
)]
mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use mandate_canon::{Digest, Value, parse};
use mandate_daemon::models::{Evaluated, Models, Streams};
use mandate_journal::{Environment, StoredEvent};
use mandate_runtime_models::{PinnedModels, registry};
use mandate_time::Date;
use proptest::prelude::*;

const RESEARCH: &str = include_str!("../../../deploy/cell/demo/research-mandate.json");
const LLM_IN_DOCUMENT: &str = "bff754a2e669add9a7928a4b8c71c764cfe61455e361621ae6f6c262af0824cd";
const LLM: &str =
    r#"{"kind":"research_model_content","model_id":"llm.research_agent","model_version":"0.1.0"}"#;
const FEE: &str = include_str!("../../../deploy/cell/demo/fee-config.json");
const CALENDAR: &str = include_str!("../../../deploy/cell/demo/trading-calendar.json");
const RULES: &str = include_str!("../../../deploy/cell/demo/rule-set.json");
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const SPY_SNAPSHOT: &str = include_str!("../../../deploy/cell/demo/instrument.json");
const QQQ_SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"nasdaq","increment":"whole","instrument_id":"eb82ffb0-efd6-4f4e-9307-8fead28e04f6","symbol":"QQQ"}"#;
/// An hour after [`NOW`], when a thesis proposed for a day still holds its instrument.
const LATER: &str = "2026-10-26T20:00:00Z";
/// An hour before [`NOW`], when a thesis that expired then holds nothing.
const EARLIER: &str = "2026-10-26T18:00:00Z";

/// The quant model the research mandate pins, `1.0.0` (a `Long` only) or `1.1.0` (an exit too).
#[derive(Clone, Copy)]
enum Quant {
    Long,
    Exit,
}

impl Quant {
    fn version(self) -> &'static str {
        match self {
            Self::Long => "1.0.0",
            Self::Exit => "1.1.0",
        }
    }

    fn hash(self) -> String {
        mandate_modelhost::content(MODEL, self.version())
            .unwrap()
            .hash
            .to_hex()
    }
}

/// The demo's research mandate pinning `quant`, with its research model's content the tests'.
fn research(quant: Quant) -> Value {
    let text = RESEARCH
        .replace(LLM_IN_DOCUMENT, &Digest::of(LLM.as_bytes()).to_hex())
        .replace(PINNED_HASH, &quant.hash())
        .replace(
            r#""id":"quant.ma_crossover","max_output_age_s":86400,"params":[{"key":"fast_periods","value":"5"},{"key":"slow_periods","value":"20"}],"version":"1.0.0""#,
            &format!(
                r#""id":"quant.ma_crossover","max_output_age_s":86400,"params":[{{"key":"fast_periods","value":"5"}},{{"key":"slow_periods","value":"20"}}],"version":"{}""#,
                quant.version()
            ),
        );
    parse(text.as_bytes()).unwrap()
}

type Artifacts = BTreeMap<Digest, Vec<u8>>;

/// The workspace's control rows and store: both models registered, then the fee, calendar and rule
/// set, then each of `snapshots`.
fn control(quant: Quant, snapshots: &[&str]) -> (Vec<StoredEvent>, Artifacts) {
    let mut store = Artifacts::new();
    let mut put = |object: &str| {
        let object = object.trim();
        store.insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
        format!("sha256:{}", Digest::of(object.as_bytes()).to_hex())
    };
    let llm = put(LLM);
    let mut payloads = vec![
        format!(
            r#"{{"admits_instruments":true,"content_hash":"{llm}","kind":"model_version","model_id":"llm.research_agent","model_version":"0.1.0","params":[]}}"#
        ),
        format!(
            r#"{{"admits_instruments":false,"content_hash":"sha256:{}","kind":"model_version","model_id":"{MODEL}","model_version":"{}","params":{PARAMS}}}"#,
            quant.hash(),
            quant.version()
        ),
    ];
    let configs = [
        ("fee_config", FEE),
        ("trading_calendar", CALENDAR),
        ("rule_set", RULES),
    ];
    for (kind, object) in configs
        .into_iter()
        .chain(snapshots.iter().map(|s| ("instrument_snapshot", *s)))
    {
        let hash = put(object);
        payloads.push(format!(
            r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
        ));
    }
    let rows = (1..)
        .zip(payloads)
        .map(|(seq, payload)| control_row(seq, "ConfigSnapshotRegistered", "paper", &payload))
        .collect();
    (rows, store)
}

/// One record of the agent stream at `seq`, its payload `payload`.
fn agent_row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let body = format!(r#"{{"event_type":"{event_type}","payload":{payload}}}"#);
    StoredEvent {
        stream_id: "agent:ws1:agent-a".into(),
        seq,
        event_id: format!("01J8Z3MA{seq:018}"),
        event_type: event_type.into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: "2026-10-26T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// A `ThesisProposed` of thesis and lineage `id` on `instrument`, expiring at `expires`, admitted
/// unless `refused`.
fn proposed(seq: u64, id: &str, instrument: &str, expires: &str, refused: bool) -> StoredEvent {
    let (admitted, reason) = if refused {
        ("false", r#""direction_not_allowed""#)
    } else {
        ("true", "null")
    };
    agent_row(
        seq,
        "ThesisProposed",
        &format!(
            r#"{{"admitted":{admitted},"expires_at":"{expires}","instrument_id":"{instrument}","lineage_id":"{id}","reason":{reason},"revision":0,"thesis_id":"{id}"}}"#
        ),
    )
}

fn invalidated(seq: u64, id: &str) -> StoredEvent {
    agent_row(
        seq,
        "ThesisInvalidated",
        &format!(r#"{{"thesis_id":"{id}"}}"#),
    )
}

/// The cell root with each symbol's series stored: rising, or falling into an exit.
fn bars(name: &str, symbols: &[&str], falling: bool) -> Root {
    let root = Root::new(name);
    let series: Vec<(Date, u32)> = if falling {
        sessions(30, last())
            .into_iter()
            .zip((0u32..).map(|n| 20_000 - 10 * n))
            .collect()
    } else {
        rising(30)
    };
    for symbol in symbols {
        store_series(&root.bars(), symbol, &series, None);
    }
    root
}

fn composer(root: &Root, quant: Quant, rows: &[StoredEvent], store: Artifacts) -> PinnedModels {
    PinnedModels::working(
        &research(quant),
        registry(rows, Environment::Paper).unwrap(),
        &root.bars(),
        Box::new(store),
    )
    .unwrap()
}

/// What an output says: a `Long`, or an exit (conviction −1, DEC-2390).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    Long,
    Exit,
}

/// The instruments of `answer` and what each says, in order.
fn named(answer: &[Evaluated]) -> Vec<(String, Said)> {
    answer
        .iter()
        .map(|e| {
            let said = if e.output.conviction.to_string().starts_with('-') {
                Said::Exit
            } else {
                Said::Long
            };
            (e.output.instrument_id.as_str().to_owned(), said)
        })
        .collect()
}

fn evaluate(
    models: &PinnedModels,
    control: Option<&[StoredEvent]>,
    agent: &[StoredEvent],
) -> Vec<(String, Said)> {
    named(&models.evaluate(tick(NOW), Streams { control, agent }))
}

/// GW's repro: the research mandate pins nothing, so before DEC-2827 its admitted SPY was never
/// evaluated. Now the admitted SPY gets its `Long`; with no thesis, nothing is evaluated.
#[test]
fn a_research_mandate_s_quant_model_is_pinned_on_its_admitted_instrument() {
    let root = bars("admitted", &["SPY"], false);
    let (rows, store) = control(Quant::Long, &[QQQ_SNAPSHOT, SPY_SNAPSHOT]);
    let models = composer(&root, Quant::Long, &rows, store);
    let theses = [proposed(1, "th-1", SPY, LATER, false)];
    assert_eq!(
        evaluate(&models, Some(&rows), &theses),
        [(SPY.to_owned(), Said::Long)]
    );
    assert_eq!(evaluate(&models, Some(&rows), &[]), []);
    assert!(
        PinnedModels::new(
            &research(Quant::Long),
            registry(&rows, Environment::Paper).unwrap(),
            &root.bars()
        )
        .is_none(),
        "the pinned reading still pins nothing for a research mandate"
    );
}

/// A refused thesis admits nothing; a thesis that has expired or been invalidated gives no `Long`,
/// but the instrument it admitted keeps its exit output, which a held position sells on.
#[test]
fn an_ended_thesis_gives_no_long_and_keeps_its_instrument_s_exit() {
    let (rows, store) = control(Quant::Long, &[SPY_SNAPSHOT]);
    let rising = bars("ended-rising", &["SPY"], false);
    let models = composer(&rising, Quant::Long, &rows, store);
    for theses in [
        vec![proposed(1, "th-1", SPY, LATER, true)],
        vec![proposed(1, "th-1", SPY, EARLIER, false)],
        vec![
            proposed(1, "th-1", SPY, LATER, false),
            invalidated(2, "th-1"),
        ],
    ] {
        assert_eq!(evaluate(&models, Some(&rows), &theses), []);
    }
    let (rows, store) = control(Quant::Exit, &[SPY_SNAPSHOT]);
    let falling = bars("ended-falling", &["SPY"], true);
    let models = composer(&falling, Quant::Exit, &rows, store);
    let exit = [(SPY.to_owned(), Said::Exit)];
    assert_eq!(
        evaluate(
            &models,
            Some(&rows),
            &[proposed(1, "th-1", SPY, EARLIER, false)]
        ),
        exit
    );
    assert_eq!(
        evaluate(
            &models,
            Some(&rows),
            &[proposed(1, "th-1", SPY, LATER, true)]
        ),
        [],
        "a refused thesis admitted nothing to exit"
    );
}

/// A thesis record that cannot be read holds every `Long`, as it holds the executor's openings, and
/// takes no instrument another record admitted from its exits.
#[test]
fn an_unreadable_thesis_record_holds_longs_and_keeps_exits() {
    let unreadable = agent_row(2, "ThesisProposed", r#"{"admitted":true}"#);
    let (rows, store) = control(Quant::Long, &[SPY_SNAPSHOT]);
    let rising = bars("unreadable-rising", &["SPY"], false);
    let models = composer(&rising, Quant::Long, &rows, store);
    let theses = [proposed(1, "th-1", SPY, LATER, false), unreadable.clone()];
    assert_eq!(evaluate(&models, Some(&rows), &theses), []);
    let (rows, store) = control(Quant::Exit, &[SPY_SNAPSHOT]);
    let falling = bars("unreadable-falling", &["SPY"], true);
    let models = composer(&falling, Quant::Exit, &rows, store);
    let theses = [unreadable, proposed(3, "th-1", SPY, LATER, false)];
    assert_eq!(
        evaluate(&models, Some(&rows), &theses),
        [(SPY.to_owned(), Said::Exit)]
    );
}

/// Two surviving instruments give no `Long`, as the executor holds both openings; an instrument with
/// no snapshot of its own, or control rows that do not read, give no output at all; a control row
/// that does not read holds the `Long` and keeps the exit.
#[test]
fn two_survivors_an_unregistered_instrument_or_unread_control_give_no_long() {
    let (rows, store) = control(Quant::Long, &[SPY_SNAPSHOT, QQQ_SNAPSHOT]);
    let both = bars("both", &["SPY", "QQQ"], false);
    let models = composer(&both, Quant::Long, &rows, store);
    let theses = [
        proposed(1, "th-1", SPY, LATER, false),
        proposed(2, "th-2", QQQ, LATER, false),
    ];
    assert_eq!(evaluate(&models, Some(&rows), &theses), []);
    let only_spy = [proposed(1, "th-1", SPY, LATER, false)];
    assert_eq!(evaluate(&models, None, &only_spy), []);
    let (rows, store) = control(Quant::Long, &[QQQ_SNAPSHOT]);
    let models = composer(&both, Quant::Long, &rows, store);
    assert_eq!(evaluate(&models, Some(&rows), &only_spy), []);

    let unread = |mut rows: Vec<StoredEvent>| {
        rows.push(StoredEvent {
            body: b"not json".to_vec(),
            ..control_row(9, "AgentDeployed", "paper", "{}")
        });
        rows
    };
    let (rows, store) = control(Quant::Exit, &[SPY_SNAPSHOT]);
    let rows = unread(rows);
    let falling = bars("control-unread", &["SPY"], true);
    let models = composer(&falling, Quant::Exit, &rows, store);
    assert_eq!(
        evaluate(&models, Some(&rows), &only_spy),
        [(SPY.to_owned(), Said::Exit)],
        "the rows that read still map a held instrument's exit"
    );
    let (rows, store) = control(Quant::Long, &[SPY_SNAPSHOT]);
    let models = composer(&both, Quant::Long, &rows, store.clone());
    assert_eq!(
        evaluate(&models, Some(&rows), &only_spy),
        [(SPY.to_owned(), Said::Long)]
    );
    let rows = unread(rows);
    let models = composer(&both, Quant::Long, &rows, store);
    assert_eq!(evaluate(&models, Some(&rows), &only_spy), []);
}

/// The symbol, which names the daily dataset, is the latest snapshot's that names the instrument;
/// a snapshot whose object the store does not hold gives no output, never an earlier snapshot's.
#[test]
fn the_latest_snapshot_names_the_dataset_and_an_unreadable_one_gives_none() {
    let renamed = SPY_SNAPSHOT.replace(r#""symbol":"SPY""#, r#""symbol":"SPYX""#);
    let root = bars("renamed", &["SPYX"], false);
    let theses = [proposed(1, "th-1", SPY, LATER, false)];
    let (rows, store) = control(Quant::Long, &[SPY_SNAPSHOT, &renamed, QQQ_SNAPSHOT]);
    let models = composer(&root, Quant::Long, &rows, store.clone());
    assert_eq!(
        evaluate(&models, Some(&rows), &theses),
        [(SPY.to_owned(), Said::Long)]
    );
    let (rows, _) = control(Quant::Long, &[&renamed, SPY_SNAPSHOT]);
    let models = composer(&root, Quant::Long, &rows, store.clone());
    assert_eq!(evaluate(&models, Some(&rows), &theses), []);
    let mut held = store;
    held.remove(&Digest::of(QQQ_SNAPSHOT.as_bytes()));
    let (rows, _) = control(Quant::Long, &[&renamed, QQQ_SNAPSHOT]);
    let models = composer(&root, Quant::Long, &rows, held);
    assert_eq!(evaluate(&models, Some(&rows), &theses), []);
}

/// A pinned mandate is read only as before: the working reading refuses it, and its evaluation
/// never reads the agent stream's theses.
#[test]
fn a_pinned_mandate_is_never_read_as_a_working_universe() {
    let root = bars("pinned", &["QQQ"], false);
    let rows = registered();
    assert!(
        PinnedModels::working(
            &document(),
            registry(&rows, Environment::Paper).unwrap(),
            &root.bars(),
            Box::new(Artifacts::new())
        )
        .is_none()
    );
    let models = PinnedModels::new(
        &document(),
        registry(&rows, Environment::Paper).unwrap(),
        &root.bars(),
    )
    .unwrap();
    let qqq = [(QQQ.to_owned(), Said::Long)];
    assert_eq!(evaluate(&models, None, &[]), qqq);
    let refused = [proposed(1, "th-1", QQQ, EARLIER, true)];
    assert_eq!(evaluate(&models, Some(&rows), &refused), qqq);
}

/// One generated agent-stream record: a proposal on SPY (`false`) or QQQ (`true`), admitted or
/// refused, expiring before or after the tick; or the invalidation of the `n`th proposal.
#[derive(Debug, Clone)]
enum Record {
    Proposed {
        qqq: bool,
        admitted: bool,
        survives: bool,
    },
    Invalidated(usize),
    Unreadable,
}

fn records() -> impl Strategy<Value = Vec<Record>> {
    prop::collection::vec(
        prop_oneof![
            4 => (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(qqq, admitted, survives)| {
                Record::Proposed { qqq, admitted, survives }
            }),
            2 => (0usize..8).prop_map(Record::Invalidated),
            1 => Just(Record::Unreadable),
        ],
        0..8,
    )
}

/// The rows of `records`, and, computed without the replay, the one instrument a `Long` may name
/// and every instrument an exit may.
fn oracle(records: &[Record]) -> (Vec<StoredEvent>, Option<String>, BTreeSet<String>) {
    let mut rows = Vec::new();
    let mut holding: BTreeMap<&str, (String, bool, bool)> = BTreeMap::new();
    let mut held = BTreeSet::new();
    let mut readable = true;
    for (n, record) in records.iter().enumerate() {
        let seq = u64::try_from(n).unwrap() + 1;
        match record {
            Record::Proposed {
                qqq,
                admitted,
                survives,
            } => {
                let instrument = if *qqq { QQQ } else { SPY };
                let id = format!("th-{n}");
                let expires = if *survives { LATER } else { EARLIER };
                rows.push(proposed(seq, &id, instrument, expires, !admitted));
                if *admitted {
                    held.insert(instrument.to_owned());
                    holding.insert(instrument, (id, *survives, false));
                }
            }
            Record::Invalidated(target) => {
                let id = format!("th-{target}");
                rows.push(invalidated(seq, &id));
                for entry in holding.values_mut() {
                    if entry.0 == id {
                        entry.2 = true;
                    }
                }
            }
            Record::Unreadable => {
                rows.push(agent_row(seq, "ThesisProposed", r#"{"admitted":true}"#));
                readable = false;
            }
        }
    }
    let surviving: Vec<&str> = holding
        .iter()
        .filter(|(_, (_, survives, ended))| *survives && !*ended)
        .map(|(instrument, _)| *instrument)
        .collect();
    let open = match (readable, surviving.as_slice()) {
        (true, [one]) => Some((*one).to_owned()),
        _ => None,
    };
    (rows, open, held)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// The models' `Long`s name exactly the instrument the executor opens in, and their exits
    /// exactly the instruments the executor maps a held position's exits to.
    #[test]
    fn the_models_pin_exactly_the_executor_s_working_universe(records in records()) {
        let (agent, open, held) = oracle(&records);
        let snapshots = [SPY_SNAPSHOT, QQQ_SNAPSHOT];
        let (rows, store) = control(Quant::Long, &snapshots);
        let rising = bars("property-rising", &["SPY", "QQQ"], false);
        let models = composer(&rising, Quant::Long, &rows, store);
        let longs: Vec<String> = evaluate(&models, Some(&rows), &agent)
            .into_iter()
            .map(|(instrument, direction)| {
                assert_eq!(direction, Said::Long);
                instrument
            })
            .collect();
        prop_assert_eq!(longs, open.into_iter().collect::<Vec<_>>());

        let (rows, store) = control(Quant::Exit, &snapshots);
        let falling = bars("property-falling", &["SPY", "QQQ"], true);
        let models = composer(&falling, Quant::Exit, &rows, store);
        let exits: BTreeSet<String> = evaluate(&models, Some(&rows), &agent)
            .into_iter()
            .map(|(instrument, direction)| {
                assert_eq!(direction, Said::Exit);
                instrument
            })
            .collect();
        prop_assert_eq!(exits, held);
    }
}
