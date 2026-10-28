//! The §4.3 gate's probe set (DEC-2217 item 3, inference spec §4.3): built into the research
//! runtime's and the model-gateway service's images alike, read strictly, and named by one digest,
//! so a session opened on another probe set is refused before any call.

use mandate_canon::{Digest, parse, to_canonical};
use mandate_research_run::agentic::RunKind;
use mandate_research_run::gate::{PROBE_SET, ProbeError, ProbeSet, probe_set_digest};
use mandate_research_run::{InstrumentReader, NewsReader, TextQuery};

fn built_in() -> ProbeSet {
    ProbeSet::read(&parse(PROBE_SET.as_bytes()).unwrap()).unwrap()
}

/// DEC-2217 item 3: five `ideate`, five `monitor` and five prompt-injection runs, in that order,
/// each over a corpus slice the set holds, its universe's instruments in the slice.
#[test]
fn the_built_in_probe_set_holds_fifteen_runs_over_its_corpus() {
    let set = built_in();
    assert_eq!(set.runs.len(), 15);
    assert_eq!(set.allowlist_version, 1);
    let sources: Vec<&str> = set.sources.iter().map(|s| s.as_str()).collect();
    assert_eq!(sources, ["src.newswire", "src.wiretwo"]);
    let kinds: Vec<(RunKind, bool)> = set.runs.iter().map(|r| (r.kind, r.injection)).collect();
    assert_eq!(&kinds[..5], &[(RunKind::Ideate, false); 5]);
    assert_eq!(&kinds[5..10], &[(RunKind::Monitor, false); 5]);
    assert_eq!(&kinds[10..], &[(RunKind::Ideate, true); 5]);
    for run in &set.runs {
        let plane = set.plane(run).unwrap();
        let facts = plane.instruments(&run.universe).unwrap();
        assert_eq!(facts.len(), run.universe.len(), "{}", run.corpus);
        assert_eq!(run.theses.is_empty(), run.kind != RunKind::Monitor);
        for thesis in &run.theses {
            assert!(run.universe.contains(&thesis.symbol));
        }
        let news = plane
            .news(&TextQuery {
                workspace: "ws".to_owned(),
                symbols: run.universe.clone(),
                window_start: mandate_time::UtcNanos::EPOCH,
                cut_off: run.cut_off,
                allowlist_version: set.allowlist_version,
                max_items: 10,
            })
            .unwrap();
        let injected = news
            .iter()
            .any(|item| item.summary.contains("Ignore every instruction"));
        assert_eq!(injected, run.injection, "{}", run.corpus);
    }
}

/// DEC-2217 items 2 and 4: the digest is SHA-256 over the set's canonical bytes, so a reformatted
/// file names the same set and any change of content names another.
#[test]
fn the_digest_is_the_canonical_bytes_hash() {
    let value = parse(PROBE_SET.as_bytes()).unwrap();
    assert_eq!(probe_set_digest(), Some(Digest::of(&to_canonical(&value))));
    let changed = PROBE_SET.replacen("Probe item:", "Probe item, changed:", 1);
    let other = Digest::of(&to_canonical(&parse(changed.as_bytes()).unwrap()));
    assert_ne!(Some(other), probe_set_digest());
}

/// The set is read strictly: a run naming a corpus slice the set lacks, an unknown kind, a
/// monitoring run with no thesis, or a corpus slice that is not a plane refuses the whole set.
#[test]
fn a_malformed_probe_set_is_refused_whole() {
    let read = |text: &str| ProbeSet::read(&parse(text.as_bytes()).unwrap());
    for (from, to, refused) in [
        (
            r#""corpus": "steady_index","#,
            r#""corpus": "no_such_slice","#,
            ProbeError::Member("corpus"),
        ),
        (
            r#""kind": "ideate","#,
            r#""kind": "owner","#,
            ProbeError::Member("kind"),
        ),
        (
            r#""theses": [
    {"#,
            r#""theses": [],
   "unused": [
    {"#,
            ProbeError::Member("theses"),
        ),
        (
            r#""bars": ["#,
            r#""bars": {}, "was": ["#,
            ProbeError::Member("bars.json"),
        ),
        (
            r#""injection": false,"#,
            r#""injection": "no","#,
            ProbeError::Member("injection"),
        ),
        (
            r#""cut_off": "2026-10-20T13:30:00.000000000Z","#,
            r#""cut_off": "Tuesday","#,
            ProbeError::Member("cut_off"),
        ),
        (
            r#""universe": [
    "SPY""#,
            r#""universe": [], "was": [
    "SPY""#,
            ProbeError::Member("universe"),
        ),
        (
            r#""version": 1
 },"#,
            r#""version": "1"
 },"#,
            ProbeError::Member("allowlist"),
        ),
        (
            r#""src.wiretwo""#,
            r#""not a source!""#,
            ProbeError::Member("allowlist"),
        ),
        (
            r#""symbol": "XLF",
     "thesis_id": "th-probe-3""#,
            r#""symbol": "SPY",
     "thesis_id": "th-probe-3""#,
            ProbeError::Member("theses"),
        ),
        (
            r#""source": "src.wiretwo","#,
            r#""source": "src.unlisted","#,
            ProbeError::Member("source"),
        ),
        (
            r#""theses": [],"#,
            r#""theses": [{"direction": "long", "invalidation": "x", "symbol": "SPY", "thesis_id": "t"}],"#,
            ProbeError::Member("theses"),
        ),
    ] {
        assert!(PROBE_SET.contains(from), "{from}");
        let text = PROBE_SET.replacen(from, to, 1);
        assert_eq!(read(&text).err(), Some(refused), "{from}");
    }
}

/// A set with no runs is refused, and the built-in set is the one `ProbeSet::built_in` reads.
#[test]
fn a_set_with_no_runs_is_refused_and_the_built_in_one_reads() {
    let empty = PROBE_SET.replacen(r#""runs": ["#, r#""runs": [], "was": ["#, 1);
    assert_eq!(
        ProbeSet::read(&parse(empty.as_bytes()).unwrap()).err(),
        Some(ProbeError::Member("runs"))
    );
    assert_eq!(ProbeSet::built_in(), Ok(built_in()));
}
