//! The id and symbol grammars: what the model and the readers may name, bounded so neither can grow
//! the journal or the prompt without limit.

use mandate_research_run::{DropReason, ItemId, ItemKind, ResearchRunError, SourceId, Symbol};

#[test]
fn ids_take_ascii_word_characters_and_the_four_separators_up_to_128_bytes() {
    for good in [
        "n-1",
        "bar:AAPL:1792094400",
        "src.newswire",
        "a/b_c",
        &"x".repeat(128),
    ] {
        assert!(ItemId::new(good).is_ok(), "{good}");
        assert!(SourceId::new(good).is_ok(), "{good}");
    }
    for bad in ["", " ", "n 1", "n\n1", "ü", "n;1", &"x".repeat(129)] {
        assert_eq!(
            ItemId::new(bad).map(|_| ()),
            Err(ResearchRunError::InvalidId { what: "item id" }),
            "{bad:?}"
        );
        assert_eq!(
            SourceId::new(bad).map(|_| ()),
            Err(ResearchRunError::InvalidId { what: "source id" }),
            "{bad:?}"
        );
    }
    assert_eq!(
        ItemId::new("n-1").map(|id| id.as_str().to_owned()),
        Ok("n-1".to_owned())
    );
}

#[test]
fn symbols_are_upper_case_tickers_up_to_16_bytes() {
    for good in ["AAPL", "BRK.B", "BTC/USD", "RDS-A", "A", "ABCDEFGHIJKLMNOP"] {
        assert_eq!(
            Symbol::new(good).map(|s| s.as_str().to_owned()),
            Ok(good.to_owned())
        );
    }
    for bad in ["", "aapl", "AAPL ", "ABCDEFGHIJKLMNOPQ", "AA_PL", "ÄPL"] {
        assert_eq!(
            Symbol::new(bad).map(|_| ()),
            Err(ResearchRunError::InvalidId { what: "symbol" }),
            "{bad:?}"
        );
    }
}

#[test]
fn kinds_and_drop_reasons_write_their_journal_names() {
    let kinds: Vec<&str> = [
        ItemKind::Bar,
        ItemKind::News,
        ItemKind::Filing,
        ItemKind::Instrument,
    ]
    .iter()
    .map(|kind| kind.as_str())
    .collect();
    assert_eq!(kinds, vec!["bar", "news", "filing", "instrument"]);
    let reasons: Vec<&str> = [
        DropReason::AfterCutOff,
        DropReason::BeforeWindow,
        DropReason::NotAllowlisted,
        DropReason::Licensed,
        DropReason::OutsideScope,
        DropReason::Duplicate,
        DropReason::OverCap,
        DropReason::Trimmed,
        DropReason::ReservedId,
    ]
    .iter()
    .map(|reason| reason.as_str())
    .collect();
    assert_eq!(
        reasons,
        vec![
            "after_cut_off",
            "before_window",
            "not_allowlisted",
            "licensed",
            "outside_scope",
            "duplicate",
            "over_cap",
            "trimmed",
            "reserved_id",
        ]
    );
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(
        ResearchRunError::Unimplemented { story: "x" }.code(),
        "unimplemented"
    );
    assert_eq!(
        ResearchRunError::InvalidId { what: "x" }.code(),
        "invalid_id"
    );
    assert_eq!(
        ResearchRunError::ObservationsMismatch {
            items: 1,
            observations: 2
        }
        .code(),
        "observations_mismatch"
    );
    assert_eq!(ResearchRunError::TimeOutOfRange.code(), "time_out_of_range");
}
