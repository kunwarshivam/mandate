//! The cell's recorded research bundle (DEC-2210 items 1 and 2): what loads, what refuses, and the
//! recorded readers' ages, read through the harness's own `retrieve`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_canon::{Value, parse, to_canonical};
use mandate_daemon::recorded::{Bundle, BundleError};
use mandate_gateway::money::Usd6;
use mandate_research_run::{
    Allowlist, FilingReader, InstrumentReader, NewsReader, Readers, RetrievalPlan, RetrievedItem,
    Scope, SourceId, Symbol, TextQuery, retrieve,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const FILES: [&str; 4] = [
    "research.json",
    "bars.json",
    "news.json",
    "instruments.json",
];

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn symbol(text: &str) -> Symbol {
    Symbol::new(text).unwrap()
}

/// A copy of the fixture bundle in a directory of its own, with `edit` applied to one file's JSON.
fn copy(name: &str, file: &str, edit: impl FnOnce(&mut Value)) -> PathBuf {
    let mut edit = Some(edit);
    let dir = std::env::temp_dir().join(format!("daemon-{}-bundle-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for each in FILES {
        let mut value = parse(&fs::read(Path::new(FIXTURE).join(each)).unwrap()).unwrap();
        if each == file
            && let Some(edit) = edit.take()
        {
            edit(&mut value);
        }
        fs::write(dir.join(each), to_canonical(&value)).unwrap();
    }
    dir
}

fn remove(value: &mut Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut node = value;
    for parent in parents {
        node = match node {
            Value::Object(object) => object.get_mut(*parent).unwrap(),
            Value::Array(rows) => rows.get_mut(parent.parse::<usize>().unwrap()).unwrap(),
            _ => panic!("{parent} is not a container"),
        };
    }
    let Value::Object(object) = node else {
        panic!("{last}'s parent is not an object");
    };
    assert!(object.remove(*last).is_some(), "{last} is in the fixture");
}

#[test]
fn the_bundle_loads_every_file_it_names() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    assert_eq!(bundle.entry.model_id, "llm.research_agent");
    assert_eq!(bundle.entry.seed, Some(7));
    assert_eq!(bundle.entry.plan, RetrievalPlan::v0_1());
    assert_eq!(bundle.endpoint.route.upstream, "anthropic");
    assert_eq!(
        bundle.probe.model.as_deref(),
        Some("anthropic/claude-sonnet-4.5")
    );
    assert_eq!(bundle.workspace_cap, Usd6::dollars(50).unwrap());
    assert_eq!(bundle.deployment_cap, Usd6::dollars(20).unwrap());
    assert_eq!(bundle.reply.usage.input, 1_800);
    assert!(bundle.reply.output.contains("nw-aapl-services"));
    assert_eq!(bundle.plane.bars.len(), 4);
    assert_eq!(bundle.plane.news.len(), 4);
    assert_eq!(bundle.plane.instruments.len(), 2);
}

#[test]
fn the_registry_offers_the_entry_on_its_one_endpoint() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    let registry = bundle.registry().unwrap();
    let entry = registry
        .resolve(&bundle.entry.content().model_ref())
        .unwrap_or_else(|_| panic!("the pin resolves"));
    let endpoints: Vec<_> = entry.endpoints().map(|(e, _)| e.id.clone()).collect();
    assert_eq!(endpoints, ["openrouter:anthropic"]);
    assert!(entry.serves(&bundle.endpoint, &bundle.probe));
}

#[test]
fn a_missing_file_refuses_the_load() {
    for file in FILES {
        let dir = copy(&format!("missing-{file}"), "", |_| {});
        fs::remove_file(dir.join(file)).unwrap();
        assert_eq!(
            Bundle::load(&dir),
            Err(BundleError::Unreadable { file }),
            "{file}"
        );
    }
}

#[test]
fn a_missing_member_refuses_the_load_and_nothing_is_defaulted() {
    let cases: [(&str, &[&str], &str); 8] = [
        ("research.json", &["entry", "seed"], "seed"),
        ("research.json", &["endpoint", "upstream"], "upstream"),
        (
            "research.json",
            &["caps", "deployment_usd"],
            "deployment_usd",
        ),
        ("research.json", &["reply", "output"], "output"),
        ("news.json", &["0", "licensed"], "licensed"),
        ("news.json", &["1", "age_s"], "age_s"),
        ("bars.json", &["2", "volume"], "volume"),
        ("instruments.json", &["0", "eligible"], "eligible"),
    ];
    for (index, (file, path, member)) in cases.into_iter().enumerate() {
        let dir = copy(&format!("member-{index}"), file, |value| {
            remove(value, path)
        });
        assert_eq!(
            Bundle::load(&dir),
            Err(BundleError::Malformed { file, member }),
            "{file} {path:?}"
        );
    }
}

