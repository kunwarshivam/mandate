//! The in-process fixture is the only broker the executor can reach (E21-30, DEC-2033 item 7,
//! `AGENTS.md` rule 8). `xtask/layers.toml` holds it: no external crate but `tokio`, so no HTTP
//! client; none of the crates that carry a transport to a host or a simulator; and no `live`
//! feature, which only the paper runner may declare. This test pins that entry, so loosening it
//! fails here as well as in review.

use std::fs;
use std::path::Path;

type Error = Box<dyn std::error::Error>;

/// The `[crates.mandate-executord]` entry's lines, up to the next entry.
fn entry() -> Result<Vec<String>, Error> {
    let layers = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/layers.toml");
    let text = fs::read_to_string(layers)?;
    Ok(text
        .lines()
        .skip_while(|line| line.trim() != "[crates.mandate-executord]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(str::to_owned)
        .collect())
}

fn value(entry: &[String], key: &str) -> Option<String> {
    entry.iter().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

#[test]
fn the_executor_reaches_no_broker_but_its_in_process_fixture() -> Result<(), Error> {
    let entry = entry()?;
    assert!(!entry.is_empty(), "no mandate-executord entry");
    assert_eq!(
        value(&entry, "allowed_external").as_deref(),
        Some(r#"["tokio"]"#)
    );
    assert_eq!(value(&entry, "live_feature"), None);
    let forbidden = value(&entry, "forbidden_internal").ok_or("no forbidden_internal")?;
    for transport in [
        "mandate-paper",
        "mandate-shell",
        "mandate-robinhood",
        "mandate-rh-sim",
    ] {
        assert!(
            forbidden.contains(&format!("\"{transport}\"")),
            "{transport} is reachable"
        );
    }
    Ok(())
}
