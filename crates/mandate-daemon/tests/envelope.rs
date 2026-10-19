//! The research envelope of a mandate document (DEC-2214 item 2): each member read as the
//! document states it, and any member that cannot be read refusing the envelope. The oracle reads
//! the document's JSON itself.

use std::collections::BTreeSet;

use mandate_canon::{Digest, parse};
use mandate_daemon::source::{SourceError, envelope};
use mandate_num::Usd;
use mandate_research::{AssetClass, AutonomyDecision};
use serde_json::{Value as Json, json};

const MANDATE: &str = include_str!("fixtures/mandate.json");

/// The fixture mandate made one the research agent may work under: an unpinned universe of up to
/// five instruments, an admitting signal model, and a research envelope.
fn researching() -> Json {
    let mut document: Json = serde_json::from_str(MANDATE).unwrap();
    document["universe"]["pinned"] = json!(false);
    document["universe"]["pinned_instruments"] = json!([]);
    document["universe"]["max_instruments"] = json!(5);
    document["behavior"]["signal_models"][0]["admits_instruments"] = json!(true);
    document["behavior"]["research"] = json!({
        "interval_s": 3600,
        "cost_cap_usd_per_day": "5",
        "max_revisions_per_lineage": 3
    });
    document
}

/// The envelope is the document's own, member by member (mandate spec §8.5 and §8.6), for the
/// fixture as committed and for a researching one.
#[test]
#[ignore = "pending E21-54"]
fn the_envelope_is_the_documents_own() {
    for document in [
        serde_json::from_str::<Json>(MANDATE).unwrap(),
        researching(),
    ] {
        let read = envelope(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
        let universe = &document["universe"];
        assert_eq!(read.universe_pinned, universe["pinned"].as_bool().unwrap());
        assert_eq!(
            u64::from(read.max_instruments),
            universe["max_instruments"].as_u64().unwrap()
        );
        assert_eq!(read.asset_classes, BTreeSet::from([AssetClass::UsEquity]));
        assert_eq!(
            read.leveraged_etps_enabled,
            universe["leveraged_etps_enabled"].as_bool().unwrap()
        );
        assert_eq!(read.leveraged_etp_disclosure_version, None);
        let models = document["behavior"]["signal_models"].as_array().unwrap();
        assert_eq!(
            read.admits_instruments,
            models
                .iter()
                .any(|m| m["admits_instruments"] == json!(true))
        );
        let research = &document["behavior"]["research"];
        match &read.research {
            None => assert!(research.is_null()),
            Some(envelope) => {
                assert_eq!(
                    u64::from(envelope.interval_s),
                    research["interval_s"].as_u64().unwrap()
                );
                assert_eq!(
                    envelope.cost_cap_usd_per_day,
                    Usd::parse(research["cost_cap_usd_per_day"].as_str().unwrap()).unwrap()
                );
                assert_eq!(
                    u64::from(envelope.max_revisions_per_lineage),
                    research["max_revisions_per_lineage"].as_u64().unwrap()
                );
            }
        }
        assert_eq!(read.admission, AutonomyDecision::Ask);
    }
}

/// Every value the mandate schema allows is read as itself: each asset class, a disclosure version,
/// and each admission decision.
#[test]
#[ignore = "pending E21-54"]
fn the_envelope_reads_every_value_the_schema_allows() {
    let disclosure = Digest::of(b"the leveraged ETP disclosure");
    for (admission, decision) in [
        ("auto", AutonomyDecision::Auto),
        ("ask", AutonomyDecision::Ask),
        ("deny", AutonomyDecision::Deny),
    ] {
        let mut document = researching();
        document["universe"]["asset_classes"] = json!(["us_equity", "crypto"]);
        document["universe"]["leveraged_etp_disclosure_version"] =
            json!(format!("sha256:{}", disclosure.to_hex()));
        document["autonomy"]["admission"] = json!(admission);
        let read = envelope(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            read.asset_classes,
            BTreeSet::from([AssetClass::UsEquity, AssetClass::Crypto])
        );
        assert_eq!(read.leveraged_etp_disclosure_version, Some(disclosure));
        assert_eq!(read.admission, decision, "{admission}");
    }
}

/// A member the envelope cannot read refuses it, naming that member: nothing is defaulted.
#[test]
#[ignore = "pending E21-54"]
fn an_envelope_member_it_cannot_read_refuses_it() {
    type Edit = fn(&mut Json);
    let cases: [(&str, Edit); 10] = [
        ("universe.pinned", |d| d["universe"]["pinned"] = json!("no")),
        ("universe.max_instruments", |d| {
            d["universe"]["max_instruments"] = json!(4_294_967_296_u64)
        }),
        ("universe.asset_classes", |d| {
            d["universe"]["asset_classes"] = json!(["options"])
        }),
        ("universe.leveraged_etps_enabled", |d| {
            d["universe"]["leveraged_etps_enabled"] = json!(null)
        }),
        ("universe.leveraged_etp_disclosure_version", |d| {
            d["universe"]["leveraged_etp_disclosure_version"] = json!("sha256:nothex")
        }),
        ("behavior.signal_models", |d| {
            d["behavior"]["signal_models"][0]
                .as_object_mut()
                .unwrap()
                .remove("admits_instruments");
        }),
        ("behavior.research", |d| {
            d["behavior"].as_object_mut().unwrap().remove("research");
        }),
        ("behavior.research.interval_s", |d| {
            d["behavior"]["research"]
                .as_object_mut()
                .unwrap()
                .remove("interval_s");
        }),
        ("behavior.research.cost_cap_usd_per_day", |d| {
            d["behavior"]["research"]["cost_cap_usd_per_day"] = json!("five")
        }),
        ("autonomy.admission", |d| {
            d["autonomy"]["admission"] = json!("maybe")
        }),
    ];
    for (member, edit) in cases {
        let mut document = researching();
        edit(&mut document);
        assert_eq!(
            envelope(&parse(document.to_string().as_bytes()).unwrap()),
            Err(SourceError::Unreadable { member }),
            "{member}"
        );
    }
}
