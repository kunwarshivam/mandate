//! The journal's `asset_id` check and `mandate-domain`'s `AssetId::parse` are two copies of one
//! rule (journal spec v0.10 §9.3, mandate spec §3). Over a generated corpus, a `UniverseChanged`
//! draft appends exactly when `AssetId::parse` accepts its `instrument`, so neither copy can drift
//! without this failing; a drift would let a record append that the mapping cannot read, leaving the
//! stream's context unbuildable (DEC-404 item 9; #509 round 1, m1).

use std::path::Path;
use std::sync::Arc;

use mandate_domain::AssetId;
use mandate_journal::Draft;
use mandate_refcases::{Json, read_fixture};

const BASE: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const ALPHABET: &str = "0123456789abcdefABCDEFgGzZ-_ .";

fn universe_change() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    let fixture = Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").expect("the fixture"));
    fixture
        .get("risk_state")
        .and_then(|s| s.get("drafts"))
        .and_then(|d| d.get("universe_admitted"))
        .cloned()
        .expect("the risk_state section's universe_admitted draft")
}

/// Every single-character substitution, deletion and insertion of a valid asset ID over an alphabet
/// that holds every legal character, their uppercase twins, and the near misses, plus whole-string
/// edge cases.
fn corpus() -> Vec<String> {
    let base: Vec<char> = BASE.chars().collect();
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    let mut out = vec![
        BASE.to_owned(),
        BASE.to_uppercase(),
        "00000000-0000-0000-0000-000000000000".to_owned(),
        "ffffffff-ffff-ffff-ffff-ffffffffffff".to_owned(),
        BASE.replace('-', ""),
        format!("{BASE}-0000"),
        String::new(),
    ];
    for at in 0..=base.len() {
        if at < base.len() {
            let mut deleted = base.clone();
            deleted.remove(at);
            out.push(deleted.iter().collect());
        }
        for c in &alphabet {
            if at < base.len() {
                let mut substituted = base.clone();
                substituted[at] = *c;
                out.push(substituted.iter().collect());
            }
            let mut inserted = base.clone();
            inserted.insert(at, *c);
            out.push(inserted.iter().collect());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn the_journal_appends_an_instrument_exactly_when_asset_id_parses_it() {
    let base = universe_change();
    let (mut accepted, mut refused) = (0, 0);
    let mut disagreements = Vec::new();
    for instrument in corpus() {
        let mut draft = base.clone();
        draft["payload"]["instrument"] = Json::String(instrument.clone());
        let bytes = serde_json::to_vec(&draft).expect("a draft serializes");
        let journal = Draft::parse(&bytes).is_ok();
        let domain = AssetId::parse(&instrument).is_ok();
        if journal == domain {
            if domain {
                accepted += 1;
            } else {
                refused += 1;
            }
        } else {
            disagreements.push(format!(
                "{instrument:?}: journal {journal}, domain {domain}"
            ));
        }
    }
    assert!(
        accepted > 100 && refused > 1000,
        "the corpus has both sides: {accepted} accepted, {refused} refused"
    );
    assert!(disagreements.is_empty(), "{}", disagreements.join("\n"));
}
