//! DEC-2283 item 3, data-plane spec §3.8 "Instruments" and "Scope": the instruments a workspace's
//! IEX generations hold are its registered US-equity instruments, the symbol of each asset id's
//! latest `instrument_snapshot`, at most 50 in order of first registration. Two asset ids naming
//! one symbol both get none, and a control stream with any row not of the `paper` environment gets
//! nothing at all. The oracle is each case's list written out by hand.

use std::collections::BTreeMap;

use mandate_canon::Digest;
use mandate_datad::iex::instruments::{BARS_INSTRUMENTS, Registered, registered};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent};

#[derive(Default)]
struct Objects(BTreeMap<String, Vec<u8>>);

impl Objects {
    fn put(&mut self, bytes: &str) -> String {
        let reference = ArtifactRef::of(bytes.as_bytes());
        self.0
            .insert(reference.to_string(), bytes.as_bytes().to_vec());
        reference.to_string()
    }
}

impl ArtifactSource for Objects {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.0
            .get(&reference.to_string())
            .cloned()
            .ok_or(ArtifactError::Missing)
    }
}

fn row(seq: u64, environment: &str, event_type: &str, payload: &str) -> StoredEvent {
    StoredEvent {
        stream_id: "ctl:ws".to_owned(),
        seq,
        event_id: format!("e{seq}"),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: "2026-10-27T12:00:00Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::ZERO,
        body: format!(r#"{{"payload":{payload}}}"#).into_bytes(),
    }
}

fn snapshot(objects: &mut Objects, asset: &str, symbol: &str, class: &str) -> String {
    let reference = objects.put(&format!(
        r#"{{"asset_class":"{class}","instrument_id":"{asset}","symbol":"{symbol}"}}"#
    ));
    format!(r#"{{"kind":"instrument_snapshot","content_hash":"{reference}"}}"#)
}

fn registration(seq: u64, payload: &str) -> StoredEvent {
    row(seq, "paper", "ConfigSnapshotRegistered", payload)
}

/// The `(asset id, symbol)` of each registered instrument, in order.
fn listed(registered: &Registered) -> Vec<(String, String)> {
    match registered {
        Registered::Instruments(instruments) => instruments
            .iter()
            .map(|i| (i.asset_id.clone(), i.symbol.as_str().to_owned()))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, s)| ((*a).to_owned(), (*s).to_owned()))
        .collect()
}

#[test]
fn each_asset_id_s_latest_snapshot_names_its_symbol_in_first_registration_order() {
    let mut objects = Objects::default();
    let control = vec![
        registration(1, &snapshot(&mut objects, "b", "SPY", "us_equity")),
        row(2, "paper", "ModelRegistered", r#"{"kind":"model"}"#),
        registration(3, &snapshot(&mut objects, "a", "QQQ", "us_equity")),
        registration(4, &snapshot(&mut objects, "b", "SPYG", "us_equity")),
        registration(5, r#"{"kind":"fee_config","content_hash":"sha256:00"}"#),
        registration(6, &snapshot(&mut objects, "c", "BTC/USD", "crypto")),
        registration(7, &snapshot(&mut objects, "d", "IWM", "us_equity")),
        registration(8, &snapshot(&mut objects, "a", "QQQ", "us_equity")),
    ];
    assert_eq!(
        listed(&registered(&control, &objects)),
        pairs(&[("b", "SPYG"), ("a", "QQQ"), ("d", "IWM")]),
        "b first registered first, renamed SPYG later; c is not a US equity"
    );
}

#[test]
fn two_asset_ids_naming_one_symbol_or_a_symbol_that_is_not_one_get_none() {
    let mut objects = Objects::default();
    let control = vec![
        registration(1, &snapshot(&mut objects, "a", "QQQ", "us_equity")),
        registration(2, &snapshot(&mut objects, "b", "SPY", "us_equity")),
        registration(3, &snapshot(&mut objects, "c", "SPY", "us_equity")),
        registration(4, &snapshot(&mut objects, "d", "IWM", "us_equity")),
        registration(5, &snapshot(&mut objects, "e", "dia", "us_equity")),
        registration(6, &snapshot(&mut objects, "f", "DIA", "us_equity")),
    ];
    assert_eq!(
        listed(&registered(&control, &objects)),
        pairs(&[("a", "QQQ"), ("d", "IWM"), ("f", "DIA")]),
        "b and c share SPY; e's symbol is not one"
    );
    let nameless = objects.put(r#"{"asset_class":"us_equity","instrument_id":"a"}"#);
    let mut renamed = control.clone();
    renamed.push(registration(
        7,
        &format!(r#"{{"kind":"instrument_snapshot","content_hash":"{nameless}"}}"#),
    ));
    assert_eq!(
        listed(&registered(&renamed, &objects)),
        pairs(&[("d", "IWM"), ("f", "DIA")]),
        "a's latest snapshot names no symbol, so a gets none, never its older symbol"
    );
}

/// A registration whose snapshot cannot be read could name any asset id, so no instrument's
/// symbol can be trusted: the workspace gets no bars at all, as the composer gets no models.
#[test]
fn a_snapshot_that_does_not_read_gives_no_instruments() {
    let mut objects = Objects::default();
    let good = registration(1, &snapshot(&mut objects, "a", "QQQ", "us_equity"));
    for payload in [
        r#"{"kind":"instrument_snapshot","content_hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
        r#"{"kind":"instrument_snapshot"}"#,
        r#"{"kind":"instrument_snapshot","content_hash":"sha256:zz"}"#,
    ] {
        let control = vec![good.clone(), registration(2, payload)];
        assert!(
            matches!(registered(&control, &objects), Registered::Unreadable),
            "{payload}"
        );
    }
    let garbled = objects.put("not json");
    let control = vec![
        good.clone(),
        registration(
            2,
            &format!(r#"{{"kind":"instrument_snapshot","content_hash":"{garbled}"}}"#),
        ),
    ];
    assert!(matches!(
        registered(&control, &objects),
        Registered::Unreadable
    ));
    let mut broken = row(3, "paper", "ConfigSnapshotRegistered", "{}");
    broken.body = b"not json".to_vec();
    assert!(matches!(
        registered(&[good, broken], &objects),
        Registered::Unreadable
    ));
}

#[test]
fn at_most_fifty_the_first_registered_first() {
    let mut objects = Objects::default();
    let control: Vec<StoredEvent> = (0..60_u64)
        .map(|n| {
            registration(
                n + 1,
                &snapshot(
                    &mut objects,
                    &format!("id{n:02}"),
                    &format!("S{n:02}"),
                    "us_equity",
                ),
            )
        })
        .rev()
        .enumerate()
        .map(|(n, mut row)| {
            row.seq = u64::try_from(n).unwrap() + 1;
            row
        })
        .collect();
    let got = listed(&registered(&control, &objects));
    assert_eq!(BARS_INSTRUMENTS, 50);
    assert_eq!(got.len(), 50);
    assert_eq!(got.first().unwrap().1, "S59", "the first registered");
    assert_eq!(got.last().unwrap().1, "S10");
}

#[test]
fn any_row_not_of_the_paper_environment_gets_nothing() {
    let mut objects = Objects::default();
    let paper = registration(1, &snapshot(&mut objects, "a", "QQQ", "us_equity"));
    for environment in ["live", "", "Paper"] {
        let control = vec![
            paper.clone(),
            row(2, environment, "ModelRegistered", r#"{"kind":"model"}"#),
        ];
        assert!(
            matches!(registered(&control, &objects), Registered::NotPaper),
            "{environment:?}"
        );
    }
    assert_eq!(
        listed(&registered(&[paper], &objects)),
        pairs(&[("a", "QQQ")])
    );
    assert_eq!(listed(&registered(&[], &objects)), pairs(&[]));
}