#[test]
fn text_that_is_not_strict_json_refuses_the_load() {
    let dir = copy("not-json", "", |_| {});
    fs::write(dir.join("news.json"), b"[{\"age_s\": 1.5}]").unwrap();
    assert_eq!(
        Bundle::load(&dir),
        Err(BundleError::Malformed {
            file: "news.json",
            member: ""
        })
    );
}

#[test]
fn the_plane_gives_no_filings_and_only_the_instruments_asked_for() {
    let plane = Bundle::load(Path::new(FIXTURE)).unwrap().plane;
    let query = TextQuery {
        workspace: "ws1".to_owned(),
        symbols: BTreeSet::from([symbol("AAPL")]),
        window_start: at(1_792_000_000),
        cut_off: at(1_792_180_800),
        allowlist_version: 3,
        max_items: 50,
    };
    assert_eq!(plane.filings(&query).unwrap(), Vec::new());
    let facts = plane
        .instruments(&BTreeSet::from([symbol("MSFT"), symbol("SPY")]))
        .unwrap();
    let symbols: Vec<_> = facts.iter().map(|f| f.symbol.as_str().to_owned()).collect();
    assert_eq!(symbols, ["MSFT"]);
}

/// The scope a run at `cut_off` reads with: AAPL and MSFT, two allowlisted wires.
fn scope(cut_off: UtcNanos) -> Scope {
    Scope {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        cut_off,
        data_universe: BTreeSet::from([symbol("AAPL"), symbol("MSFT")]),
        held: BTreeSet::new(),
        allowlist: Allowlist {
            version: 3,
            sources: ["src.newswire", "src.wire_two"]
                .into_iter()
                .map(|s| SourceId::new(s).unwrap())
                .collect(),
        },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every recorded item is stamped at its age before the read's cut-off, whatever the date, so
    /// the same items stay in the plan's windows on any day (DEC-2210 item 2). The oracle reads
    /// the ages from the fixture's own JSON, not from the loaded bundle.
    #[test]
    fn recorded_items_are_stamped_at_their_age_before_each_cut_off(
        cut_off in 1_700_000_000_i64..2_100_000_000,
    ) {
        let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
        let news = parse(&fs::read(Path::new(FIXTURE).join("news.json")).unwrap()).unwrap();
        let ages: Vec<(String, i64)> = news
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let id = row.get("item_id").and_then(Value::as_str).unwrap().to_owned();
                let age = row.get("age_s").and_then(Value::as_int).unwrap();
                (id, i64::try_from(age).unwrap())
            })
            .collect();
        let query = TextQuery {
            workspace: "ws1".to_owned(),
            symbols: BTreeSet::new(),
            window_start: at(cut_off - 172_800),
            cut_off: at(cut_off),
            allowlist_version: 3,
            max_items: 50,
        };
        let stamped = bundle.plane.news(&query).unwrap();
        prop_assert_eq!(stamped.len(), ages.len());
        for (item, (id, age)) in stamped.iter().zip(&ages) {
            prop_assert_eq!(item.item_id.as_str(), id.as_str());
            prop_assert_eq!(item.knowledge_at, at(cut_off - age));
        }

        let readers = Readers {
            bars: &bundle.plane,
            news: &bundle.plane,
            filings: &bundle.plane,
            instruments: &bundle.plane,
        };
        let retrieval = retrieve(&RetrievalPlan::v0_1(), &scope(at(cut_off)), readers).unwrap();
        let mut news_ids: Vec<&str> = retrieval
            .items
            .iter()
            .filter_map(|item| match item {
                RetrievedItem::News(news) => Some(news.item_id.as_str()),
                _ => None,
            })
            .collect();
        news_ids.sort_unstable();
        prop_assert_eq!(news_ids, ["nw-aapl-services", "w2-aapl-supply"]);
        let bars = retrieval
            .items
            .iter()
            .filter(|item| matches!(item, RetrievedItem::Bar(_)))
            .count();
        prop_assert_eq!(bars, 4);
    }
}
