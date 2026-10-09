//! The paper adapter's arguments (E7-19 slice 5, the first paper trade brief's E1a, DEC-846 item
//! 6): opaque ids, the journal, the store, the bars and two acknowledgements, and nothing that
//! could name a mandate, a configuration, a model output or a host (FT-3, FT-4, FT-10).

use std::path::PathBuf;

use mandate_paper::{Args, PaperError, parse};

const IDS: &str = "--workspace w --agent a --account-ref r --store s --bars b";
/// [`IDS`] without `flag` and its value.
fn without(flag: &str) -> String {
    let words: Vec<&str> = IDS.split(' ').collect();
    let at = words.iter().position(|word| *word == flag).unwrap();
    [&words[..at], &words[at + 2..]].concat().join(" ")
}

fn parsed(text: &str) -> Result<Args, PaperError> {
    parse(text.split_whitespace().map(str::to_owned))
}

/// Asserts that `text` is a usage error, for the reason `why`.
fn usage(text: &str, why: &str) {
    let outcome = parsed(text);
    let refused = matches!(outcome, Err(PaperError::Usage(_)));
    assert!(refused, "{why}: {text}: {outcome:?}");
}

/// Every flag reaches its own field, and only `--place-one-order` asks to place (FT-1).
#[test]
#[ignore = "pending E7-19"]
fn each_flag_reaches_its_field() {
    let placing = parsed(&format!(
        "{IDS} --journal d --confirm-paper --place-one-order"
    ));
    let expected = Args {
        workspace: "w".to_owned(),
        agent: "a".to_owned(),
        account_ref: "r".to_owned(),
        journal: Some("d".to_owned()),
        store: PathBuf::from("s"),
        bars: PathBuf::from("b"),
        place_one_order: true,
    };
    assert_eq!(placing.unwrap(), expected);
    let dry = parsed(&format!("{IDS} --confirm-paper")).unwrap();
    assert_eq!((dry.journal, dry.place_one_order), (None, false));
}

/// The paper acknowledgement is required, a placement needs its journal (DEC-157 item 6), every
/// id, path and value is required, and no argument names a mandate, configuration, output or host.
#[test]
#[ignore = "pending E7-19"]
fn anything_else_is_a_usage_error() {
    usage(IDS, "--confirm-paper is required");
    let placing = format!("{IDS} --confirm-paper --place-one-order");
    usage(&placing, "a placement needs --journal");
    for flag in IDS.split(' ').step_by(2) {
        let others = without(flag);
        assert_eq!(others.split_whitespace().count(), 8, "{others}");
        usage(
            &format!("{others} --confirm-paper"),
            &format!("{flag} is required"),
        );
        let empty = format!("{others} {flag} --confirm-paper");
        usage(&empty, &format!("{flag} needs a value"));
    }
    for extra in [
        "--mandate m",
        "--config-dir c",
        "--model-output o",
        "--host h",
        "--dataset d",
    ] {
        usage(
            &format!("{IDS} --confirm-paper {extra}"),
            "{extra} is no input",
        );
    }
}
